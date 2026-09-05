/**
 * The watcher — seam ② for opencode, folded into the one command.
 *
 * It watches an opencode server's `/event` stream and turns the two things opencode already knows
 * how to say — "I am asking this, and here are the options" and "I need permission to do this" —
 * into the hub's `ask`, then posts the operator's tap back as a reply. And it carries the other
 * half of a phone: what the operator TYPES at the conversation goes to the session as a prompt,
 * verbatim, the way a Claude session gets it in its own turn (`carry`, below).
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
 * tool, a file or a command. His typed words are the same rule from the other side: they travel
 * only as the text of a prompt, and WHICH session gets them is the server's answer to a question
 * this watcher asks about the project directory — nothing in the text can pick a session, a path
 * or a URL.
 *
 * # The wire is not written here
 *
 * The queue, the framing, the reconnect and the three-outcome vocabulary are `hub-link.ts`, the
 * same module the door and the tool server use. A fork of it drifted by twelve fixed defects once;
 * a test fails if any adapter starts writing its own again.
 */

import type { Project } from '../../plugins/kickoff-channel/where.ts'
import { HubLink, MAX_FRAME_BYTES, type Delivery, type Outbound, type Unanswered } from '../../plugins/kickoff-channel/hub-link.ts'

/** What the watcher needs, worked out by `main.ts` from the namespace and the door it opened. */
export type WatcherConfig = {
  /** attach's own door — the watcher dials it as a producer, exactly as the old bridge dialled a relay. */
  door: string
  /** The conversation attach holds, echoed in `hello` and checked in `welcome`. */
  address: string | null
  /**
   * This run of the watcher, as its `hello` names it. Minted by `main.ts` rather than here because
   * the door has to know it: of the producers behind the door this is the one that carries the
   * operator's typed words, and the door forwards its answer about them over a tool server's.
   */
  instance: string
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
  const INSTANCE = cfg.instance

  /**
   * How long one request to the server may take before it is given up on.
   *
   * Bun's `fetch` waits for ever by default, and typed lines are carried one after another so
   * their order is kept — so one request the server accepted and never answered parked that line
   * AND every line typed after it, none of them acked: the hub went on believing each was read,
   * and the operator went on typing at a wall that took none of it. Measured: two messages,
   * twelve seconds, no ack for either. The measured round trip is 4–24 ms; a server that has not
   * answered in ten seconds is not going to.
   */
  const OPENCODE_ANSWERS_WITHIN_MS = 10_000

  const open = new Map<string, Open>()

  /**
   * Sessions his typed words were handed to and that have not finished a turn since.
   *
   * 204 from `prompt_async` means opencode wrote the words down; the agent has not run. When it
   * then cannot — the gateway down, a provider key expired, the context overflowed — opencode says
   * so as `session.error`, and until this existed nothing here listened: the hub had posted
   * nothing, and the only record was a stack trace in a stream nobody watches. Cleared by the
   * session's next `session.idle`, which is a turn that ran. Bounded, oldest first, for a server
   * that never goes idle.
   */
  const prompted = new Map<string, number>()
  const MAX_PROMPTED = 100
  function rememberPrompted(sessionID: string): void {
    prompted.delete(sessionID)
    prompted.set(sessionID, Date.now())
    while (prompted.size > MAX_PROMPTED) {
      const oldest = prompted.keys().next()
      if (oldest.done) break
      prompted.delete(oldest.value)
    }
  }

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
    onUnanswered,
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

  /**
   * Frames the door took and the connection ended before it answered for them.
   *
   * The same fact the hub's own `unseen` carries, learned one hop earlier, and handled the same way:
   * a question nobody can confirm was asked is given up on here, never asked again — a second copy
   * would leave two live keyboards for one answer.
   */
  function onUnanswered(gone: Unanswered[], why: string): void {
    for (const o of gone) if (o.askId) giveUpOn(o.askId, why)
    note(`${gone.length} frame(s) went out and nobody knows what became of them: ${why}`)
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
        // The operator typed at this conversation. Carried one at a time, in the order the lines
        // arrived: two lines typed a second apart are one thought, and two lookups racing could
        // put the second in front of the agent before the first.
        carrying = carrying
          .then(() => carry(f))
          .catch(e => {
            // The line must never be poisoned: a rejected promise here would skip every later
            // `carry`, and every line typed after it would be neither posted nor acked — the
            // silence this whole path exists to end. `carry` catches its own failures; this is
            // for the one it cannot foresee, and the hub is still answered.
            note(`could not carry the operator's words: ${(e as Error)?.message ?? e}`)
            answerFor(String(f.id), 'the worker could not take it')
          })
        return
      default:
        // Unknown kind ignored, so something shipped after this cannot kill the link by being newer.
        // `ping` never arrives here — `hub-link.ts` answers it — so liveness never waits on this switch.
        return
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────────────────────
  // Typed steering.

  /** The line of typed messages, carried one after another — see `case 'message'`. */
  let carrying: Promise<void> = Promise.resolve()

  /**
   * The operator typed at this conversation, and his words become a PROMPT to the session —
   * verbatim, the way a Claude session gets them in its own turn. Nothing is put in front of them
   * and nothing in them is read: the session is the machine's answer (`sessionForTypedWords`), the
   * URL is built from that answer and the `--opencode` flag, and the words travel only as the text
   * of the body. `from` is not shown to the agent, exactly as the Claude adapter does not show it.
   *
   * The endpoint and the body were captured from opencode 1.18.25 on 5 September, not guessed —
   * the last time this adapter guessed an opencode shape every test agreed with the guess.
   * `POST /session/{id}/prompt_async` with `{parts: [{type: 'text', text}]}` answers 204 at once
   * and runs the agent with the session's own model and agent. The v2 `/api/session/{id}/prompt`,
   * the one a reader of the spec reaches for first, admitted the prompt, emitted two events, and
   * ran nothing. A prompt to a session blocked on its own question is taken (204), written down,
   * and run once the question is answered.
   *
   * Every `message` is answered on the wire with `ack{ref, status, reason?}`. `refused` carries a
   * reason in the operator's own register, because the hub puts it in the topic he typed in — the
   * one place he can learn that a line he wrote reached nobody. Until this existed his words
   * reached a line on stderr saying it was not built, and nothing at all reached him.
   */
  async function carry(f: Record<string, any>): Promise<void> {
    const ref = String(f.id)
    if (typeof f.text !== 'string') {
      answerFor(ref, 'the message arrived without any words in it')
      return
    }
    const replyTo = f.in_reply_to_ask === undefined || f.in_reply_to_ask === null ? null : String(f.in_reply_to_ask)
    const target = await sessionForTypedWords(replyTo)
    if ('refused' in target) {
      note(`the operator's words were not handed on: ${target.refused}`)
      answerFor(ref, target.refused)
      return
    }
    const sid = encodeURIComponent(target.sessionID)
    try {
      const r = await fetch(`${OPENCODE}/session/${sid}/prompt_async`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ parts: [{ type: 'text', text: f.text }] }),
        signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS),
      })
      if (!r.ok) {
        // The status is for a developer, here. The reason goes verbatim into his topic, where a
        // number is jargon; and it is "it", because the hub's sentence is about "what you typed".
        note(`opencode would not take the operator's words (${r.status})`)
        answerFor(ref, "the worker's server would not take it")
        return
      }
      note(`the operator's words went to ${target.how} as a prompt`)
      rememberPrompted(target.sessionID)
      answerFor(ref)
    } catch (e) {
      note(`could not reach opencode with the operator's words: ${(e as Error)?.message ?? e}`)
      answerFor(ref, unreached(e))
    }
  }

  /** What he is told when a request to the server ended without an answer. */
  function unreached(e: unknown): string {
    return (e as Error)?.name === 'TimeoutError'
      ? "the worker's server did not answer in time"
      : "the worker's server could not be reached"
  }

  /** Tell the hub what became of one `message`, on the wire. The hub acks this like any frame. */
  function answerFor(ref: string, refused?: string): void {
    say(
      refused ? { t: 'ack', ref, status: 'refused', reason: refused } : { t: 'ack', ref, status: 'accepted' },
      'an answer about typed words',
    )
  }

  /**
   * Which session his words go to. The MACHINE's answer, in this order, and never the text's:
   *
   *   1. Typed under a question this watcher asked and still holds open: the session that asked
   *      it. It is the one time the operator has said which session he means. The words are
   *      still a prompt and not an answer to the question — its answers are the buttons opencode
   *      published, and a permission takes three words and no others — so the question stays open
   *      for his tap, and opencode runs the words once it is answered (measured, 5 September).
   *   2. Otherwise the session this wall's server is running for the project directory attach
   *      speaks for: `GET /session?directory=<it>&roots=true`, which the server answers most
   *      recently updated first. The operator's decision is one server per wall, so this is
   *      usually one; when it is several, the one that moved last is the one whose words he is
   *      reading. Root sessions only, because a subagent's session is not the conversation on his
   *      phone; and nothing archived. Measured: the directory match takes a trailing slash and a
   *      symlink, and excludes a subfolder.
   *   3. None: refused, with a reason the hub can put in front of him. Never a guess — a guess is
   *      his words in a session he was not talking to.
   *
   * `GET /api/session/active` was measured and is NOT used: it sees only the v2 drains, and stayed
   * empty for the whole of a session driven through the v1 endpoint this watcher uses.
   */
  async function sessionForTypedWords(
    inReplyTo: string | null,
  ): Promise<{ sessionID: string; how: string } | { refused: string }> {
    if (inReplyTo) {
      const asked = open.get(inReplyTo)
      if (asked) return { sessionID: asked.sessionID, how: `the session that asked ${inReplyTo}` }
    }
    let list: unknown
    try {
      const r = await fetch(`${OPENCODE}/session?directory=${encodeURIComponent(cfg.projectDir)}&roots=true`, {
        signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS),
      })
      if (!r.ok) {
        note(`opencode would not say which session is open (${r.status})`)
        return { refused: "the worker's server would not say which session is open" }
      }
      list = await r.json()
    } catch (e) {
      note(`could not ask opencode which session is open: ${(e as Error)?.message ?? e}`)
      return { refused: unreached(e) }
    }
    if (!Array.isArray(list)) return { refused: "the worker's server gave an answer that could not be read" }
    const candidates = list
      .filter((s: any) => s && typeof s.id === 'string' && s.id.startsWith('ses') && !s.parentID && !s.time?.archived)
      .sort((a: any, b: any) => Number(b.time?.updated ?? 0) - Number(a.time?.updated ?? 0))
    if (!candidates.length) {
      return { refused: 'the worker has no session open, so there was nothing to hand it to' }
    }
    return {
      sessionID: String(candidates[0].id),
      how: candidates.length === 1 ? 'the one session open' : `the most recently active of ${candidates.length} sessions`,
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
        // Not carried one after another as typed words are, so one stuck tap takes no other with
        // it — but a request that never answers is still a promise this process holds for ever.
        signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS),
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
      case 'session.error': {
        // The agent could not run on what he typed. opencode says so twice for one failure — the
        // second carrying a stack trace — and then `session.idle`; the first is answered, once,
        // and the session is forgotten so the second says nothing. Only for a session his words
        // went to and that has not finished a turn since: an error in a session nobody typed at
        // is the agent's own business, in its own terminal. The `ack` for his words was spent
        // when opencode wrote them down, so this is a line in the topic, in his words. The first
        // line of the message, control characters out: opencode's second event is a stack.
        const sid = String(data.sessionID ?? '')
        if (!sid || !prompted.delete(sid)) return
        const err = data.error ?? {}
        const said = firstLine(err?.data?.message) || firstLine(err?.name) || 'it did not say why'
        say(
          { t: 'say', text: `The agent could not act on what you typed: ${said}`, hint: 'prose' },
          'a word about typed words the agent could not act on',
        )
        return
      }
      case 'session.idle': {
        // A turn ended. Whatever he typed at this session has been read, so a later error in it
        // is not about his words.
        const sid = String(data.sessionID ?? '')
        if (sid) prompted.delete(sid)
        say({ t: 'beat', state: 'idle' }, 'a heartbeat')
        return
      }
      default:
        return
    }
  }

  /** The first line of a message, fit to put on a phone: no control characters, and not a stack. */
  function firstLine(s: unknown): string {
    if (typeof s !== 'string') return ''
    return s
      .split(/\r?\n/)[0]
      .replace(/[\u0000-\u001f\u007f]/g, ' ')
      .trim()
      .slice(0, 300)
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
