/**
 * The opencode adapter — seam ② for the other engine.
 *
 * One opencode server, one project, one hub connection. It watches the server's event stream and
 * turns the two things opencode already knows how to say — "I am asking this, and here are the
 * options" and "I need permission to do this" — into the hub's `ask`, then posts the operator's
 * tap back as a reply.
 *
 * # Why this is not the screen scraper
 *
 * The pane-reading design died because herdr's protocol carries agent status and raw screen bytes
 * and nothing about what an agent is ASKING, so every prompt had to be reconstructed from pixels
 * that had already thrown the structure away. opencode publishes `question.v2.asked` with real
 * `options[{label, description}]`, and `permission.v2.asked` with a closed reply set of
 * once/always/reject. Nothing here parses a screen, and nothing here invents a choice.
 *
 * # Inbound content selects; it never names
 *
 * Every button this bridge mints comes from a list opencode published. A tap comes back as an
 * `option_id` this bridge wrote down, and it is looked up in that record — never used to address
 * anything. The operator cannot name a tool, a file or a command; he can only pick one of the
 * answers the machine already offered.
 *
 * # One server, one project
 *
 * The server's own `/session` reports a `directory` for each session, and it is tempting to use it
 * to serve several projects from one server. It is not used for that, and must not be: `directory`
 * is data the server reports, and identity here is a secret the hub resolves. One bridge process
 * per project, addressed by URL, is also what survives each agent moving into its own container.
 *
 * # The wire is not written here any more
 *
 * This file used to carry its own copy of the queue, the framing and the reconnect. It drifted from
 * the reviewed one by twelve invariants, every one of them a defect that had already been found and
 * fixed on the other side: no drain handler, so a frame the kernel refused sat unsent until
 * something else happened to be sent; a half-written frame resumed rather than re-sent, which put a
 * headless tail on the next connection and made the hub close in silence; a backoff reset on
 * connect rather than on `welcome`, so a squatting claim became a permanent 1 Hz hammer; an unknown
 * refusal treated as permanent; a missing secret treated as permanent, which made "run herdr-tg
 * enroll while it is running" a lie for this adapter alone; and a `bye` written and then abandoned
 * on the same tick. It is `hub-link.ts` now — the same module the tool server and the relay use.
 *
 * What stays here is what is genuinely opencode's: which events become questions, where an answer
 * is posted, and the fact that a tap needs no retirement because the hub already made one.
 */

import { readConfig, secretFor } from '../../plugins/kickoff-channel/attach.ts'
import { HubLink, MAX_FRAME_BYTES, type Delivery, type Outbound } from '../../plugins/kickoff-channel/hub-link.ts'

function note(msg: string): void {
  process.stderr.write(`opencode-bridge: ${msg}\n`)
}

function die(msg: string): never {
  note(msg)
  process.exit(2)
}

/**
 * Which project, which conversation, and what to dial — from the one reader every adapter shares.
 *
 * `docs/ATTACHING.md` is the contract. This adapter used to fall back to its own cwd when nothing
 * named a project, which is the guess the other two refuse to make: an opencode server is routinely
 * started from somewhere that is not the repo, and a bridge that guessed would authenticate as
 * whatever repository happened to be above it.
 */
const READ = readConfig()
if ('problem' in READ) die(READ.problem.note)
const CONFIG = READ.config

/** The one opencode server this bridge watches. Seam ②, and deliberately outside the namespace. */
const OPENCODE = (process.env.OPENCODE_URL ?? 'http://127.0.0.1:9700').replace(/\/$/, '')

/** This run. A new instance invalidates every question the last one left open. */
const INSTANCE = `${process.pid}-${Date.now()}`

// ───────────────────────────────────────────────────────────────────────────────────────────────
// What the operator was offered, so a tap can be turned back into an opencode reply.

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
 * agent is meanwhile blocked on a permission prompt, and there is nobody this bridge can tell —
 * opencode has no channel back into a turn — so the loudest thing available is this line.
 */
function giveUpOn(askId: string, why: string): void {
  if (!open.delete(askId)) return
  note(`nothing can answer ${askId} any more: ${why}`)
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The hub link — the shared one.

/** The address this bridge names, so a `welcome` that does not echo it can be caught. */
const ADDRESS = CONFIG.address

const link = new HubLink({
  // Opaque to the hub, and only ever read in a log. Kept as it was so an existing transcript still
  // matches this process's frames.
  framePrefix: 'f',
  note,
  whenUnreachable: CONFIG.viaRelay
    ? 'The relay that carries this project to his phone is not running.'
    : 'The hub is not running, so nothing reaches his phone until it is back.',
  identify() {
    const project = secretFor(CONFIG)
    if (!project) {
      // Retried, NOT permanent. The documented recovery from "this project is not enrolled" is to
      // run `herdr-tg enroll` while the adapter is running, and the old fork made that impossible
      // for this adapter alone: it set a permanent flag, never scheduled another attempt, and said
      // so only on stderr.
      const where = CONFIG.tokenFile ?? `${CONFIG.projectDir}/.kickoff/hub.token`
      return {
        refuse: {
          permanent: true,
          why: `This project is not enrolled, so nothing from it reaches his phone. Run:  herdr-tg enroll ${CONFIG.projectDir}`,
          note: `no secret at ${where}. Run:  herdr-tg enroll ${CONFIG.projectDir}`,
          retryMs: 30_000,
        },
      }
    }
    return {
      socket: CONFIG.dial,
      hello: {
        t: 'hello',
        project_id: 'unknown-until-the-hub-says',
        token: project.token,
        instance: INSTANCE,
        repo: project.repo,
        pid: process.pid,
        // Omitted entirely when there is no address, which is byte for byte what this bridge sent
        // before addresses existed. Never `"lane": null`.
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
 * Every one of them was a question or a permission prompt an agent is still blocked on, and the old
 * fork simply kept them: they rotted in a queue whose reconnect could never happen, because the
 * same code path had already decided the failure was permanent.
 */
function onLost(lost: Outbound[], why: string): void {
  for (const o of lost) if (o.askId) giveUpOn(o.askId, why)
  note(`${lost.length} frame(s) will never go out: ${why}`)
}

/** Send, and act on what actually happened to it. */
function say(payload: Record<string, unknown>, what: string, askId?: string): Delivery {
  const d = link.send(payload, what, askId)
  // "Written to a live socket", "parked in a queue" and "refused outright" are three different
  // things, and the old fork returned one boolean for all three which every caller then discarded.
  if (!d.delivered && d.permanent) {
    note(`not sent (${what}): ${d.why}`)
    if (askId) giveUpOn(askId, d.why)
  }
  return d
}

function fromHub(f: Record<string, any>): void {
  switch (f.t) {
    case 'welcome': {
      // A bridge that named an address and was not given it back is talking to a hub older than
      // itself, and it must NOT go up: that hub ignored the unknown field and admitted this process
      // AS THE WHOLE PROJECT, taking the project's one claim and its topic while the project's own
      // voice is then refused.
      if (ADDRESS && f.lane !== ADDRESS) {
        link.markDown(
          true,
          `The hub on this machine is older than this bridge and cannot give ${ADDRESS} a place of its own.`,
        )
        note(`the hub did not confirm ${ADDRESS}; it is older than this bridge`)
        link.end()
        return
      }
      // The claim counter counts a RUN of consecutive refusals, and a connection that succeeded is
      // the end of any run. Without this line it is a lifetime tally instead: two ordinary
      // restarts months apart add up, the third trips "this holder is not letting go", and the
      // bridge takes itself down for good — emptying its queue and giving up on the question an
      // agent is blocked on, while telling whoever reads stderr to kill a process that does not
      // exist. opencode has no channel into a turn, so that agent simply hangs.
      heldByAnother = 0
      note(`connected as "${f.project}"${ADDRESS ? ` · ${ADDRESS}` : ''}`)
      link.markUp()
      return
    }
    case 'refused': {
      const why = String(f.reason)
      const forGood: Record<string, string> = {
        unknown_project: `the hub does not know ${CONFIG.projectDir}. Run:  herdr-tg enroll ${CONFIG.projectDir}`,
        bad_token: `the secret for ${CONFIG.projectDir} is not one the hub knows. Re-run:  herdr-tg enroll ${CONFIG.projectDir}`,
        not_enabled: 'this project is enrolled but switched off',
        version_skew: 'this bridge and the hub do not speak the same version; upgrade one of them',
        bad_lane: `the hub will not address a conversation called ${ADDRESS ?? 'this one'}; if the hub is older than this bridge, restarting herdr-tg is the whole of the fix`,
      }
      const forNow: Record<string, string> = {
        already_claimed: 'another bridge already holds this conversation; waiting for it to go',
        frame_too_large: 'the hub refused a frame for being too large',
      }
      // One refusal blaming another bridge is an ordinary restart racing its predecessor. Several
      // in a row is a bridge that outlived its server, and this box has had one squat a claim.
      heldByAnother = why === 'already_claimed' ? heldByAnother + 1 : 0
      const stuck = heldByAnother >= 3
      const said = stuck
        ? 'another bridge has held this conversation across several attempts and is not letting go; if no opencode server is running, its bridge outlived it and needs to be ended'
        : (forGood[why] ?? forNow[why])
      // An unknown reason is treated as TEMPORARY on purpose: a hub shipped after this bridge may
      // refuse for something recoverable, and giving up on a guess is worse than waiting.
      link.markDown(stuck || why in forGood, said ?? `the hub refused this connection for a reason this bridge does not know (${why})`)
      note(said ?? `refused: ${why}`)
      return
    }
    case 'ack': {
      // The hub says what became of a frame. `unseen` is NOT success: it means the send went out and
      // could not be confirmed, and it is never retried, because Telegram has no idempotency key and
      // a second copy of a question would leave two live keyboards for it.
      const was = link.frameInFlight(String(f.ref))
      link.forgetInFlight(String(f.ref))
      if (f.delivered === 'yes') {
        if (f.why === 'clamped' && was) note(`${was.what} arrived on his phone clipped short`)
        return
      }
      const why = f.delivered === 'no' ? String(f.why ?? 'no reason given') : 'the hub could not confirm it arrived'
      note(`the hub did not deliver ${was?.what ?? 'a frame'} (${why})`)
      // Without this, a question the operator never saw stayed answerable here — the record aged
      // out a hundred questions later while the agent waited on a keyboard that never existed.
      if (was?.askId) giveUpOn(was.askId, why)
      return
    }
    case 'choice':
      void answer(String(f.ask_id), String(f.option_id))
      return
    case 'message':
      // The operator typed at this project. opencode's own prompt endpoint is the place for this,
      // and it is deliberately not wired yet: steering a session by text is a second decision, and
      // this slice is the question path.
      note('the operator typed something; passing typed steering to opencode is not built yet')
      return
    default:
      // Unknown kind: ignored, so a hub shipped after this bridge cannot kill the link by being
      // newer. `ping` never arrives here — `hub-link.ts` answers it itself, so liveness never waits
      // on this switch.
      return
  }
}

/** Consecutive refusals blaming another bridge. One is a restart; several is one that stayed. */
let heldByAnother = 0

// ───────────────────────────────────────────────────────────────────────────────────────────────
// opencode.

/**
 * Where an answer goes. Four endpoints, and the spec is the only authority on which is which:
 * the v2 pair live under `/api` and name the session in the path, the v1 pair do not.
 */
export function replyUrl(o: Pick<Open, 'kind' | 'v2' | 'sessionID' | 'requestID'>): string {
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
    // A lookup that comes up empty is a failure, not a silent continue. The question is gone —
    // the server restarted, or this record aged out — and there is nobody to tell.
    note(`a tap arrived for ${askId}, which this bridge has no record of; nothing was answered`)
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
    // said so in his own words — "answered from your phone — Always". A second retirement from
    // this side overwrote that note with "answered at the terminal", which is not where he
    // answered it. A retirement is for a question that stopped being asked somewhere the hub
    // cannot see; a tap is not that.
    note(`opencode took the answer to ${askId}`)
  } catch (e) {
    note(`could not reach opencode to answer ${askId}: ${(e as Error)?.message ?? e}`)
  }
}

/** One opencode event, mapped onto the hub's vocabulary. */
export function onOpencodeEvent(ev: Record<string, any>): void {
  const type = String(ev?.type ?? '')
  // opencode carries the same payload under two names. `/event` nests it in `properties`; the
  // durable per-session stream nests it in `data`, and the OpenAPI schemas describe both. Reading
  // only one of them is not a parse error — every field simply comes back undefined, and the first
  // real permission request reached the operator's phone fine and then answered nothing at all,
  // because the ask id it was recorded under was the string "pundefined".
  const data = ev?.properties ?? ev?.data ?? {}
  switch (type) {
    case 'question.v2.asked':
    case 'question.asked': {
      const questions: any[] = Array.isArray(data.questions) ? data.questions : []
      // opencode can publish several questions in one request. Only the first is drawn: the reply
      // shape answers them in order, and a phone that shows two keyboards for one request cannot
      // say which one an answer belonged to. The rest are named so the operator is not misled
      // into thinking he answered everything.
      const q = questions[0]
      if (!q || !Array.isArray(q.options) || q.options.length === 0) {
        note('a question arrived with no options; it needs a keyboard this bridge cannot draw')
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
      // one of three; he never names the action, and this bridge never invents a fourth.
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

if (import.meta.main) {
  link.start()
  void watch()
  // A clean goodbye, best effort and time-boxed. `process.exit()` on the same tick as the write
  // loses the `bye` to a short write, which is what this used to do: the frame was handed to the
  // kernel and the process was gone before the kernel had taken it. A relay in front of the hub
  // acts on that frame — it detaches this producer and starts its grace clock — so losing it makes
  // an ordinary restart look like a bridge that vanished.
  let leaving = false
  const goodbye = (): void => {
    if (leaving) return
    leaving = true
    try {
      if (link.isUp) link.sendControl({ t: 'bye', reason: 'stopping' })
    } catch {
      /* going away regardless */
    }
    setTimeout(() => process.exit(0), 200)
  }
  for (const sig of ['SIGTERM', 'SIGINT'] as const) process.on(sig, goodbye)
}
