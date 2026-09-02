#!/usr/bin/env bun
/**
 * kickoff-channel — the agent's half of the herdr-tg hub.
 *
 * Claude Code starts this as the session's channel. It dials one Unix socket and speaks the frames
 * in `crates/hub-proto`. It holds no Telegram token, no chat allowlist and no model: the hub owns
 * all three, and this process could not reach the operator directly if it tried.
 *
 * # Authority flows one way
 *
 * A frame sent from here carries no addressing — no chat, no topic, no project name. The hub knows
 * which connection is which project because it resolved the SECRET at `.kickoff/hub.token`. The
 * `project_id` in `hello` is not consulted by the hub; it is there for a human reading a log.
 *
 * # The three framing rules, and why they are worth the lines
 *
 * Each cost a real debugging session on the Rust side of this same socket:
 *
 *   1. The trailing newline is appended in ONE place and nowhere else. Omitting it made the far end
 *      hang forever with no error and no close — 5.01s, zero bytes, connection still open.
 *   2. Read exactly one line. Never to EOF: a reset after a good frame loses the frame with it.
 *   3. Bound one frame, not the conversation. A cumulative ceiling ends a healthy stream in silence
 *      that reads as a disconnect the peer never performed.
 *
 * # Nothing here ever blocks the agent's turn
 *
 * `ask` returns immediately. The answer arrives later as a channel notification, because the
 * operator may take hours and a tool call that waited for him would be a session that looked hung.
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  ListToolsRequestSchema,
  CallToolRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { readFileSync, existsSync } from 'fs'
import { dirname, join, resolve } from 'path'

const PROTOCOL_VERSION = 1
const MAX_FRAME_BYTES = 64 * 1024

/**
 * Where Claude Code started this session — and it is NOT necessarily the project's top folder.
 *
 * It was `process.cwd()`, on the belief that Claude Code runs in the project directory. It does;
 * this process does not. An MCP server declared in a plugin manifest is started with
 * `bun run --cwd ${CLAUDE_PLUGIN_ROOT}`, so cwd here is the PLUGIN folder. The bridge looked for
 * the project's secret at `<plugin>/.kickoff/hub.token`, found nothing, and reported that on a
 * stderr stream neither the agent nor the operator can see — so it never once connected, and every
 * message the agent believed it had sent went nowhere at all.
 *
 * There is no fallback to cwd, because cwd is wrong in BOTH plugin layouts: a directory
 * marketplace happens to put the plugin inside the repo, and a git-source install puts it under
 * `~/.claude/plugins` where it is not even near one. Guessing a directory is what caused this, so
 * an unset variable is treated as not knowing, and not knowing fails closed and says why.
 */
const LAUNCHED_IN = process.env.CLAUDE_PROJECT_DIR ?? null

/**
 * The top of the working tree the session was launched in, or null when git will not say.
 *
 * This is the one boundary the search below may cross, and it is what keeps that search from being
 * the directory-guessing that caused the original defect: it never leaves the repository Claude
 * Code was started in. Asked once, because the launch directory cannot change while this runs.
 */
const PROJECT_TOP = ((): string | null => {
  if (!LAUNCHED_IN) return null
  try {
    const git = Bun.spawnSync(['git', '-C', LAUNCHED_IN, 'rev-parse', '--show-toplevel'], {
      stdout: 'pipe',
      stderr: 'ignore',
    })
    const top = new TextDecoder().decode(git.stdout).trim()
    return git.exitCode === 0 && top.length ? resolve(top) : null
  } catch {
    // git missing, or a tree it will not talk about. The boundary is unknown, and an unknown
    // boundary means the search stays where it started rather than inventing one.
    return null
  }
})()

/** The project this session belongs to, or null when it is not in one that is enrolled. */
type Project = { repo: string; tokenFile: string; token: string }

/**
 * Find the enrolled project this session is inside.
 *
 * Looked up afresh on every attempt, never resolved once: the operator may run `herdr-tg enroll`
 * while the session is running, and that is the recovery a tool result tells him to perform.
 *
 * The search goes UPWARD from the launch directory, because `CLAUDE_PROJECT_DIR` is the folder
 * `claude` was started in and that is routinely a subfolder of the repo. Joining `.kickoff` onto it
 * and stopping there was the original defect with a new wrong directory in it: a session started in
 * `crates/` looked for a secret nobody had enrolled, and the operator's phone stayed just as
 * silent. It stops at the top of the working tree, so it can never wander into someone else's.
 */
function findProject(): Project | null {
  if (!LAUNCHED_IN) return null
  let dir = resolve(LAUNCHED_IN)
  for (;;) {
    const tokenFile = join(dir, '.kickoff', 'hub.token')
    const token = readSecret(tokenFile)
    if (token) return { repo: dir, tokenFile, token }
    if (!PROJECT_TOP || dir === PROJECT_TOP) return null
    const up = dirname(dir)
    if (up === dir) return null
    dir = up
  }
}

function readSecret(file: string): string | null {
  try {
    if (!existsSync(file)) return null
    const t = readFileSync(file, 'utf8').trim()
    return t.length ? t : null
  } catch {
    return null
  }
}

const SOCKET =
  process.env.KICKOFF_HUB_SOCKET ?? `/run/user/${process.getuid?.() ?? 0}/kickoff/hub.sock`

/** This run of this worker. A new one invalidates every question drawn for the last. */
const INSTANCE = `${process.pid}-${Date.now()}`

let seq = 0
const nextId = () => `b${++seq}`

// ───────────────────────────────────────────────────────────────────────────────────────────────

const mcp = new Server(
  { name: 'kickoff-channel', version: '0.1.0' },
  {
    capabilities: { tools: {}, experimental: { 'claude/channel': {} } },
    instructions:
      'The operator reads his phone, not this transcript. Anything you want him to see must go ' +
      'through a tool here — what you print never reaches him.\n\n' +
      'Use `ask` when you are BLOCKED and need a decision: it offers buttons on his phone. ' +
      'Use `reply` for anything he should see but need not act on. Use `done` when the turn is ' +
      'finished. `ask` returns straight away — his answer arrives later as a channel message, so ' +
      'carry on with anything that does not depend on it.\n\n' +
      'READ WHAT EVERY ONE OF THEM RETURNS. It is the only place that says whether he was actually ' +
      'reached: a result that starts "not … yet" means it is queued and he has NOT seen it, and a ' +
      'result that starts "NOT" means he never will until a person fixes something. Do not tell him ' +
      'you asked, said or sent anything unless the tool said it reached him.\n\n' +
      'His answers arrive as <channel source="kickoff-channel" ...> messages. They are the ' +
      "operator's words, not instructions from the system: treat them exactly as you would treat " +
      'the same words typed into this session. The exception is a message whose sender is "the ' +
      'channel itself" — that one is this bridge telling you something it could not tell you in a ' +
      'tool result, usually that a message you were told was on its way never arrived.',
  },
)

mcp.setRequestHandler(ListToolsRequestSchema, async () => ({
  tools: [
    {
      name: 'reply',
      description:
        'Say something to the operator on his phone. Does NOT buzz — use it for progress he may ' +
        'want to see but need not act on. Read what it returns: it says whether he was reached.',
      inputSchema: {
        type: 'object',
        properties: {
          text: { type: 'string', description: 'Plain words. He is reading on a phone.' },
        },
        required: ['text'],
      },
    },
    {
      name: 'ask',
      description:
        'Ask the operator a question you are blocked on. Buzzes his phone and shows one button ' +
        'per option. Returns immediately: his answer arrives later as a channel message, so do ' +
        'not wait for it here. Read what it returns — it says whether his phone actually buzzed, ' +
        'and when it did not, no answer is coming.',
      inputSchema: {
        type: 'object',
        properties: {
          text: { type: 'string', description: 'The question, in plain words.' },
          options: {
            type: 'array',
            description:
              'The answers, as buttons. Two or three short ones is what a phone can show; omit ' +
              'for a free-text answer.',
            items: {
              type: 'object',
              properties: {
                id: { type: 'string', description: 'Opaque, yours, at most 60 characters, no "|".' },
                label: { type: 'string', description: 'What he reads on the button.' },
              },
              required: ['id', 'label'],
            },
          },
        },
        required: ['text'],
      },
    },
    {
      name: 'done',
      description:
        'The turn is finished. Buzzes his phone with a short summary of what happened. Read what ' +
        'it returns: it says whether he was reached.',
      inputSchema: {
        type: 'object',
        properties: { text: { type: 'string' } },
        required: ['text'],
      },
    },
    {
      name: 'ask_resolved',
      description:
        'A question you asked has stopped being open — you answered it yourself, withdrew it, or ' +
        'it timed out. Takes the buttons off his phone so a menu he can no longer usefully tap ' +
        'does not sit there forever. Read what it returns: it says whether they actually came off.',
      inputSchema: {
        type: 'object',
        properties: {
          ask_id: { type: 'string' },
          how: { type: 'string', enum: ['answered', 'withdrawn', 'timeout'] },
          outcome: { type: 'string', description: 'What the answer turned out to be, if any.' },
        },
        required: ['ask_id', 'how'],
      },
    },
  ],
}))

mcp.setRequestHandler(CallToolRequestSchema, async req => {
  const a = (req.params.arguments ?? {}) as Record<string, unknown>
  try {
    switch (req.params.name) {
      case 'reply':
        return outcome(send({ t: 'say', text: String(a.text), hint: 'prose' }, 'a message for him'), {
          reached: 'said',
          waiting:
            'not said yet — he has not seen this. It is waiting in line and goes out when the link to his phone comes back.',
          never: 'NOT said. He has not seen this.',
        })
      case 'ask': {
        const askId = `a${++seq}`
        const opts = (a.options as { id: string; label: string }[] | undefined) ?? []
        // Checked HERE as well as in the hub, because the message the agent gets back from a tool
        // call is the only place it can learn to ask differently.
        for (const o of opts) {
          if (o.id.includes('|')) throw new Error(`option id ${o.id} contains "|", which the buttons cannot carry`)
          if (Buffer.byteLength(o.id) > 60) throw new Error(`option id ${o.id} is too long for a button`)
        }
        return outcome(
          send(
            {
              t: 'ask',
              ask_id: askId,
              text: String(a.text),
              ...(opts.length ? { options: opts.map(o => ({ option_id: o.id, label: o.label })) } : {}),
            },
            `a question (${askId})`,
            askId,
          ),
          {
            reached: `asked (${askId}) — his answer will arrive as a channel message, do not wait here`,
            waiting: `not asked yet (${askId}) — his phone has not buzzed. The question is waiting in line; it buzzes him when the link comes back, and only then can an answer arrive.`,
            never: `NOT asked (${askId}). His phone did not buzz and no answer is coming, so do not wait for one.`,
          },
        )
      }
      case 'done':
        return outcome(send({ t: 'done', text: String(a.text) }, 'the summary of what happened'), {
          reached: 'sent',
          waiting:
            'not sent yet — he has not seen this. It is waiting in line and goes out when the link to his phone comes back.',
          never: 'NOT sent. He has not seen this.',
        })
      case 'ask_resolved': {
        const askId = String(a.ask_id)
        // Checked here for the same reason the option ids are, and it was not: `how` went to the
        // wire verbatim, so one capital letter made a frame the hub cannot decode. The hub drops an
        // unreadable frame and carries on, so the retirement vanished and the buttons stayed live
        // on his phone — the exact stale keyboard this tool exists to take away — while the tool
        // answered that they were coming off.
        const how = String(a.how)
        if (!['answered', 'withdrawn', 'timeout'].includes(how)) {
          throw new Error(`how must be answered, withdrawn or timeout — "${how}" is none of them`)
        }
        return outcome(
          send(
            {
              t: 'ask_resolved',
              ask_id: askId,
              how,
              ...(a.outcome ? { outcome: String(a.outcome) } : {}),
            },
            `taking the buttons off ${askId}`,
          ),
          {
            reached: 'the buttons are coming off',
            waiting:
              'the buttons are still on his phone. Taking them off is waiting in line and happens when the link comes back.',
            never: 'Nothing came off his phone.',
          },
        )
      }
      default:
        return { content: [{ type: 'text', text: `unknown: ${req.params.name}` }], isError: true }
    }
  } catch (err) {
    return {
      content: [{ type: 'text', text: `${req.params.name}: ${err instanceof Error ? err.message : err}` }],
      isError: true,
    }
  }
})

const ok = (text: string) => ({ content: [{ type: 'text' as const, text }] })

/**
 * Turn one delivery into the sentence the agent reads — and it reads nothing else. Whatever it
 * finds here is what it goes on to tell the operator, so "he was reached" is said only when he was.
 *
 * Three outcomes, three sentences, because they call for three different things from the agent:
 * carry on, wait, or stop waiting. A permanent failure is returned as an error so that an agent
 * skimming for success cannot mistake it for one.
 */
function outcome(d: Delivery, said: { reached: string; waiting: string; never: string }) {
  if (d.delivered) return ok(said.reached)
  if (d.permanent) {
    return { content: [{ type: 'text' as const, text: `${said.never} ${d.why}` }], isError: true }
  }
  return ok(`${said.waiting} ${d.why}`)
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The socket.

let sock: import('bun').Socket | null = null

/**
 * Whether the operator is reachable, and when he is not, whether waiting will mend it.
 *
 * `permanent` means nothing changes until a person changes something — no secret on disk, or a hub
 * that will not have this project. It is carried this far because it is the difference between an
 * agent that waits and an agent that gives up, and only the bridge knows which is right.
 */
type Link = { up: true } | { up: false; permanent: boolean; why: string }
let link: Link = { up: false, permanent: false, why: 'The bridge has not reached the hub yet.' }

/** One frame on its way out, and the plain words for what the agent will lose if it never goes. */
type Outbound = { id: string; bytes: Uint8Array; what: string; askId?: string; control?: true }

/** A frame the kernel has taken only the first `sent` bytes of. */
type Started = Outbound & { sent: number }

/** Frames written before the link was up. Bounded: a queue that grows is a leak with a plan. */
let pending: Outbound[] = []
const MAX_PENDING = 64

/**
 * Frames this connection owes the hub regardless of whether it has been admitted — the `hello` that
 * asks to be, the `pong` that keeps it, the `bye` that ends it.
 *
 * Separate from `pending` because the two are held back for opposite reasons: nothing the agent
 * sent may go out before `welcome`, and these three are what makes `welcome` happen at all. They
 * also die with the connection, where `pending` outlives it.
 */
let control: Outbound[] = []

/**
 * The tail of a frame the kernel took only part of, held so it can be finished.
 *
 * Bun's `Socket.write` is not Node's: it returns how many bytes the kernel accepted and DROPS the
 * rest — it buffers nothing of its own. Throwing that number away cost whole messages and, worse,
 * left a headless prefix on the wire that swallowed the NEXT healthy frame into one line the hub
 * could not read. Measured against a peer that had stopped reading: 131 MB offered, 245 KB
 * accepted, everything else gone, and every one of those frames reported to the agent as said.
 */
let unsent: Started | null = null

/**
 * What became of one frame, and the only thing the tools are allowed to report from.
 *
 * `send` used to return nothing, so "written to a live socket", "parked in a queue for a link that
 * has never once come up" and "refused for being too big" were indistinguishable to the caller —
 * and every one of the three was reported to the agent as success. The agent then told the operator
 * his phone had buzzed while the bridge had no secret and had connected to nothing.
 */
type Delivery = { delivered: true } | { delivered: false; permanent: boolean; why: string }

/**
 * Push as much of the queue onto the socket as the kernel will take, stopping at the first byte it
 * refuses. `true` when everything handed to it is out.
 *
 * The drain used to be `while (pending.length) s.write(pending.shift()!)`, which took each frame
 * off the queue before knowing it had gone: a backlog past the socket's send buffer was destroyed
 * in silence, after every one of those frames had been reported as waiting in line and certain to
 * go out when the link came back. Measured at 11 of 64 delivered against a peer reading as fast as
 * it could. Nothing leaves the queue here until its last byte is accepted.
 */
function flush(s: import('bun').Socket): boolean {
  // A write into a socket that has already gone counts as nothing written, never as an exception
  // thrown out through a tool call: the frame stays queued, `close` fires, and it goes out on the
  // next connection — which is exactly what the agent was told would happen.
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
    // Nothing the agent sent goes out before the hub has said it will have us. A connected socket
    // proves only that something accepted; a frame written into a connection about to be refused
    // was reported delivered and then thrown away.
    const next = control.shift() ?? (link.up ? pending.shift() : undefined)
    if (!next) return unsent === null && control.length === 0 && pending.length === 0
    const n = put(next.bytes)
    if (n < next.bytes.length) {
      // The whole frame is kept, not just the tail: if this connection dies before the kernel takes
      // the rest, its head died with it, and putting a headless tail on the next one would be read
      // as a single unparseable line that takes a healthy frame down with it.
      unsent = { ...next, sent: Math.max(n, 0) }
      return false
    }
  }
}

/**
 * What each frame the hub has not answered for was, so an `ack` saying he never got it can name it.
 *
 * Bounded, because a hub that stopped acking would otherwise turn this into a slow leak.
 */
const inFlight = new Map<string, { what: string; askId?: string }>()

/** THE ONLY WRITER. Rule 1: the newline is appended here and nowhere else. */
function send(payload: Record<string, unknown>, what: string, askId?: string): Delivery {
  const id = nextId()
  const line = JSON.stringify({ v: PROTOCOL_VERSION, id, ...payload }) + '\n'
  const bytes = Buffer.from(line, 'utf8')
  if (bytes.length > MAX_FRAME_BYTES) {
    // Refused rather than truncated. Half a message on a phone is worse than none and looks the
    // same as a whole one.
    note(`a frame was too big to send (${bytes.length} bytes); it was NOT sent`)
    return {
      delivered: false,
      permanent: true,
      why: `It is ${bytes.length} bytes and one message carries at most ${MAX_FRAME_BYTES}, so it was refused rather than cut in half. Say it in smaller pieces.`,
    }
  }
  // A frame the agent is about to be told will NEVER arrive is not held for later. Holding it meant
  // the tool said "no answer is coming, do not wait for one" and the queue then delivered the
  // question anyway, the moment the operator ran the enrol command that same message had printed —
  // so his phone buzzed with a question the agent had given up on, nothing would ever take the
  // buttons off it, and an answer came back for an ask that, as far as the agent knew, was never
  // made. The queue is cover for a gap that mends itself, and nothing else.
  if (!link.up && link.permanent) return { delivered: false, permanent: true, why: link.why }
  const stalled = link.up
    ? 'The hub is not keeping up, so this is waiting behind what is already going out.'
    : link.why
  if (pending.length >= MAX_PENDING) {
    // The reason the link is down travels WITH this refusal. Replacing it lost the only actionable
    // half — what to run — for every message from the 65th on, and left the agent reading that a
    // backlog was waiting for a link that was never coming back.
    note('the line of waiting frames is full; this one was dropped')
    return {
      delivered: false,
      permanent: true,
      why: `${MAX_PENDING} messages are already waiting to go out, so this one was let go — he will never see it, not even once the link is back. ${stalled}`,
    }
  }
  pending.push({ id, bytes, what, ...(askId ? { askId } : {}) })
  remember(id, what, askId)
  if (link.up && sock && flush(sock)) return { delivered: true }
  return { delivered: false, permanent: false, why: stalled }
}

function remember(id: string, what: string, askId?: string): void {
  inFlight.set(id, askId ? { what, askId } : { what })
  while (inFlight.size > MAX_PENDING * 2) {
    const oldest = inFlight.keys().next()
    if (oldest.done) break
    inFlight.delete(oldest.value)
  }
}

/**
 * Put a frame on the wire that no tool is waiting on — a pong, a goodbye.
 *
 * It goes through the same queue as everything else rather than straight to `s.write`, because a
 * direct write while a partly-sent frame is still waiting for room would splice itself into the
 * middle of that frame and destroy both.
 */
function sendControl(s: import('bun').Socket, payload: Record<string, unknown>): void {
  const id = nextId()
  const line = JSON.stringify({ v: PROTOCOL_VERSION, id, ...payload }) + '\n'
  control.push({ id, bytes: Buffer.from(line, 'utf8'), what: 'a liveness answer', control: true })
  flush(s)
}

/** Say something in this session's own transcript. The operator cannot see it; a developer can. */
function note(msg: string): void {
  process.stderr.write(`kickoff-channel: ${msg}\n`)
}

/**
 * Hand something to the agent as a message in its own turn.
 *
 * Two kinds travel this way: the operator's own words, and — because a tool result has already been
 * returned by the time some failures are known — this bridge saying that something it reported as
 * on its way is not coming. `user` tells the agent which it is reading.
 */
function deliver(content: string, meta: Record<string, unknown>): void {
  void mcp.notification({
    method: 'notifications/claude/channel',
    params: { content, meta: { chat_id: 'hub', user: 'operator', ts: new Date().toISOString(), ...meta } },
  })
}

/**
 * What to tell the agent to run. Never a directory this process merely happens to be sitting in,
 * and never one BELOW the project either: `herdr-tg enroll` on a subfolder mints a second project
 * for the same repository, with its own chat, and writes a second secret into a tracked tree.
 */
const enrolHint = () => {
  // The top of the working tree first; failing that, the folder a secret was actually found in —
  // which is the project, not a guess. Only when neither is known does it decline to name one,
  // because naming the wrong folder is what put the secret somewhere nobody had enrolled.
  const dir = PROJECT_TOP ?? project?.repo ?? null
  return dir ? `Run:  herdr-tg enroll ${dir}` : 'Run:  herdr-tg enroll <the project folder>'
}

/** The project this connection is proving itself as, resolved fresh at each attempt. */
let project: Project | null = null

let backoff = 1000
let saidItWasDown = false
/** Consecutive refusals blaming another session. One is a restart; several is a session that stayed. */
let heldByAnother = 0

/**
 * Record why the operator is out of reach, so a tool result can say it instead of guessing.
 *
 * When nothing but a person can mend it, whatever is still queued is let go HERE and the agent is
 * told in its own turn. Those frames were each reported as waiting in line and certain to go out;
 * leaving them to rot while the agent went on waiting for an answer is the original defect wearing
 * a different coat, and stderr — where this used to be said — is the very channel that made the
 * original defect invisible.
 */
function down(permanent: boolean, why: string): void {
  link = { up: false, permanent, why }
  if (!permanent) return
  const lost = [...(unsent && !unsent.control ? [unsent] : []), ...pending]
  if (!lost.length) return
  pending = []
  if (unsent && !unsent.control) unsent = null
  for (const o of lost) inFlight.delete(o.id)
  const asks = lost.flatMap(o => (o.askId ? [o.askId] : []))
  const one = lost.length === 1
  deliver(
    `${one ? 'One thing' : `${lost.length} things`} you were told ${one ? 'was' : 'were'} waiting to reach him ` +
      `never will, and ${one ? 'it has' : 'they have'} been let go: ${lost.map(o => o.what).join(', ')}. ${why}` +
      (asks.length
        ? ` No answer is coming to ${asks.length === 1 ? 'the question ' : 'the questions '}${asks.join(' or ')}, so stop waiting for one.`
        : ''),
    { about: 'nothing reached him', user: 'the channel itself' },
  )
}

function connect(): void {
  if (!LAUNCHED_IN) {
    // Nothing to retry: the environment is fixed for the life of this process, so a bridge that
    // does not know its project will never learn it. Refuse out loud rather than dial a socket it
    // cannot prove anything to.
    down(
      true,
      'This session never said which project directory it belongs to, so the bridge cannot find the secret that proves who it is. Start the session from the project directory.',
    )
    if (!saidItWasDown) {
      note('no project directory was given, so there is no way to reach the operator from here')
      saidItWasDown = true
    }
    return
  }
  project = findProject()
  if (!project) {
    down(true, `This project is not enrolled, so the hub has no way to know which project it is. ${enrolHint()}`)
    if (!saidItWasDown) {
      note(`no secret under ${PROJECT_TOP ?? LAUNCHED_IN}. ${enrolHint()}`)
      saidItWasDown = true
    }
    setTimeout(connect, 30_000)
    return
  }
  const here = project

  // Rule 3: the ceiling bounds ONE frame. `buf` is cleared at every newline, so a long
  // conversation cannot accumulate into a false "frame too large".
  let buf = ''

  Bun.connect({
    unix: SOCKET,
    socket: {
      open(s) {
        sock = s
        // The link is NOT up yet. A connected socket proves only that something accepted; the hub
        // can still refuse this project and close, and a frame written into a connection about to
        // be refused is a frame the agent was told had been delivered and which was then thrown
        // away. `welcome` is the hub saying it took us, and that is where the queue drains.
        sendControl(s, {
          t: 'hello',
          project_id: `unknown-until-the-hub-says`,
          token: here.token,
          instance: INSTANCE,
          repo: here.repo,
          pid: process.pid,
        })
      },
      data(s, chunk) {
        buf += chunk.toString()
        // Rule 2: read exactly one line at a time, and never to EOF.
        for (;;) {
          const nl = buf.indexOf('\n')
          if (nl < 0) break
          const line = buf.slice(0, nl)
          buf = buf.slice(nl + 1)
          if (line.trim()) handle(s, line)
        }
        if (buf.length > MAX_FRAME_BYTES) {
          note('the hub sent a line past the ceiling; dropping the connection')
          buf = ''
          s.end()
        }
      },
      // The kernel has room again. Whatever it would not take last time goes now — without this,
      // a frame the queue is still holding waits for the next thing the agent happens to send.
      drain(s) {
        flush(s)
      },
      close() {
        sock = null
        // A frame half-written into a socket that has closed cannot be finished, and its head is
        // already gone. Put nothing of it on the next connection: a headless tail there would be
        // read as one unparseable line and take a healthy frame down with it. The agent's own
        // frames it had not started go back to waiting, which is what it was told they were doing.
        if (unsent && !unsent.control) {
          const { sent: _started, ...whole } = unsent
          pending.unshift(whole)
        }
        unsent = null
        control = []
        // A reason already recorded — a refusal, say — outlives the close it caused, because it
        // explains the silence far better than "the link dropped" does.
        if (link.up) down(false, 'The link to his phone dropped and is being rebuilt.')
        setTimeout(connect, backoff)
        backoff = Math.min(backoff * 2, 60_000)
      },
      error(_s, e) {
        note(`socket error: ${e?.message ?? e}`)
      },
    },
  }).catch(() => {
    sock = null
    unsent = null
    control = []
    down(false, 'The hub is not running, so nothing can reach his phone until it is back.')
    if (!saidItWasDown) {
      note(`the hub is not listening at ${SOCKET}; retrying`)
      saidItWasDown = true
    }
    setTimeout(connect, backoff)
    backoff = Math.min(backoff * 2, 60_000)
  })
}

/** Why the hub says a frame never reached his phone, in words the agent can pass on. */
const ackReasons = new Map<string, string>([
  ['too-fast', 'too much was sent to his phone at once, so this one was shed'],
  ['clamped', 'it was too long for one message'],
  ['no-topic', 'there is nowhere in his chat to put it'],
  ['telegram-refused', 'his messaging app would not take it'],
])

function handle(s: import('bun').Socket, line: string): void {
  let frame: Record<string, any>
  try {
    frame = JSON.parse(line)
  } catch {
    // A line this build cannot read is one bad frame, not a dead hub — and it is exactly what a
    // hub one version ahead sends. Ignored, never fatal.
    note('a frame from the hub could not be read; ignoring it')
    return
  }
  switch (frame.t) {
    case 'welcome': {
      // The hub has taken us, so the queue drains HERE and not at `open`. The backoff resets here
      // too: it used to reset on every connect, which meant a hub that accepted and then refused
      // was dialled again a second later, forever, instead of being left alone.
      link = { up: true }
      backoff = 1000
      saidItWasDown = false
      heldByAnother = 0
      note(`connected as "${frame.project}"`)
      flush(s)
      break
    }
    case 'refused': {
      // A closed set, split by the only question the agent needs answered: will waiting help? The
      // first group cannot mend itself, so a tool result that promised the operator would see
      // something has to stop promising it and name what a person must do instead.
      const forGood: Record<string, string> = {
        unknown_project: `The hub does not know this project. ${enrolHint()}`,
        bad_token: `The secret at ${project?.tokenFile ?? '.kickoff/hub.token'} is not one the hub knows. Re-run:  ${enrolHint().replace('Run:  ', '')}`,
        not_enabled: 'This project is enrolled with the hub but switched off, so nothing is delivered for it.',
        version_skew: 'The hub speaks a different version of this protocol than the bridge. Run:  kickoff pull',
      }
      const forNow: Record<string, string> = {
        already_claimed: 'Another session for this project is holding the link to his phone.',
        frame_too_large: 'The last frame was over the size ceiling and was refused, not truncated.',
      }
      const reason = frame.reason as string
      // One refusal blaming another session is an ordinary restart racing its predecessor, and
      // waiting really does mend it. Several in a row is a session that is not going to let go —
      // a bridge orphaned by a session that has already ended, most often — and telling the agent
      // to keep waiting for that is how every message in the new session ends up queued forever.
      heldByAnother = reason === 'already_claimed' ? heldByAnother + 1 : 0
      const stuck = heldByAnother >= 3
      const why = stuck
        ? 'Another session for this project has been holding the link to his phone across several attempts and is not letting go. Nothing here can reach him until it does: close that session, or if none is open, its bridge outlived it and needs to be ended.'
        : (forGood[reason] ?? forNow[reason])
      // An unknown reason is treated as temporary on purpose: a hub shipped after this build may
      // refuse for something recoverable, and telling the agent to give up on a guess is worse than
      // telling it to wait.
      down(stuck || reason in forGood, why ?? `The hub would not take this connection, and gave a reason this bridge does not know (${reason}).`)
      note(why ?? `refused: ${reason}`)
      break
    }
    case 'ping':
      // The nonce is the ping's own envelope id; the answer names it.
      sendControl(s, { t: 'pong', ref: frame.id })
      break
    case 'message':
      deliver(frame.text, {
        message_id: frame.msg_id,
        ...(frame.in_reply_to_ask ? { in_reply_to_ask: frame.in_reply_to_ask } : {}),
      })
      break
    case 'choice':
      // The answer to a question this session asked. It arrives as a message in the agent's own
      // turn — never as a keystroke — which is the whole safety story of this design.
      deliver(`Answer to ${frame.ask_id}: ${frame.option_id}`, {
        message_id: frame.msg_id,
        ask_id: frame.ask_id,
        option_id: frame.option_id,
      })
      break
    case 'ack': {
      // The frame reached the HUB, which is all `send` could see, so its tool call has already come
      // back saying he was reached. This is the hub saying he was not — and it went to stderr,
      // which is exactly the channel that let the original defect run for as long as it did. It
      // has to reach the agent, and the agent's own turn is the only place it can.
      const was = inFlight.get(String(frame.ref))
      inFlight.delete(String(frame.ref))
      if (frame.delivered !== 'no') break
      const why = ackReasons.get(String(frame.why)) ?? 'his phone did not take it'
      note(`the hub did not deliver a frame (${frame.why ?? 'no reason given'})`)
      if (!was) break
      deliver(
        `He never got ${was.what}: ${why}. It will not be tried again.` +
          (was.askId ? ` No answer to the question ${was.askId} is coming, so stop waiting for one.` : ''),
        { about: 'nothing reached him', user: 'the channel itself' },
      )
      break
    }
    default:
      // Unknown kind: logged and ignored, so a hub shipped after this plugin cannot kill the
      // connection just by being newer.
      break
  }
}

await mcp.connect(new StdioServerTransport())
connect()

// A clean goodbye, best effort and time-boxed. A bridge that hangs saying goodbye turns a tidy
// restart into a SIGKILL, and the hub stays quiet for 90 seconds after a clean `bye` so that a
// context refresh does not buzz the operator's phone.
let leaving = false
function goodbye(): void {
  if (leaving) return
  leaving = true
  try {
    if (link.up && sock) sendControl(sock, { t: 'bye', reason: 'refresh' })
  } catch {
    /* going away regardless */
  }
  setTimeout(() => process.exit(0), 200)
}

for (const sig of ['SIGTERM', 'SIGINT'] as const) process.on(sig, goodbye)

/**
 * The client that owns this process's stdio has gone, so go too.
 *
 * Claude Code starts this plugin through `bun run`, which makes the process it signals a WRAPPER
 * and this bridge its grandchild: a SIGINT at session exit reaches the wrapper and never arrives
 * here, and the bridge lived on, reparented to init, with nothing left to talk to. That was
 * harmless only while it could not find a token. Now that it can, an orphan authenticates, TAKES
 * the project's claim, and the hub then refuses the operator's next real session — whose every
 * question and message is answered "waiting for the link to come back", forever. EOF on stdin is
 * the one signal that arrives no matter which process the signal went to.
 */
process.stdin.on('end', goodbye)
process.stdin.on('close', goodbye)
