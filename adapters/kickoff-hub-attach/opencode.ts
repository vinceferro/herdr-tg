/**
 * The watcher — seam ② for opencode, folded into the one command.
 *
 * It watches an opencode server's `/event` stream and turns the two things opencode already knows
 * how to say — "I am asking this, and here are the options" and "I need permission to do this" —
 * into the hub's `ask`, then posts the operator's tap back as a reply.
 *
 * # It is a PRODUCER at attach's own door, in the same process
 *
 * This used to be a process of its own (`adapters/opencode-bridge/bridge.ts`) that dialled the
 * relay with two variables and not a line of it changing. Collapsing the process boundary changes
 * nothing on the wire: the watcher holds a `HubLink` whose socket is attach's OWN door, and whose
 * `hello` carries the secret, the address and an instance of its own. `relay.ts` greets it, numbers
 * it, namespaces its ask ids, shares the queue with it, and — when the hub link drops — ends its
 * connection exactly as it ends every other producer's. One routing path, one greeting path, one
 * ledger; the machinery that exists for two producers is not duplicated for a third that happens to
 * live in-process. `docs/ATTACHING.md` §13.2 is the argument in full.
 *
 * # Inbound content selects; it never names
 *
 * Every button this watcher mints comes from a list opencode published. A tap comes back as an
 * `option_id` this watcher wrote down, and it is looked up in that record — never used to address
 * anything. The operator picks one of the answers the machine already offered; he cannot name a
 * tool, a file or a command.
 *
 * # The wire is not written here
 *
 * The queue, the framing, the reconnect and the three-outcome vocabulary are `hub-link.ts`, the
 * same module the door and the tool server use. A fork of it drifted by twelve fixed defects once;
 * a test fails if any adapter starts writing its own again.
 */

import type { Project } from '../../plugins/kickoff-channel/where.ts'
import { HubLink, MAX_FRAME_BYTES, type Delivery, type Outbound } from '../../plugins/kickoff-channel/hub-link.ts'

/** What the watcher needs, worked out by `main.ts` from the namespace and the door it opened. */
export type WatcherConfig = {
  /** attach's own door — the watcher dials it as a producer, exactly as the old bridge dialled a relay. */
  door: string
  /** The conversation attach holds, echoed in `hello` and checked in `welcome`. */
  address: string | null
  /** The opencode server to watch. Seam ②, and deliberately outside the namespace. */
  opencodeUrl: string
  /**
   * The enrolled project, resolved AFRESH on every attempt — the watcher presents the same secret
   * the door authenticated with, so the door's defence-in-depth token check passes.
   */
  secretOf: () => Project | null
  /** The directory to name in the "not enrolled" instruction. */
  projectDir: string
  /** Say something in this process's own transcript. The operator cannot see it; a developer can. */
  note: (msg: string) => void
}

/** One open question, as it was published and as it was drawn. */
type Open = {
  /** `que_…` for a question, `per_…` for a permission request. */
  requestID: string
  sessionID: string
  kind: 'question' | 'permission'
  /**
   * Which family of event published it. opencode carries two, and they take the same body at
   * different URLs — the v2 endpoint is under `/api` and names the session, the v1 one does not.
   * Guessing here posts an answer at a path that 404s, and the question stays open on his phone
   * with the agent still waiting.
   */
  v2: boolean
  /** option_id → the label opencode published. A tap is looked up here and nowhere else. */
  labels: Map<string, string>
}

export function startWatcher(cfg: WatcherConfig): void {
  const { note } = cfg
  const OPENCODE = cfg.opencodeUrl.replace(/\/$/, '')
  const ADDRESS = cfg.address

  /** This run. A new instance invalidates every question the last one left open. */
  const INSTANCE = `${process.pid}-w-${Date.now()}`

  const open = new Map<string, Open>()

  /**
   * Bounded, because a server that never resolves its questions would otherwise make this a leak.
   * The oldest goes first: a question nobody answered in the last hundred is not being waited on.
   */
  const MAX_OPEN = 100
  function remember(askId: string, o: Open): void {
    open.set(askId, o)
    while (open.size > MAX_OPEN) {
      const oldest = open.keys().next()
      if (oldest.done) break
      open.delete(oldest.value)
    }
  }

  /**
   * Forget a question nothing can ever answer, and say so where a developer will see it.
   *
   * A record kept for a question that never reached a phone is worse than no record: it stays
   * answerable HERE for the next hundred questions, while no keyboard for it has ever existed. The
   * agent is meanwhile blocked, and there is nobody this watcher can tell — opencode has no channel
   * back into a turn — so the loudest thing available is this line.
   */
  function giveUpOn(askId: string, why: string): void {
    if (!open.delete(askId)) return
    note(`nothing can answer ${askId} any more: ${why}`)
  }

  const link = new HubLink({
    framePrefix: 'f',
    note,
    // The watcher dials attach's own door, which is up whenever this process is running. So the
    // sentence that names a missing relay can only be briefly true, between the door closing on a
    // hub outage and the watcher's redial finding it again.
    whenUnreachable: 'The relay that carries this project to his phone is not running.',
    identify() {
      const project = cfg.secretOf()
      if (!project) {
        // Retried, NOT permanent. The documented recovery from "not enrolled" is to run
        // `herdr-tg enroll` while the adapter is running.
        return {
          refuse: {
            permanent: true,
            why: `This project is not enrolled, so nothing from it reaches his phone. Run:  herdr-tg enroll ${cfg.projectDir}`,
            note: `no secret for ${cfg.projectDir}. Run:  herdr-tg enroll ${cfg.projectDir}`,
            retryMs: 30_000,
          },
        }
      }
      return {
        socket: cfg.door,
        hello: {
          t: 'hello',
          project_id: 'unknown-until-the-hub-says',
          token: project.token,
          instance: INSTANCE,
          repo: project.repo,
          pid: process.pid,
          // Omitted entirely when there is no address. Never `"lane": null`.
          ...(ADDRESS ? { lane: ADDRESS } : {}),
        },
      }
    },
    onFrame: fromHub,
    onLost,
  })

  /**
   * Frames the link let go because nothing but a person can mend the gap.
   *
   * Every one of them was a question or a permission prompt an agent is still blocked on.
   */
  function onLost(lost: Outbound[], why: string): void {
    for (const o of lost) if (o.askId) giveUpOn(o.askId, why)
    note(`${lost.length} frame(s) will never go out: ${why}`)
  }

  /** Send, and act on what actually happened to it. */
  function say(payload: Record<string, unknown>, what: string, askId?: string): Delivery {
    const d = link.send(payload, what, askId)
    if (!d.delivered && d.permanent) {
      note(`not sent (${what}): ${d.why}`)
      if (askId) giveUpOn(askId, d.why)
    }
    return d
  }

  function fromHub(f: Record<string, any>): void {
    switch (f.t) {
      case 'welcome': {
        // A watcher that named an address and was not given it back is talking to something older
        // than itself, and it must NOT go up. Against attach's own door this cannot happen — the
        // door always echoes the lane it holds — but the check is kept as the same defence the tool
        // server keeps, so the watcher never impersonates a project.
        if (ADDRESS && f.lane !== ADDRESS) {
          link.markDown(
            true,
            `The door did not confirm ${ADDRESS} — it is holding a different conversation.`,
          )
          note(`the door did not confirm ${ADDRESS}`)
          link.end()
          return
        }
        note(`the watcher is attached${ADDRESS ? ` · ${ADDRESS}` : ''}`)
        link.markUp()
        return
      }
      case 'refused': {
        const why = String(f.reason)
        const forGood: Record<string, string> = {
          unknown_project: `the hub does not know ${cfg.projectDir}`,
          bad_token: `the secret for ${cfg.projectDir} is not one the hub knows`,
          not_enabled: 'this project is enrolled but switched off',
          version_skew: 'the watcher and the door do not speak the same version',
          bad_lane: `the door will not address ${ADDRESS ?? 'this conversation'}`,
        }
        const forNow: Record<string, string> = {
          // Temporary, always. The bridge this came from counted three of these and gave up for
          // good — on its own asks, the prompts an agent is blocked on. Here every `already_claimed`
          // is the DOOR relaying the hub's refusal, at the hub link's own cadence, so three of them
          // is three seconds: shorter than the ten a predecessor attach gets to stop. The door
          // decides when a claim is stuck (`relay.ts`, by elapsed time), and when it does, it
          // answers each of this watcher's queued frames with an `ack` saying no — which the `ack`
          // branch below turns into giving up on that one question. A second rule here would only
          // ever be the wrong one.
          already_claimed: 'another voice already holds this conversation at the door; waiting for it to go',
          frame_too_large: 'a frame was refused for being too large',
        }
        const said = forGood[why] ?? forNow[why]
        link.markDown(why in forGood, said ?? `refused for a reason this watcher does not know (${why})`)
        note(said ?? `refused: ${why}`)
        return
      }
      case 'ack': {
        // `unseen` is NOT success: it means the send went out and could not be confirmed, and it is
        // never retried, because Telegram has no idempotency key and a second copy of a question
        // would leave two live keyboards for it.
        const was = link.frameInFlight(String(f.ref))
        link.forgetInFlight(String(f.ref))
        if (f.delivered === 'yes') {
          if (f.why === 'clamped' && was) note(`${was.what} arrived on his phone clipped short`)
          return
        }
        const why = f.delivered === 'no' ? String(f.why ?? 'no reason given') : 'the hub could not confirm it arrived'
        note(`the hub did not deliver ${was?.what ?? 'a frame'} (${why})`)
        if (was?.askId) giveUpOn(was.askId, why)
        return
      }
      case 'choice':
        void answer(String(f.ask_id), String(f.option_id))
        return
      case 'message':
        // The operator typed at this project. opencode's own prompt endpoint is the place for this,
        // and it is deliberately not wired yet: steering a session by text is a second decision.
        note('the operator typed something; passing typed steering to opencode is not built yet')
        return
      default:
        // Unknown kind ignored, so something shipped after this cannot kill the link by being newer.
        // `ping` never arrives here — `hub-link.ts` answers it — so liveness never waits on this switch.
        return
    }
  }

  /**
   * Where an answer goes. Four endpoints, and the spec is the only authority on which is which:
   * the v2 pair live under `/api` and name the session in the path, the v1 pair do not.
   */
  function replyUrl(o: Pick<Open, 'kind' | 'v2' | 'sessionID' | 'requestID'>): string {
    const sid = encodeURIComponent(o.sessionID)
    const rid = encodeURIComponent(o.requestID)
    if (o.kind === 'question') {
      return o.v2
        ? `${OPENCODE}/api/session/${sid}/question/${rid}/reply`
        : `${OPENCODE}/question/${rid}/reply`
    }
    return o.v2
      ? `${OPENCODE}/api/session/${sid}/permission/${rid}/reply`
      : `${OPENCODE}/permission/${rid}/reply`
  }

  /** Post the operator's tap back to opencode, as the label it published. */
  async function answer(askId: string, optionId: string): Promise<void> {
    const o = open.get(askId)
    if (!o) {
      note(`a tap arrived for ${askId}, which this watcher has no record of; nothing was answered`)
      return
    }
    const label = o.labels.get(optionId)
    if (label === undefined) {
      note(`a tap named an option ${askId} never offered; nothing was answered`)
      return
    }
    open.delete(askId)
    const url = replyUrl(o)
    // Both question endpoints take the same body — the answers to each question in order, and each
    // answer is the list of labels chosen for it. One question, one label.
    const body = o.kind === 'question' ? { answers: [[label]] } : { reply: label }
    try {
      const r = await fetch(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
      })
      if (!r.ok) {
        note(`opencode refused the answer to ${askId} (${r.status})`)
        return
      }
      // No `ask_resolved` here. The hub already took the buttons off when it resolved the tap, and
      // said so in his own words. A second retirement from this side would overwrite that.
      note(`opencode took the answer to ${askId}`)
    } catch (e) {
      note(`could not reach opencode to answer ${askId}: ${(e as Error)?.message ?? e}`)
    }
  }

  /** One opencode event, mapped onto the hub's vocabulary. Exported shape kept identical to before. */
  function onOpencodeEvent(ev: Record<string, any>): void {
    const type = String(ev?.type ?? '')
    // opencode carries the same payload under two names. `/event` nests it in `properties`; the
    // durable per-session stream nests it in `data`. Reading only one is not a parse error — every
    // field simply comes back undefined, and the first real permission request reached the phone
    // fine and then answered nothing, because its ask id was the string "pundefined".
    const data = ev?.properties ?? ev?.data ?? {}
    switch (type) {
      case 'question.v2.asked':
      case 'question.asked': {
        const questions: any[] = Array.isArray(data.questions) ? data.questions : []
        // opencode can publish several questions in one request. Only the first is drawn: the reply
        // shape answers them in order, and a phone that shows two keyboards for one request cannot
        // say which one an answer belonged to.
        const q = questions[0]
        if (!q || !Array.isArray(q.options) || q.options.length === 0) {
          note('a question arrived with no options; it needs a keyboard this watcher cannot draw')
          return
        }
        const askId = `q${data.id}`
        const labels = new Map<string, string>()
        const options = q.options.slice(0, 3).map((o: any, i: number) => {
          const id = `o${i}`
          labels.set(id, String(o.label))
          return { option_id: id, label: String(o.label) }
        })
        remember(askId, {
          requestID: String(data.id),
          sessionID: String(data.sessionID),
          kind: 'question',
          v2: type.includes('.v2.'),
          labels,
        })
        const more =
          questions.length > 1 ? `\n\n(it asked ${questions.length} things at once; this is the first)` : ''
        const trimmed = q.options.length > options.length ? `\n\n(showing ${options.length} of ${q.options.length} choices)` : ''
        say({ t: 'ask', ask_id: askId, text: `${q.question}${more}${trimmed}`, options }, `a question (${askId})`, askId)
        return
      }
      case 'permission.v2.asked':
      case 'permission.asked': {
        // The reply set is opencode's own and it is closed: once, always, reject. The operator picks
        // one of three; he never names the action, and this watcher never invents a fourth.
        const askId = `p${data.id}`
        const labels = new Map<string, string>([
          ['once', 'once'],
          ['always', 'always'],
          ['reject', 'reject'],
        ])
        remember(askId, {
          requestID: String(data.id),
          sessionID: String(data.sessionID),
          kind: 'permission',
          v2: type.includes('.v2.'),
          labels,
        })
        const what = Array.isArray(data.resources) && data.resources.length
          ? `${data.action}: ${data.resources.join(', ')}`
          : String(data.action ?? 'something')
        say(
          {
            t: 'ask',
            ask_id: askId,
            text: `It wants to ${what}`,
            options: [
              { option_id: 'once', label: 'Just this once' },
              { option_id: 'always', label: 'Always' },
              { option_id: 'reject', label: 'No' },
            ],
          },
          `a permission request (${askId})`,
          askId,
        )
        return
      }
      case 'question.v2.replied':
      case 'question.v2.rejected':
      case 'permission.v2.replied': {
        // Answered somewhere else — at the keyboard, or by a saved rule. The buttons come off, so a
        // stale keyboard cannot be tapped an hour later. This is the frame no screen could produce.
        const id = String(data.requestID ?? data.id ?? '')
        const askId = (type.startsWith('question') ? 'q' : 'p') + id
        if (!open.has(askId)) return
        open.delete(askId)
        say(
          { t: 'ask_resolved', ask_id: askId, how: type.endsWith('rejected') ? 'withdrawn' : 'answered' },
          `retiring ${askId}`,
        )
        return
      }
      case 'session.idle':
        say({ t: 'beat', state: 'idle' }, 'a heartbeat')
        return
      default:
        return
    }
  }

  /**
   * Follow the server's event stream, reconnecting for as long as this process lives.
   *
   * A stream that ends is normal — the server restarts, a proxy times it out — so the end of one is
   * not an error and never stops the loop.
   */
  async function watch(): Promise<void> {
    for (;;) {
      try {
        const r = await fetch(`${OPENCODE}/event`, { headers: { accept: 'text/event-stream' } })
        if (!r.ok || !r.body) throw new Error(`opencode answered ${r.status}`)
        note(`watching ${OPENCODE}`)
        const reader = r.body.getReader()
        const dec = new TextDecoder()
        let buf = ''
        for (;;) {
          const { done, value } = await reader.read()
          if (done) break
          buf += dec.decode(value, { stream: true })
          for (;;) {
            const nl = buf.indexOf('\n')
            if (nl < 0) break
            const line = buf.slice(0, nl).trim()
            buf = buf.slice(nl + 1)
            if (!line.startsWith('data:')) continue
            try {
              onOpencodeEvent(JSON.parse(line.slice(5).trim()))
            } catch {
              // One unreadable event never ends the stream.
            }
          }
          if (buf.length > MAX_FRAME_BYTES) buf = ''
        }
      } catch (e) {
        note(`lost the opencode stream: ${(e as Error)?.message ?? e}`)
      }
      await new Promise(r => setTimeout(r, 2000))
    }
  }

  link.start()
  void watch()
}
