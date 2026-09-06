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

import { closeSync, constants, fstatSync, existsSync, lstatSync, openSync, readFileSync, realpathSync, type Stats } from 'fs'
import { basename, dirname, isAbsolute, join } from 'path'

/** Open the file and NOT whatever a link at that path points at. */
const { O_NOFOLLOW, O_RDONLY } = constants

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
 * What is wrong with `--opencode-binding-file`, or null when nothing is.
 *
 * The path is what whatever starts the engine writes this worker's own binding into, and it is read
 * afresh on every line the operator types — so a relative path would be read against whatever
 * directory attach happens to be started in, and a wall started from two places would read two
 * different files. A binding with no `--opencode` names a session on a server nothing is watching.
 */
export function bindingFileProblem(path: string | null, opencodeUrl: string | null): string | null {
  if (path === null) return null
  if (!isAbsolute(path)) {
    return `--opencode-binding-file ${path} is not an absolute path; give the full path of the file whatever starts the engine writes the worker's binding into`
  }
  if (opencodeUrl === null) {
    return '--opencode-binding-file was given without --opencode; the binding names a session on an opencode server, and without --opencode there is no server being watched'
  }
  return null
}

/**
 * What is wrong with `--opencode-binding-generation`, or null when nothing is.
 *
 * The number is the FLOOR the fence is held at, and the reason it is on the command line rather
 * than remembered is that the memory is what a restart destroys: the monotonic rule inside a
 * running watcher dies with the process, so a wall restarted after a rollover reads whatever note
 * it finds and takes it as the newest thing it has ever seen. Whatever starts the wall is the same
 * party that numbers the bindings, so it is the party that can say the floor — and it says it once,
 * for the life of that process, where nothing the wall reads later can lower it.
 *
 * Refused rather than ignored when it is not a whole number: a floor misread as none is a fence
 * standing open, and it would stand open in silence.
 */
export function bindingGenerationProblem(given: string | null, bindingFile: string | null): string | null {
  if (given === null) return null
  if (!/^[0-9]+$/.test(given) || !Number.isSafeInteger(Number(given))) {
    return `--opencode-binding-generation ${given} is not a whole number; give the number of the binding this worker was started for, e.g. --opencode-binding-generation 7`
  }
  if (bindingFile === null) {
    return '--opencode-binding-generation was given without --opencode-binding-file; the number is the oldest binding that file may name, and without the file there is no binding for it to hold'
  }
  return null
}

/** What the binding says: the worker's own session, and what may be checked about it. */
export type SessionBinding = {
  /** The session the operator's words go to, and the only session whose questions reach him. */
  sessionID: string
  /** The project directory the launcher says that session is for, when it says. */
  directory: string | null
  /** The agent the launcher says that session runs, when it says. */
  agent: string | null
  /** Which writing of the binding this is, when the launcher numbers them. */
  generation: number | null
}

/**
 * The binding, read whole, or the sentence the operator gets instead.
 *
 * `state` is for `--check`, which has no operator to talk to and says what it found in a
 * developer's register; `refused` is what goes into the topic he typed in, so it names no path, no
 * id and no file — the binding is a thing he has never been told exists. `why` is the half only
 * whoever runs the wall can act on (a mode, an owner, a directory), and it goes to the journal.
 */
export type BindingRead =
  | { binding: SessionBinding }
  | {
      refused: string
      state: 'not written yet' | 'unreadable' | 'unreadable shape' | 'a form it does not know' | 'not safe to read'
      why?: string
    }

/** A session id as opencode mints them, measured against 1.18.25: `ses_` and then base62. */
const SESSION_ID = /^ses_[A-Za-z0-9]{1,60}$/

/**
 * The one shape this reads, stamped by whoever writes it.
 *
 * ONE shape, and it says which it is. A reader that takes several has to decide what a launcher
 * meant by the one it did not write, and that guess is the whole of what this flag exists to
 * remove: the id on a line said which session and could say nothing else, so a launcher that
 * learned to narrow the binding — to a project, to an agent, to a numbered writing — had no way to
 * say so that an older reader would not silently ignore. The number is how a launcher and a reader
 * that disagree find out, instead of meeting in the middle on his typed words.
 */
const THE_FORM_THIS_READS = 1

/**
 * Every key the form has. A CLOSED set, on purpose: a key this attach does not know may be a
 * NARROWING of which session may be spoken to — a title, a model, a worker id somebody adds later —
 * and obeying the rest of the binding while quietly dropping it would deliver his words on a rule
 * nobody checked. A launcher that adds a key upgrades the attach that reads it; they are two halves
 * of one wall, and this file is not a wire contract.
 */
const THE_KEYS_IT_HAS = ['v', 'session', 'directory', 'agent', 'generation']

/**
 * The most a binding file may be before it is refused unread. The longest legitimate one — a
 * version, an id, a directory, an agent and a number — is a couple of hundred bytes; four kilobytes
 * is generous enough that no launcher will meet it and small enough that a flag aimed at the wrong
 * file costs nothing.
 */
const MOST_A_BINDING_CAN_BE = 4096

/** What he is told when the file is there and this refused to trust a byte of it. */
const COULD_NOT_BE_READ = "the note naming the worker's session could not be read"
/** What he is told when it was read and says something this cannot make sense of. */
const NOT_ONE_IT_CAN_READ = "the note naming the worker's session is not one it can read"
/** What he is told when it was written by a launcher this attach is not the other half of. */
const A_FORM_IT_DOES_NOT_KNOW = "the note naming the worker's session is written in a form this worker does not know"

/**
 * Read the binding naming this worker's session, in the one place both the check and the watcher
 * ask.
 *
 * Read WHOLE and never partly trusted: a launcher writes it by rename, but a launcher that does not
 * would otherwise be read half way — and half of a session id is a valid-looking one that addresses
 * somebody else. Anything that is not the one shape is a refusal rather than a guess, because the
 * whole point of the binding is that nothing here guesses.
 *
 * And everything about the FILE is proved before a byte of it is believed, because every one of
 * these is a way for somebody who is not the launcher to say which session the operator is
 * steering, and none of them can be told from the launcher's own writing once the words are read:
 * the place it sits, the link it might be, who owns it, and who else can write it.
 */
export function readBindingFile(path: string): BindingRead {
  const cannotBeRead = { refused: COULD_NOT_BE_READ, state: 'unreadable' } as const
  const notWrittenYet = { refused: NOT_WRITTEN_YET, state: 'not written yet' } as const
  const notSafe = (why: string): BindingRead => ({ refused: COULD_NOT_BE_READ, state: 'not safe to read', why })

  const unsafePlace = whereItSitsIsUnsafe(path)
  if (unsafePlace) return notSafe(unsafePlace)

  // WHAT the path leads to, before a byte of it is opened. Two ways this process dies otherwise:
  // a named pipe or a device at the path makes `readFileSync` block in the kernel until somebody
  // writes, and this process is single-threaded — the event stream, every typed line and `--check`
  // itself stop for ever while the process stays alive holding the claim, so nothing restarts it.
  // And a flag pointed at a log would be read whole into memory on every line the operator types.
  //
  // `lstat`, not `stat`: a link at the path is somebody else's answer to which session this worker
  // speaks to, and it can be re-pointed between two lines he types without the file it names ever
  // changing.
  let seen: Stats
  try {
    seen = lstatSync(path)
  } catch (e) {
    if ((e as { code?: string })?.code === 'ENOENT') return notWrittenYet
    return cannotBeRead
  }
  if (seen.isSymbolicLink()) return notSafe(`${path} is a link to somewhere else`)
  if (!seen.isFile()) return cannotBeRead

  // Opened WITHOUT following a link, and everything else asked of the open file rather than of the
  // path: between the look above and the read below, the path can be made to lead somewhere else,
  // and a check on the name proves nothing about the bytes.
  let fd: number
  try {
    fd = openSync(path, O_RDONLY | O_NOFOLLOW)
  } catch (e) {
    const code = (e as { code?: string })?.code
    if (code === 'ENOENT') return notWrittenYet
    if (code === 'ELOOP') return notSafe(`${path} is a link to somewhere else`)
    // EACCES, EISDIR: the file is there in some form and cannot be read, which is a different fix
    // from "not yet" — and never a fall back to a guess.
    return cannotBeRead
  }
  let text: string
  try {
    const st = fstatSync(fd)
    if (!st.isFile()) return cannotBeRead
    // Who wrote it. A binding this process's own user did not write is one somebody else wrote,
    // and a session somebody else chose is where his typed words would go.
    if (st.uid !== process.getuid!()) return notSafe(`${path} belongs to somebody else`)
    // Who could have. A file another account can write is a binding another account can set; one
    // another account can read is a session id it has no business knowing.
    if ((st.mode & 0o077) !== 0) return notSafe(`${path} can be read or written by somebody other than the user this runs as`)
    if (st.size > MOST_A_BINDING_CAN_BE) return cannotBeRead
    text = readFileSync(fd, 'utf8')
  } catch {
    return cannotBeRead
  } finally {
    closeSync(fd)
  }

  const trimmed = text.trim()
  if (trimmed.length === 0) return notWrittenYet
  const bad = { refused: NOT_ONE_IT_CAN_READ, state: 'unreadable shape' } as const
  const notThisForm = { refused: A_FORM_IT_DOES_NOT_KNOW, state: 'a form it does not know' } as const
  // The id on a line is the form this flag was born with, and it is told APART from nonsense: a
  // launcher still writing it is a launcher to upgrade, and that is a different sentence from
  // "this is not a binding at all".
  if (!trimmed.startsWith('{')) return SESSION_ID.test(trimmed) ? notThisForm : bad
  let o: Record<string, unknown>
  try {
    o = JSON.parse(trimmed) as Record<string, unknown>
  } catch {
    return bad
  }
  if (o === null || typeof o !== 'object' || Array.isArray(o)) return bad
  // Two keys of one name: JSON keeps the LAST and `Object.keys` sees one, so a binding whose first
  // `session` line is the one a person reads is delivered to the second. Whatever wrote it is
  // confused about which session this worker is, and this is the one file the whole flag treats as
  // authoritative — "what it says is not what it does" is the property that must not exist here.
  if (keysInTheText(trimmed) !== Object.keys(o).length) return bad
  // The version before anything else, so a launcher writing a form this cannot read is told that
  // and not told its perfectly good binding is unreadable rubbish.
  if (o.v !== THE_FORM_THIS_READS) return notThisForm
  for (const key of Object.keys(o)) {
    if (!THE_KEYS_IT_HAS.includes(key)) return bad
  }
  const id = o.session
  if (typeof id !== 'string' || !SESSION_ID.test(id)) return bad
  const directory = o.directory === undefined || o.directory === null ? null : o.directory
  if (directory !== null && (typeof directory !== 'string' || !isAbsolute(directory))) return bad
  const agent = o.agent === undefined || o.agent === null ? null : o.agent
  if (agent !== null && (typeof agent !== 'string' || agent.length === 0)) return bad
  const generation = o.generation === undefined || o.generation === null ? null : o.generation
  if (generation !== null && (typeof generation !== 'number' || !Number.isSafeInteger(generation) || generation < 0)) return bad
  return { binding: { sessionID: id, directory: directory as string | null, agent: agent as string | null, generation: generation as number | null } }
}

/**
 * How many keys the TEXT has at its top level, duplicates counted each time.
 *
 * Scanned rather than parsed, because the parser is exactly what loses the answer: it is the count
 * `Object.keys` cannot give.
 */
function keysInTheText(text: string): number {
  let keys = 0
  let depth = 0
  let inString = false
  let escaped = false
  let aKeyComesNext = false
  for (const c of text) {
    if (inString) {
      if (escaped) escaped = false
      else if (c === '\\') escaped = true
      else if (c === '"') {
        inString = false
        if (depth === 1 && aKeyComesNext) {
          keys++
          aKeyComesNext = false
        }
      }
      continue
    }
    if (c === '"') inString = true
    else if (c === '{') {
      depth++
      if (depth === 1) aKeyComesNext = true
    } else if (c === '[') depth++
    else if (c === '}' || c === ']') depth--
    else if (c === ',' && depth === 1) aKeyComesNext = true
  }
  return keys
}

/**
 * Whether the PLACE the binding file sits lets somebody who is not the launcher decide what is read
 * there — the half of it that a check on the file itself cannot see.
 *
 * A directory anybody can write in is a directory anybody can move the binding out of and put their
 * own in its place, or plant a link for it to be reached through; a directory somebody else owns is
 * theirs to do that in whenever they like. Checked on the path as it was GIVEN and on what it
 * RESOLVES to, because a link in the middle of the path leads somewhere this process was never told
 * about, and the safety of the place it actually reads is what matters.
 *
 * The sticky bit is the one exception, and only because it is exactly the rule that stops one user
 * moving another's file — without it `/tmp`, which is where a wall's private door already lives,
 * would fail this for everyone.
 *
 * Returns a sentence for the journal, naming the directory, or null. Nothing here reaches the
 * operator: the fix is a `chmod` on a box he has never been told about.
 */
function whereItSitsIsUnsafe(path: string): string | null {
  const parent = dirname(path)
  const chains = new Set([parent])
  try {
    chains.add(realpathSync(parent))
  } catch {
    // Nothing is there yet, or it cannot be resolved. The read of the file itself says which, and
    // says it in the operator's words.
  }
  const us = process.getuid!()
  for (const chain of chains) {
    for (const dir of ancestors(chain)) {
      let st: Stats
      try {
        st = lstatSync(dir)
      } catch (e) {
        // Nothing is there yet — the ordinary case for a wall checked before its launcher has made
        // the directory it writes into. The file's own read then says "not written yet", which is
        // the true sentence; turning that into a refusal about safety would send whoever runs the
        // wall looking for a permission problem that does not exist.
        if ((e as { code?: string })?.code === 'ENOENT') return null
        return `${dir} could not be looked at`
      }
      if (st.isSymbolicLink()) {
        // Followed by everything under it, so whoever can replace the link chooses the file. Its
        // own permission bits mean nothing on Linux; who owns it is the whole of it.
        if (st.uid !== us && st.uid !== 0) return `${dir} is a link somebody else can replace`
        continue
      }
      if (!st.isDirectory()) return `${dir} is not a directory`
      if (st.uid !== us && st.uid !== 0) return `${dir} belongs to somebody else`
      if ((st.mode & 0o022) !== 0 && (st.mode & 0o1000) === 0) return `anybody can write in ${dir}`
    }
  }
  return null
}

/** Every directory on the way to this one, `/` first — the order they are resolved in. */
function ancestors(dir: string): string[] {
  const out: string[] = []
  let at = dir
  for (;;) {
    out.push(at)
    const up = dirname(at)
    if (up === at) break
    at = up
  }
  return out.reverse()
}

/**
 * The sentence for a binding that is not there yet, said the same way for an absent file and an
 * empty one — a launcher that creates the file and writes it a moment later is the ordinary case,
 * and "try again in a moment" is the true instruction for both.
 */
const NOT_WRITTEN_YET = 'the worker has not yet said which session to speak to; try again in a moment'

/**
 * Whether two paths name the same project directory.
 *
 * Trailing slashes off, and then — only when the strings differ — both sides resolved. The two
 * sides come from different places and one of them has already been through the kernel: opencode
 * stores and reports the directory it RESOLVED, which is what a launcher copies into the note,
 * while attach was handed the path on its command line and never resolves it. A wall whose project
 * is reached through a symlink is the ordinary way to have those disagree, and comparing them as
 * strings told the operator his words belonged to a different project than the one he is plainly
 * in — for ever, with nothing he could do about it. Resolving can only ever make two paths that
 * are the same directory compare equal; it cannot make two different directories match. A path
 * that cannot be resolved (it has gone, or is not readable) falls back to what it was given, which
 * is the fail-closed half: two unresolvable paths only match if they were already the same string.
 */
export function sameDirectory(a: string, b: string): boolean {
  const trim = (s: string) => s.replace(/\/+$/, '')
  if (trim(a) === trim(b)) return true
  const resolved = (p: string) => {
    try {
      return trim(realpathSync(p))
    } catch {
      return trim(p)
    }
  }
  return resolved(a) === resolved(b)
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
  // Under --run the child is handed the conversation attach was told; hand-started, a tool server
  // working from git has only git, which never names a room.
  const gitDoor = doorDerivedFromGit(c, run)
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
  if (c.conversation) {
    // Told a conversation, hand-started: git names the folder's own conversation, never a room,
    // so the two derive different doors — and "use the worktree's own name" would be a
    // wild-goose chase, because the address may well be git's already.
    return {
      ok: false,
      warn: true,
      text: `a tool server that works out its door from git here would look for ${gitDoor}, the door of the conversation this folder is bound to and not of conversation ${c.conversation}; give the engine KICKOFF_HUB_CONVERSATION=${c.conversation}, or a config that names KICKOFF_HUB_RELAY_SOCKET=${door}`,
    }
  }
  return {
    ok: false,
    warn: true,
    text: `a tool server that works out its door from git here would look for ${gitDoor}; either use the worktree's own name as the address, or give the engine a config that names KICKOFF_HUB_RELAY_SOCKET=${door}`,
  }
}
