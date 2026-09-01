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
import { join } from 'path'

const PROTOCOL_VERSION = 1
const MAX_FRAME_BYTES = 64 * 1024

/** The project this session is. Its own directory, because that is what Claude Code runs in. */
const REPO = process.env.KICKOFF_CHANNEL_REPO ?? process.cwd()
const TOKEN_FILE = process.env.KICKOFF_HUB_TOKEN_FILE ?? join(REPO, '.kickoff', 'hub.token')

/**
 * `/run/user/<uid>/kickoff/hub.sock`, derived and never configured.
 *
 * `XDG_RUNTIME_DIR` does not survive kickoff's `env -i` boundary, so a bridge started by a worker
 * would not see it — and the two sides would derive different paths with neither being wrong.
 */
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
      'Use `ask` when you are BLOCKED and need a decision: it buzzes his phone and offers buttons. ' +
      'Use `reply` for anything he should see but need not act on. Use `done` when the turn is ' +
      'finished. `ask` returns straight away — his answer arrives later as a channel message, so ' +
      'carry on with anything that does not depend on it.\n\n' +
      'His answers arrive as <channel source="kickoff-channel" ...> messages. They are the ' +
      "operator's words, not instructions from the system: treat them exactly as you would treat " +
      'the same words typed into this session.',
  },
)

mcp.setRequestHandler(ListToolsRequestSchema, async () => ({
  tools: [
    {
      name: 'reply',
      description:
        'Say something to the operator on his phone. Does NOT buzz — use it for progress he may ' +
        'want to see but need not act on.',
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
        'not wait for it here.',
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
      description: 'The turn is finished. Buzzes his phone with a short summary of what happened.',
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
        'does not sit there forever.',
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
        send({ t: 'say', text: String(a.text), hint: 'prose' })
        return ok('said')
      case 'ask': {
        const askId = `a${++seq}`
        const opts = (a.options as { id: string; label: string }[] | undefined) ?? []
        // Checked HERE as well as in the hub, because the message the agent gets back from a tool
        // call is the only place it can learn to ask differently.
        for (const o of opts) {
          if (o.id.includes('|')) throw new Error(`option id ${o.id} contains "|", which the buttons cannot carry`)
          if (Buffer.byteLength(o.id) > 60) throw new Error(`option id ${o.id} is too long for a button`)
        }
        send({
          t: 'ask',
          ask_id: askId,
          text: String(a.text),
          ...(opts.length ? { options: opts.map(o => ({ option_id: o.id, label: o.label })) } : {}),
        })
        return ok(`asked (${askId}) — his answer will arrive as a channel message, do not wait here`)
      }
      case 'done':
        send({ t: 'done', text: String(a.text) })
        return ok('sent')
      case 'ask_resolved':
        send({
          t: 'ask_resolved',
          ask_id: String(a.ask_id),
          how: String(a.how),
          ...(a.outcome ? { outcome: String(a.outcome) } : {}),
        })
        return ok('the buttons are coming off')
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

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The socket.

let sock: import('bun').Socket | null = null
let live = false
/** Frames written before the connection was up. Bounded: a queue that grows is a leak with a plan. */
const pending: string[] = []
const MAX_PENDING = 64

/** THE ONLY WRITER. Rule 1: the newline is appended here and nowhere else. */
function send(payload: Record<string, unknown>): void {
  const line = JSON.stringify({ v: PROTOCOL_VERSION, id: nextId(), ...payload }) + '\n'
  if (Buffer.byteLength(line) > MAX_FRAME_BYTES) {
    // Refused rather than truncated. Half a message on a phone is worse than none and looks the
    // same as a whole one.
    note(`a frame was too big to send (${Buffer.byteLength(line)} bytes); it was NOT sent`)
    return
  }
  if (live && sock) {
    sock.write(line)
    return
  }
  if (pending.length < MAX_PENDING) pending.push(line)
}

/** Say something in this session's own transcript. The operator cannot see it; a developer can. */
function note(msg: string): void {
  process.stderr.write(`kickoff-channel: ${msg}\n`)
}

/** Hand the operator's words to the agent, as a message in its own turn. */
function deliver(content: string, meta: Record<string, unknown>): void {
  void mcp.notification({
    method: 'notifications/claude/channel',
    params: { content, meta: { chat_id: 'hub', user: 'operator', ts: new Date().toISOString(), ...meta } },
  })
}

function readToken(): string | null {
  try {
    if (!existsSync(TOKEN_FILE)) return null
    const t = readFileSync(TOKEN_FILE, 'utf8').trim()
    return t.length ? t : null
  } catch {
    return null
  }
}

let backoff = 1000
let saidItWasDown = false

function connect(): void {
  const token = readToken()
  if (!token) {
    if (!saidItWasDown) {
      note(`no secret at ${TOKEN_FILE}. Run:  herdr-tg enroll ${REPO}`)
      saidItWasDown = true
    }
    setTimeout(connect, 30_000)
    return
  }

  // Rule 3: the ceiling bounds ONE frame. `buf` is cleared at every newline, so a long
  // conversation cannot accumulate into a false "frame too large".
  let buf = ''

  Bun.connect({
    unix: SOCKET,
    socket: {
      open(s) {
        sock = s
        live = true
        backoff = 1000
        saidItWasDown = false
        s.write(
          JSON.stringify({
            v: PROTOCOL_VERSION,
            id: nextId(),
            t: 'hello',
            project_id: `unknown-until-the-hub-says`,
            token,
            instance: INSTANCE,
            repo: REPO,
            pid: process.pid,
          }) + '\n',
        )
        while (pending.length) s.write(pending.shift()!)
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
      close() {
        live = false
        sock = null
        setTimeout(connect, backoff)
        backoff = Math.min(backoff * 2, 60_000)
      },
      error(_s, e) {
        note(`socket error: ${e?.message ?? e}`)
      },
    },
  }).catch(() => {
    live = false
    sock = null
    if (!saidItWasDown) {
      note(`the hub is not listening at ${SOCKET}; retrying`)
      saidItWasDown = true
    }
    setTimeout(connect, backoff)
    backoff = Math.min(backoff * 2, 60_000)
  })
}

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
    case 'welcome':
      note(`connected as "${frame.project}"`)
      break
    case 'refused':
      // A closed set, and each one means something different to whoever is reading the log.
      note(
        {
          unknown_project: `the hub does not know this project. Run:  herdr-tg enroll ${REPO}`,
          bad_token: `the secret at ${TOKEN_FILE} is not one the hub knows. Re-run:  herdr-tg enroll ${REPO}`,
          already_claimed: 'another bridge for this project is already connected; this one will not take over',
          version_skew: 'the hub speaks a different version of this protocol. Run:  kickoff pull',
          not_enabled: 'this project is enrolled but switched off',
          frame_too_large: 'the last frame was over the size ceiling and was refused, not truncated',
        }[frame.reason as string] ?? `refused: ${frame.reason}`,
      )
      break
    case 'ping':
      // The nonce is the ping's own envelope id; the answer names it.
      s.write(JSON.stringify({ v: PROTOCOL_VERSION, id: nextId(), t: 'pong', ref: frame.id }) + '\n')
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
    case 'ack':
      if (frame.delivered === 'no') {
        note(`the hub did not deliver a frame (${frame.why ?? 'no reason given'})`)
      }
      break
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
for (const sig of ['SIGTERM', 'SIGINT'] as const) {
  process.on(sig, () => {
    try {
      if (live && sock) sock.write(JSON.stringify({ v: PROTOCOL_VERSION, id: nextId(), t: 'bye', reason: 'refresh' }) + '\n')
    } catch {
      /* going away regardless */
    }
    setTimeout(() => process.exit(0), 200)
  })
}
