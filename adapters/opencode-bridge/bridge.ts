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
 */

import { readFileSync, existsSync } from 'fs'
import { join } from 'path'

const PROTOCOL_VERSION = 1
const MAX_FRAME_BYTES = 64 * 1024
const MAX_PENDING = 64

/** The project this bridge speaks for. Its secret lives in the repo, not in this process's argv. */
const REPO = process.env.OPENCODE_BRIDGE_REPO ?? process.cwd()
const TOKEN_FILE = process.env.KICKOFF_HUB_TOKEN_FILE ?? join(REPO, '.kickoff', 'hub.token')

/**
 * `/run/user/<uid>/kickoff/hub.sock`, derived and never configured.
 *
 * Derived rather than read from `XDG_RUNTIME_DIR`, which does not survive an `env -i` boundary —
 * the two sides would then derive different paths with neither being wrong.
 */
const SOCKET =
  process.env.KICKOFF_HUB_SOCKET ?? `/run/user/${process.getuid?.() ?? 0}/kickoff/hub.sock`

/** The one opencode server this bridge watches. */
const OPENCODE = (process.env.OPENCODE_URL ?? 'http://127.0.0.1:9700').replace(/\/$/, '')

/** This run. A new instance invalidates every question the last one left open. */
const INSTANCE = `${process.pid}-${Date.now()}`

let seq = 0
const nextId = () => `f${++seq}`

function note(msg: string): void {
  process.stderr.write(`opencode-bridge: ${msg}\n`)
}

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

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The hub link.

type Framed = { id: string; bytes: Uint8Array; what: string }

let sock: import('bun').Socket | null = null
let up = false
let permanent: string | null = null
const pending: Framed[] = []
const control: Framed[] = []
let unsent: (Framed & { sent: number }) | null = null

/**
 * Write whole frames only.
 *
 * A short write is normal on a socket under load, and treating one as sent is how a queue reports
 * every frame delivered while the peer receives a stream of spliced halves. Nothing leaves the
 * queue until its last byte is accepted, and a frame half-written into a connection that then dies
 * is kept whole rather than resumed — a headless tail on the next connection reads as one
 * unparseable line and takes a healthy frame down with it.
 */
function flush(s: import('bun').Socket): boolean {
  const put = (b: Uint8Array): number => {
    try {
      return s.write(b)
    } catch {
      return 0
    }
  }
  for (;;) {
    if (unsent) {
      const rest = unsent.bytes.subarray(unsent.sent)
      const n = put(rest)
      if (n < rest.length) {
        unsent.sent += Math.max(n, 0)
        return false
      }
      unsent = null
    }
    // Nothing goes out before `welcome`. A connected socket proves only that something accepted;
    // the hub can still refuse this project and close.
    const next = control.shift() ?? (up ? pending.shift() : undefined)
    if (!next) return unsent === null && control.length === 0 && pending.length === 0
    const n = put(next.bytes)
    if (n < next.bytes.length) {
      unsent = { ...next, sent: Math.max(n, 0) }
      return false
    }
  }
}

/** THE ONLY WRITER. The newline is appended here and nowhere else. */
function send(payload: Record<string, unknown>, what: string): boolean {
  const id = nextId()
  const bytes = Buffer.from(JSON.stringify({ v: PROTOCOL_VERSION, id, ...payload }) + '\n', 'utf8')
  if (bytes.length > MAX_FRAME_BYTES) {
    // Refused rather than truncated. Half a message on a phone is worse than none and looks the
    // same as a whole one.
    note(`a frame was ${bytes.length} bytes and was NOT sent`)
    return false
  }
  if (permanent) {
    note(`not sent (${what}): ${permanent}`)
    return false
  }
  if (pending.length >= MAX_PENDING) {
    note(`the line of waiting frames is full; dropped ${what}`)
    return false
  }
  pending.push({ id, bytes, what })
  return up && sock ? flush(sock) : false
}

/** A frame nothing is waiting on — a pong, a hello, a goodbye. Same queue, so it cannot splice. */
function sendControl(s: import('bun').Socket, payload: Record<string, unknown>): void {
  const id = nextId()
  control.push({
    id,
    bytes: Buffer.from(JSON.stringify({ v: PROTOCOL_VERSION, id, ...payload }) + '\n', 'utf8'),
    what: 'a control frame',
  })
  flush(s)
}

function secret(): { token: string; repo: string } | null {
  if (!existsSync(TOKEN_FILE)) return null
  const t = readFileSync(TOKEN_FILE, 'utf8').trim()
  return t ? { token: t, repo: REPO } : null
}

let backoff = 1000
let saidItWasDown = false

function connect(): void {
  const here = secret()
  if (!here) {
    // Permanent: no amount of waiting writes a secret. Said once, with the command that fixes it.
    permanent = `no secret at ${TOKEN_FILE}. Run:  herdr-tg enroll ${REPO}`
    note(permanent)
    return
  }
  let buf = ''
  Bun.connect({
    unix: SOCKET,
    socket: {
      open(s) {
        sock = s
        backoff = 1000
        saidItWasDown = false
        sendControl(s, {
          t: 'hello',
          project_id: 'unknown-until-the-hub-says',
          token: here.token,
          instance: INSTANCE,
          repo: here.repo,
          pid: process.pid,
        })
      },
      data(s, chunk) {
        buf += chunk.toString()
        // Read exactly one line at a time, and never to EOF.
        for (;;) {
          const nl = buf.indexOf('\n')
          if (nl < 0) break
          const line = buf.slice(0, nl)
          buf = buf.slice(nl + 1)
          if (line.trim()) onHubFrame(s, line)
        }
        if (buf.length > MAX_FRAME_BYTES) {
          note('the hub sent a line past the ceiling; dropping the connection')
          buf = ''
          s.end()
        }
      },
      close() {
        sock = null
        up = false
        retry()
      },
      error(_s, e) {
        note(`socket error: ${(e as Error)?.message ?? e}`)
      },
    },
  }).catch(() => {
    sock = null
    up = false
    if (!saidItWasDown) {
      note(`the hub is not listening at ${SOCKET}; retrying`)
      saidItWasDown = true
    }
    retry()
  })
}

function retry(): void {
  if (permanent) return
  setTimeout(connect, backoff)
  backoff = Math.min(backoff * 2, 30_000)
}

function onHubFrame(s: import('bun').Socket, line: string): void {
  let f: Record<string, unknown>
  try {
    f = JSON.parse(line)
  } catch {
    // An unreadable frame is dropped, never fatal: a hub shipped after this bridge must not be
    // able to kill the link just by being newer.
    note('a frame from the hub could not be read; ignoring it')
    return
  }
  switch (f.t) {
    case 'welcome':
      up = true
      note(`connected as "${f.project}"`)
      flush(s)
      return
    case 'refused': {
      const why = String(f.reason)
      // Two of these mend themselves and the rest do not. Saying so is the difference between a
      // bridge that waits usefully and one that spins forever against a door that will not open.
      const mends = why === 'already_claimed'
      const said: Record<string, string> = {
        unknown_project: `the hub does not know ${REPO}. Run:  herdr-tg enroll ${REPO}`,
        bad_token: `the secret at ${TOKEN_FILE} is not one the hub knows. Re-run:  herdr-tg enroll ${REPO}`,
        already_claimed: 'another bridge already holds this project; waiting for it to go',
        version_skew: 'this bridge and the hub do not speak the same version; upgrade one of them',
        not_enabled: 'this project is enrolled but switched off',
        frame_too_large: 'the hub refused a frame for being too large',
      }
      const msg = Object.prototype.hasOwnProperty.call(said, why) ? said[why] : `the hub refused this connection (${why})`
      note(msg)
      if (!mends) permanent = msg
      return
    }
    case 'ping':
      sendControl(s, { t: 'pong', ref: f.id })
      return
    case 'ack': {
      // The hub says what became of a frame. `unseen` is not success: it means the send went out
      // and could not be checked, and treating it as delivered is how an agent comes to report
      // that the operator was reached when nobody knows whether he was.
      if (f.delivered === 'no') note(`the hub did not deliver a frame (${f.why ?? 'no reason given'})`)
      if (f.delivered === 'unseen') note('the hub could not confirm a frame arrived; it will not be sent again')
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
      return
  }
}

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
    // The buttons come off only once opencode has taken the answer. Stripping them first would
    // leave a question that looks settled on his phone and is still open in the session.
    send({ t: 'ask_resolved', ask_id: askId, how: 'answered', outcome: label }, `retiring ${askId}`)
  } catch (e) {
    note(`could not reach opencode to answer ${askId}: ${(e as Error)?.message ?? e}`)
  }
}

/** One opencode event, mapped onto the hub's vocabulary. */
export function onOpencodeEvent(ev: Record<string, any>): void {
  const type = String(ev?.type ?? '')
  const data = ev?.data ?? {}
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
      send({ t: 'ask', ask_id: askId, text: `${q.question}${more}${trimmed}`, options }, `a question (${askId})`)
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
      send(
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
      send(
        { t: 'ask_resolved', ask_id: askId, how: type.endsWith('rejected') ? 'withdrawn' : 'answered' },
        `retiring ${askId}`,
      )
      return
    }
    case 'session.idle':
      send({ t: 'beat', state: 'idle' }, 'a heartbeat')
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
  connect()
  void watch()
  for (const sig of ['SIGTERM', 'SIGINT'] as const) {
    process.on(sig, () => {
      if (sock) sendControl(sock, { t: 'bye', reason: 'stopping' })
      process.exit(0)
    })
  }
}
