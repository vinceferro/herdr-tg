#!/usr/bin/env bun
/**
 * The smallest thing that counts as an adapter, written from `docs/ATTACHING.md` and nothing else.
 *
 * It imports NOTHING from this repository — not the wire, not the configuration reader, not a type.
 * That is the point of it: the interface is only abstract if somebody who has never read our
 * TypeScript can attach from the document, so this file is the test of the document rather than of
 * the code. `adapters/kickoff-hub-attach/test-two-producers.ts` runs it against the real door, and
 * `test-run.ts` runs it as the engine attach starts. (`adapters/fanin/` is gone; the sentence
 * that named it outlived it.)
 *
 * It does the five steps §1 lists, and two more that §6 added in v22:
 *
 *   1. connect to a Unix socket,
 *   2. write one line of JSON — `hello`, carrying the secret, and promising to answer for a tap,
 *   3. answer a `ping` with a `pong`,
 *   4. write one more line — `say`, and one question with buttons,
 *   5. read the `ack` for it,
 *   6. answer any `choice` with an `ack`, because step 2 promised to,
 *   7. ignore the lease on the `welcome`'s envelope, which §6 says a bridge may hold none of.
 *
 * Everything it refuses to do — guess a directory, keep a secret in a variable, retry a name the
 * hub will not address — is refused because the document said so, and every such refusal names the
 * section it came from. It says what happened on stdout and exits; a real adapter would stay.
 *
 * Deliberately NOT here, because §8 says these are what a wire needs on a bad day and this is the
 * good-day path: the queue, the backoff, the drain handler, the whole-frame rule. Do not copy this
 * file into production; copy it into an understanding.
 */

import { randomBytes } from 'crypto'
import { existsSync, readFileSync } from 'fs'
import { isAbsolute, join } from 'path'

function say(line: string): void {
  process.stdout.write(`${line}\n`)
}

function refuse(why: string): never {
  // §2: "Fail closed and say which variable, in words. A refusal that names the variable costs one
  // line; a silent default costs a conversation nobody can find."
  say(`REFUSED ${why}`)
  process.exit(2)
}

// ── §2, the namespace ──────────────────────────────────────────────────────────────────────────
//
// One variable is required. A variable set to nothing is not set, because opencode substitutes a
// missing `{env:VAR}` with the empty string rather than failing.
const named = (v: string | undefined): string | null => (v && v.length ? v : null)

// §2: `-` means "as if this variable were not set". Stripped before anything else looks, so every
// rule below sees what it would have seen had nothing been inherited. It exists because a config
// that starts a second engine can only OVERLAY what it inherited, never remove it, and the empty
// string is taken by the rule immediately below.
const env: Record<string, string | undefined> = { ...process.env }
for (const name of Object.keys(env)) if (name.startsWith('KICKOFF_HUB_') && env[name] === '-') delete env[name]

// And for every variable whose UNSET behaviour is a working default, empty is its own refusal — it
// must never fall through to that default, because a configuration that set it meant to replace it.
// `KICKOFF_HUB_PROJECT_DIR` is in this list precisely because unset does NOT refuse on its own: it
// falls through to the engine's own `CLAUDE_PROJECT_DIR` first, which is how an adapter attaches as
// a repository nobody named.
for (const name of ['KICKOFF_HUB_PROJECT_DIR', 'KICKOFF_HUB_ADDRESS', 'KICKOFF_HUB_TOKEN_FILE', 'KICKOFF_HUB_SOCKET', 'KICKOFF_HUB_RELAY_SOCKET']) {
  if (env[name] === '') refuse(`${name} is set to nothing at all, which §2 says is never a value`)
}

const dirGiven = named(env.KICKOFF_HUB_PROJECT_DIR)
if (!dirGiven) refuse('nothing set KICKOFF_HUB_PROJECT_DIR, and §2 says never to guess from cwd')
// §2: an absolute path, or the single character "." meaning "the directory I was started in, and
// whoever wrote this vouches for it".
const projectDir = dirGiven === '.' ? process.cwd() : dirGiven
if (!isAbsolute(projectDir)) refuse(`KICKOFF_HUB_PROJECT_DIR is "${dirGiven}", which is neither a full path nor "."`)

// ── §4, the address ────────────────────────────────────────────────────────────────────────────
//
// Five shapes the hub refuses, checked here because §4 says `bad_lane` is permanent — the same name
// is refused every time, so learning it from a refusal frame costs a claim and a round trip to be
// told something that was readable off the configuration.
const wanted = env.KICKOFF_HUB_ADDRESS
let address: string | null = null
if (wanted !== undefined) {
  if (Buffer.byteLength(wanted, 'utf8') > 64) refuse(`the address "${wanted}" is over 64 bytes`)
  if (wanted === '.' || wanted === '..') refuse(`the address "${wanted}" names a folder`)
  if (wanted.includes('/') || wanted.includes('\\')) refuse(`the address "${wanted}" has a slash in it`)
  if (/[\u0000-\u001f\u007f-\u009f]/.test(wanted)) refuse('the address has a character in it that cannot be printed')
  address = wanted
}

// ── §5, the credential ─────────────────────────────────────────────────────────────────────────
//
// The environment carries a PATH to the secret and never the secret. This adapter takes the told
// path when there is one and otherwise looks where §5 says enrolment puts it; it does not implement
// the upward search, because §5 says an adapter that does not search needs the secret at
// `<project dir>/.kickoff/hub.token` or needs to be told the path, and being told is cheaper.
const tokenFile = named(env.KICKOFF_HUB_TOKEN_FILE) ?? join(projectDir, '.kickoff', 'hub.token')
if (!existsSync(tokenFile)) refuse(`there is no secret at ${tokenFile}; run: kickoff-channel enroll ${projectDir}`)
const token = readFileSync(tokenFile, 'utf8').trim()
if (!token) refuse(`the secret at ${tokenFile} is empty`)

// ── §2 and §9, where to dial ───────────────────────────────────────────────────────────────────
//
// §9: a producer behind a relay must NEVER fall back to dialling the hub when it cannot find its
// relay. That is two writers racing for one claim, which is the whole thing the claim exists to
// prevent.
const uid = process.getuid?.() ?? 0
const viaRelay = env.KICKOFF_HUB_RELAY === '1'
const socket = viaRelay
  ? named(env.KICKOFF_HUB_RELAY_SOCKET) ??
    refuse('KICKOFF_HUB_RELAY is 1 and nothing said where the relay is; §9 forbids falling back to the hub')
  : named(env.KICKOFF_HUB_SOCKET) ?? `/run/user/${uid}/kickoff/hub.sock`

// ── §6, the handshake ──────────────────────────────────────────────────────────────────────────

const PROTOCOL_VERSION = 1
const A_MOMENT_FOR_HIS_THUMB = 1_500
let seq = 0
let up = false
/** The id of the line this adapter came to say. Its `ack` is what it waits for. */
let said: string | null = null
/** Has a tap been answered for? Then there is nothing left to wait on. */
let answered = false
let leaving = false

const conn = await Bun.connect({
  unix: socket,
  socket: {
    open(s) {
      // §6 step 3: `hello` within 5 seconds. §3: the secret proves the PROJECT, so `project_id` is
      // never consulted — this puts something honest about that in it. §9: always send an instance,
      // because a relay answers `version_skew` to a `hello` without one.
      write(s, {
        t: 'hello',
        project_id: 'unknown-until-the-hub-says',
        token,
        // §3b: an instance may carry no pid, no path and no hostname. It is the one string a
        // whole fleet reads back — every open question is keyed on it — and a pid means something
        // only to this kernel and is handed out again once its numbers wrap. Random, then a clock.
        instance: `${randomBytes(8).toString('hex')}-${Date.now()}`,
        repo: projectDir,
        pid: process.pid,
        // §4: never send `"lane": null`. Omit the field.
        ...(address ? { lane: address } : {}),
        // §6: which of the hub's own frames this adapter will answer with an `ack`. Promising it is
        // what makes the operator's line say whether his answer was taken, and §8 rule 13 is the
        // other half of the bargain: having promised, every `choice` gets exactly one answer. An
        // adapter that would rather not is silent here — an EMPTY list is no promise either, and
        // saying nothing keeps the behaviour every adapter had before v22.
        confirms: ['choice'],
      })
    },
    data(s, chunk) {
      buf += chunk.toString()
      // §8 rule 2: read exactly one line at a time, never to EOF.
      for (;;) {
        const nl = buf.indexOf('\n')
        if (nl < 0) break
        const line = buf.slice(0, nl)
        buf = buf.slice(nl + 1)
        if (line.trim()) onFrame(s, JSON.parse(line))
      }
    },
    close() {
      if (!up) say('CLOSED before anything was welcomed')
    },
    error(_s, e) {
      say(`ERROR ${(e as Error)?.message ?? e}`)
    },
  },
}).catch(() => refuse(`nothing is listening at ${socket}`))

let buf = ''

/**
 * §8 rule 1: the trailing newline is appended in exactly one place.
 *
 * It hands the id back because §6 says every frame after `hello` gets exactly one `ack` naming it,
 * and this adapter waits for the one belonging to the line it came to say. Nothing here stamps a
 * `generation`: see the `welcome` arm.
 */
function write(s: import('bun').Socket, payload: Record<string, unknown>): string {
  const id = `s${++seq}`
  s.write(JSON.stringify({ v: PROTOCOL_VERSION, id, ...payload }) + '\n')
  return id
}

/**
 * §6 step 10: `bye`, then wait for the kernel to take it before exiting — `process.exit()` on the
 * same tick loses it to a short write.
 *
 * It waits a moment first, because this adapter asked a question and a question nobody is there to
 * answer for is worse than no question: leaving the instant its own line was acked would take the
 * buttons off his phone (§9, the grace window) before a thumb could reach them. A real adapter
 * stays for as long as its agent does; this one is a worked example and leaves.
 */
function leave(s: import('bun').Socket): void {
  if (leaving) return
  leaving = true
  setTimeout(() => {
    write(s, { t: 'bye', reason: 'said what it came to say' })
    setTimeout(() => process.exit(0), 200)
  }, answered ? 0 : A_MOMENT_FOR_HIS_THUMB)
}

function onFrame(s: import('bun').Socket, f: Record<string, any>): void {
  switch (f.t) {
    case 'welcome': {
      // §4: if you named an address and it does not come back, refuse and disconnect — the hub is
      // older than you and has admitted you AS THE WHOLE PROJECT.
      if (address && f.lane !== address) {
        say(`REFUSED the hub did not give ${address} a place of its own`)
        s.end()
        process.exit(3)
      }
      up = true
      say(`WELCOME ${f.project}${f.lane ? ` · ${f.lane}` : ''}`)
      // §6, the lease: it is on this frame's OWN ENVELOPE — `f.generation` — and never in the
      // payload. This adapter holds none on purpose, which §6 permits: a bridge that stamps no
      // lease is fenced by its socket and its pid, exactly as every bridge was before leases
      // existed, and the smallest thing that counts as an adapter need not carry the number at all.
      // What it must NOT do is stamp `f.generation ?? 0`: §6 says a zero is read as silence, so the
      // `?? 0` that comes naturally in this language would put on the wire the one value a lease
      // cannot mean. Nothing below writes a `generation`.
      //
      // §6 step 4 in the smallest-adapter list is "write one more line", and it goes HERE rather
      // than after a ping: §9 says a relay never pings its producers, so an adapter that waited for
      // one before speaking would sit silent for ever behind one.
      said = write(s, { t: 'say', text: 'attached from the document alone' })
      // §7 offer 4: a question with buttons, so there is something for him to tap and something for
      // the promise above to be about. The ids and the labels are this adapter's to mint.
      write(s, {
        t: 'ask',
        ask_id: 'a1',
        text: 'Attached from the document. Anything to say back?',
        options: [{ option_id: 'ok', label: 'Nothing, carry on' }, { option_id: 'stop', label: 'Stop' }],
      })
      return
    }
    case 'ping':
      // §6: the ping's own envelope id is the nonce, and §8 rule 12 says answer it in the wire
      // layer — liveness is what keeps the claim.
      write(s, { t: 'pong', ref: f.id })
      return
    case 'choice': {
      // Promised on the `hello`, so it is answered — exactly once, and §8 rule 13 says what the
      // answer means: `accepted` is "the answer is in the agent's turn", never "I read the frame".
      // Here the agent IS this file and reading it is taking it, which is the only reason
      // `accepted` is honest this early; an adapter with an engine behind it says `accepted` when
      // its engine has the answer and `refused`, with one sentence the operator can read, when it
      // never will.
      say(`CHOICE ${f.option_id}`)
      write(s, { t: 'ack', ref: f.id, status: 'accepted' })
      // §7 offer 5: the question has stopped being open, so say so — after a tap too, which is the
      // second chance rather than a duplicate.
      write(s, { t: 'ask_resolved', ask_id: f.ask_id, how: 'answered' })
      answered = true
      return
    }
    case 'ack':
      // §7 offer 3: three values, and `unseen` is not success. Branch on `=== "yes"`, never on
      // `!== "no"`.
      // The hub's ack carries `delivered`; the one this adapter sent up carries `status`, and the
      // hub acks THAT one too, because every frame after `hello` gets exactly one. It is
      // bookkeeping — answering it would be a loop with no end.
      if (f.delivered === undefined) return
      say(`ACK ${f.delivered === 'yes' ? 'reached' : f.delivered}${f.why ? ` (${f.why})` : ''}`)
      if (f.ref === said) leave(s)
      return
    case 'refused':
      say(`REFUSED ${f.reason}`)
      process.exit(4)
    default:
      // §8: an unknown frame kind is ignored, never fatal.
      return
  }
}

// Nothing here waits on a ping before it speaks, so a hub that never welcomes it must not hang a
// test for ever either.
setTimeout(() => {
  if (!up) refuse(`nothing welcomed this adapter at ${socket}`)
}, 10_000)

void conn
