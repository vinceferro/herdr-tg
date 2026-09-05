/**
 * What attach WOULD do, worked out without doing it — shared by the start (`main.ts`) and the check
 * (`check.ts`), so the two cannot disagree.
 *
 * `--check` is what a wrapper runs before trusting a wall, and its whole value is that what it
 * blesses, the worker can do. The first version kept three of the start's refusals in `main.ts`
 * alone — `KICKOFF_HUB_RELAY` on attach itself, a TMPDIR no private door can be made under, a
 * `--opencode` URL with no port — and said "everything a worker here needs is in place" to all
 * three; a wrapper that believed it started a worker that died at once, and systemd restarted it
 * every five seconds for ever. Each of those rules now lives here, once, and both callers ask it.
 */

import { existsSync } from 'fs'
import { basename, isAbsolute, join } from 'path'

import { doorDerivedFromGit, type Attachment } from '../../plugins/kickoff-channel/attach.ts'

/**
 * Why attach cannot start as a PRODUCER, or null when it is not being asked to.
 *
 * attach as a whole holds the claim; its watcher is a producer of its own door. attach itself set
 * to be a producer would dial its own door and hold a claim for one conversation twice over. The
 * pinned environment of every `--run` child carries `KICKOFF_HUB_RELAY=1`, so a worker started
 * from a shell inside a worker inherits it — which is exactly how this reaches attach by accident.
 */
export function producerFlagProblem(c: Attachment): string | null {
  return c.viaRelay
    ? 'KICKOFF_HUB_RELAY is set on attach itself; it belongs on a producer that attaches to attach, not on attach'
    : null
}

/** The folder a private door is made in is `<TMPDIR>/kickoff-hub-attach-<six random characters>/`. */
export const PRIVATE_DOOR_PREFIX = 'kickoff-hub-attach-'

/**
 * Where a private door would go, and what is wrong with the temporary directory if anything.
 *
 * Checked without making anything, so the check can say the same sentence the start dies with.
 * This box has already met the literal string `%h/.cache/tmp` here — an unexpanded systemd
 * specifier that is not a path. A relative or missing temporary directory is a refusal naming it,
 * never a guess; and `sun_path` caps at 108 bytes, terminator included, so a path past that binds
 * nothing and the failure is opaque.
 */
export function privateDoorPlan(tmpdir: string | undefined): { tmp: string; problem: string | null } {
  const tmp = tmpdir ?? '/tmp'
  if (!isAbsolute(tmp)) {
    return { tmp, problem: `TMPDIR is "${tmp}", which is not an absolute path, so a private door cannot be made under it; set TMPDIR to a real directory` }
  }
  if (!existsSync(tmp)) {
    return { tmp, problem: `TMPDIR is "${tmp}", which does not exist, so a private door cannot be made under it; set TMPDIR to a real directory` }
  }
  const longest = join(tmp, `${PRIVATE_DOOR_PREFIX}XXXXXX`, 'door.sock')
  const bytes = Buffer.byteLength(longest, 'utf8')
  if (bytes >= 108) {
    return { tmp, problem: `a private door under TMPDIR "${tmp}" would be ${bytes} bytes, past the 108-byte socket limit; set TMPDIR to a shorter directory` }
  }
  return { tmp, problem: null }
}

/**
 * What is wrong with the `--opencode` URL, or null when nothing is.
 *
 * A port is required, explicitly. The unit builds this URL from `${OPENCODE_PORT}`, and with the
 * variable unset that is `http://127.0.0.1:` — which parses, and which sent the watcher to port 80
 * beside a server on 4096: the worker held the claim, got its topic, and delivered nothing, with
 * a green check in front of it.
 */
export function opencodeUrlProblem(url: string): string | null {
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return `--opencode ${url} is not a URL; give it the server's address with its port, e.g. --opencode http://127.0.0.1:9711`
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return `--opencode ${url} is not an http URL; give it the server's address with its port, e.g. --opencode http://127.0.0.1:9711`
  }
  if (!parsed.port) {
    return `--opencode ${url} names no port; the watcher would dial the wrong server. Give it the port opencode serve was given, e.g. --opencode http://127.0.0.1:9711`
  }
  return null
}

/**
 * Whether the operator's phone reaches this engine both ways, or why only one.
 *
 * On opencode everything the agent did NOT choose to say — its questions, its permission prompts —
 * and everything the operator types back travel through the watcher `--opencode` starts; the tool
 * server the engine spawns carries only what the agent chose to say. A wall started with
 * `--run opencode …` and no `--opencode` is therefore half a phone, and it looked whole: every line
 * of the check was green. A warning rather than a refusal, because the wall works for what it does
 * carry, and typed words are refused out loud on it rather than dropped — nothing is silent, only
 * less. Null when there is nothing to say.
 */
export function typedWordsFact(run: string[] | null, opencodeUrl: string | null): { ok: boolean; text: string; warn: boolean } | null {
  if (!run || opencodeUrl !== null || basename(run[0] ?? '') !== 'opencode') return null
  return {
    ok: true,
    warn: true,
    text: 'the engine is opencode and no --opencode <url> was given, so its questions, its permission prompts and what the operator types on his phone all reach nothing; add --opencode http://127.0.0.1:<the port opencode serve was given>',
  }
}

/**
 * What the engine's tool server will do about the door — one line for `--check`, and the same
 * sentence as attach's own warning at start when there is something to warn about.
 *
 * The tool server reaches the door in one of two ways: told (`KICKOFF_HUB_RELAY_SOCKET`, which
 * `--run` pins into the engine's environment) or derived from git in its cwd. The two meet only
 * when attach's door IS the one git derives — the worktree's own name as the address, or none. A
 * dispatcher that mints another address, or a wrapper that names the door, breaks that — harmless
 * under `--run` unless the engine's config overlays the pinned door with `-`, and a real
 * split-brain without `--run`. attach cannot read the engine's config, so it cannot refuse; it
 * says which case this is instead. Null when there is no door at all: the door's own line has
 * already said what to set.
 */
export function toolServerFact(c: Attachment, door: string | null, run: boolean): { ok: boolean; text: string; warn: boolean } | null {
  if (!door) return null
  const gitDoor = doorDerivedFromGit(c)
  if (!gitDoor) {
    if (run) return { ok: true, warn: false, text: 'a tool server here must be told the door, and a worker started with --run tells it' }
    return {
      ok: true,
      warn: false,
      text: `a tool server here cannot work out a door from git, so it must be given the same KICKOFF_HUB_RELAY_SOCKET=${door}`,
    }
  }
  if (gitDoor === door) {
    return { ok: true, warn: false, text: 'a tool server that works out its door from git here finds this one' }
  }
  if (run) {
    return {
      ok: true,
      warn: true,
      text: `the tool server the engine spawns is told this door by --run; a config that overlays KICKOFF_HUB_RELAY_SOCKET with - would look for ${gitDoor} instead and never find this one`,
    }
  }
  if (c.relaySocketWasGiven) {
    return {
      ok: true,
      warn: true,
      text: `the door was named by KICKOFF_HUB_RELAY_SOCKET, so a tool server that works one out from git would look for ${gitDoor}; give the engine the same KICKOFF_HUB_RELAY_SOCKET=${door}`,
    }
  }
  return {
    ok: false,
    warn: true,
    text: `a tool server that works out its door from git here would look for ${gitDoor}; either use the worktree's own name as the address, or give the engine a config that names KICKOFF_HUB_RELAY_SOCKET=${door}`,
  }
}
