/**
 * How a configuration becomes an attachment — read in ONE place, for every adapter.
 *
 * `docs/ATTACHING.md` is the contract this file implements, and it is written to be implementable
 * by a stranger. Everything an adapter needs to attach is here: which project it speaks for, which
 * conversation of that project it is, where the secret is, and what to dial.
 *
 * # Why one file
 *
 * Eleven variables across five prefixes said the same three things in three different ways, and no
 * two of the three adapters agreed on any of them: three names for "which directory am I", two
 * vouching flags, one socket variable meaning two different things, and a bare-cwd guess in the one
 * adapter that had no flag at all. An adopter attaching something new had to read four files and
 * guess. One namespace, `KICKOFF_HUB_`, and one reader, so that a name means one thing.
 *
 * # The empty-string rule, and why it is not pedantry
 *
 * A variable set to nothing is not set. opencode substitutes a missing `{env:VAR}` in its config
 * with the EMPTY STRING rather than failing, so an empty value always means a configuration asked
 * for something that was not there — and every one of these refuses outright rather than falling
 * back, because the safe-looking reading is the misroute. An empty address silently taken as "no
 * address" puts a lane's words in the project's topic and takes the project's claim; an empty
 * project directory falls through to the ENGINE's own variable and attaches as whatever repository
 * an outer session named. The one exception is `KICKOFF_HUB_RELAY`, where unset already means "I
 * hold the claim myself" — a real answer, so empty is that answer too.
 *
 * # The credential does not travel in the environment
 *
 * The environment is promiscuous: it is inherited by every descendant of every process, across
 * engines and wrappers. This box has been bitten twice — `HERDR_PANE_ID` reaching a bridge three
 * levels below a terminal that never meant to talk to it, and `CLAUDE_PROJECT_DIR` crossing an
 * engine boundary until a tool server resolved ANOTHER repository's secret. So a variable here
 * carries a PATH to the secret and never the secret, and the filesystem enforces 0600.
 *
 * # And because the environment is promiscuous, every one of these can be un-inherited
 *
 * Making a namespace does not stop it crossing an engine boundary — it only gives the crossing one
 * prefix instead of five. A config that starts a second engine inherits whatever the first engine's
 * session was dispatched with, and opencode-shaped configs can only OVERLAY a variable, never
 * remove one. The empty string cannot be the way to remove one, because that is exactly what a
 * failed substitution produces. So `-` means "as if this variable were not set", for every variable
 * in this namespace, and a config that starts a second engine overlays all of them with it.
 */

import { createHash } from 'crypto'
import { isAbsolute, join } from 'path'

import {
  boundConversationFor,
  channelHome,
  conversationSecret,
  factsFor,
  findProject,
  isConversationId,
  legacyTokenFolder,
  readSecret,
  seedIdOf,
  type Facts,
  type Project,
} from './where.ts'

/** The hub refuses an address longer than this, counted in bytes — `MAX_LANE` in `hub.rs`. */
export const MAX_ADDRESS_BYTES = 64

/** A variable set to nothing is not set. See the header. */
const named = (v: string | undefined): string | null => (v && v.length ? v : null)

/**
 * The value that means "as if this variable were not set", for every variable in the namespace.
 *
 * A config that starts a second engine cannot unset what it inherited — opencode's can only overlay
 * — and the empty string is taken, because that is what a `{env:VAR}` substitution produces when
 * the variable is not there. Without a spelling for "ignore this", an inherited `KICKOFF_HUB_ADDRESS`
 * makes a session speak into a conversation nobody opened for it, and an inherited
 * `KICKOFF_HUB_TOKEN_FILE` attaches it with another repository's secret — silently, because it
 * really does find one. That is the second incident in the header arriving through the namespace
 * that was introduced to prevent it.
 *
 * A lone hyphen is never a path, never a socket, never a number of milliseconds, and never a
 * conversation anybody would mint, so it can carry this meaning and no other. The hub itself WOULD
 * address a conversation called `-`; this reader takes the name for its own use before the hub ever
 * sees it, which `docs/ATTACHING.md` §4 says out loud.
 */
const AS_IF_UNSET = '-'

/** The whole namespace, so that "un-inherit all of them" is one list rather than a habit. */
const NAMESPACE = [
  'KICKOFF_HUB_PROJECT_DIR',
  'KICKOFF_HUB_ADDRESS',
  'KICKOFF_HUB_CONVERSATION',
  'KICKOFF_HUB_TOKEN_FILE',
  'KICKOFF_HUB_SOCKET',
  'KICKOFF_HUB_RELAY',
  'KICKOFF_HUB_RELAY_SOCKET',
  'KICKOFF_HUB_RELAY_DIR',
  'KICKOFF_HUB_RELAY_GRACE_MS',
] as const

/** Everything one adapter needs in order to attach, worked out once from the environment. */
export type Attachment = {
  /** The directory this adapter speaks for. Absolute, and never guessed from cwd. */
  projectDir: string
  /** What git says about that directory: the working tree, the main working tree, the worktree name. */
  facts: Facts
  /** The conversation, as it goes on the wire. Null is the project speaking for itself. */
  address: string | null
  /** Whether a dispatcher named the address, or this adapter fell back to a default it derived. */
  addressWasGiven: boolean
  /** Whether this adapter hands its frames to a relay instead of holding the claim itself. */
  viaRelay: boolean
  /** What to dial: the hub, or the relay standing in for it. */
  dial: string
  /** The hub's own socket, whoever ends up dialling it. */
  hubSocket: string
  /** Where a relay for this conversation listens — null when it cannot be worked out. */
  relaySocket: string | null
  /** Whether `relaySocket` was TOLD (`KICKOFF_HUB_RELAY_SOCKET`) rather than worked out from git. */
  relaySocketWasGiven: boolean
  /**
   * The directory a derived relay socket lives under.
   *
   * Exposed so that a caller asking "which door would a tool server derive here?" asks with the
   * same directory this reader used, rather than with a second copy of the default — which is how
   * a check comes to name a door nothing derives.
   */
  relayDir: string
  /** The secret's path when something named it, or null to search for it. */
  tokenFile: string | null
  /**
   * The conversation a dispatcher said this is (`KICKOFF_HUB_CONVERSATION`), or null. The first
   * term of the ladder, and the one that never falls through.
   */
  conversation: string | null
  /**
   * Where the channel keeps every conversation's secret — the hub's own state directory, derived
   * the way the hub derives it. Null on a box with neither `XDG_STATE_HOME` nor `HOME`, where
   * only a told path or the legacy walk can find a secret.
   */
  channelHome: string | null
  /**
   * Which conversation the relay's door is keyed on: the one told, else the one the repo's link
   * names, else the id the registry's own formula mints for the main tree. Null when nothing can
   * say — no conversation told and no git.
   */
  conversationKey: string | null
  /** How long a relay gives a vanished producer to come home. */
  relayGraceMs: number
}

/**
 * Why this adapter cannot attach at all, in two registers.
 *
 * `why` is what an agent reads in a tool result — plain words plus the one name whoever configured
 * this has to change. `note` is the developer's line on stderr. Both exist because the original
 * defect of this whole system was a bridge that said why on a stream nobody reads.
 */
export type Unattachable = { why: string; note: string }

export type Read = { config: Attachment } | { problem: Unattachable }

const uid = (): number => process.getuid?.() ?? 0

/**
 * What is wrong with an address, in plain words, or null when nothing is.
 *
 * The same five rules `lane_is_addressable` keeps in `hub.rs`, checked HERE so an adapter never
 * learns them from a refusal frame. `bad_lane` is permanent — the same name is refused every time —
 * so an adapter that waited to be told would have spent a claim, a round trip and a reconnect to
 * find out something it could have read off its own configuration. Adopters minting names freely
 * hit the slash rule first: "CEO/steering" is not addressable.
 *
 * A lone `-` passes every rule here, because the hub really would address a conversation called
 * that. It never reaches this function: `readConfig` takes the name for AS_IF_UNSET before any
 * address is worked out, which `docs/ATTACHING.md` §4 says out loud rather than leaving a stranger
 * to discover that one name behaves differently from the shape rules it just read.
 */
export function addressProblem(address: string): string | null {
  if (!address.length) return 'it is empty'
  const bytes = Buffer.byteLength(address, 'utf8')
  if (bytes > MAX_ADDRESS_BYTES) {
    return `it is ${bytes} bytes long and a conversation name can be at most ${MAX_ADDRESS_BYTES}`
  }
  if (address === '.' || address === '..') return 'it names a folder rather than a conversation'
  if (address.includes('/') || address.includes('\\')) {
    return 'it has a slash in it, and a conversation name cannot contain one'
  }
  // A tab or a newline forges a line in the audit file, which is one tab-separated record per line
  // and interpolates its subject exactly as given. Any other control character reaches a topic
  // title and the journal, where it is invisible and can reorder what a person reads.
  if (/[\u0000-\u001f\u007f-\u009f]/.test(address)) {
    return 'it has a character in it that cannot be printed'
  }
  return null
}

/**
 * The socket for the relay that speaks for one conversation.
 *
 * Hashed rather than spelled out because `sun_path` caps at 108 bytes and a repo path plus a name
 * goes past that easily — a limit this project has already been bitten by. The two facts are joined
 * by a NUL, which cannot occur in either, so no pair of (repo, address) can be spelled two ways and
 * land on one socket.
 *
 * Keyed on the CONVERSATION, never on the repository: two rooms in one repo are both top-level
 * with no lane, and a door keyed on the repo gave both the same one — the relay re-checked the
 * secret and turned the second away, so the second room's opencode path was dead the first time
 * the feature was used for what it is for. `conversationKeyFor` says which id that is; a
 * container with no git and no conversation told is told the door with `KICKOFF_HUB_RELAY_SOCKET`
 * instead.
 */
export function relaySocketPath(relayDir: string, conversation: string, address: string | null): string {
  const digest = createHash('sha256').update(`${conversation}\0${address ?? ''}`).digest('hex').slice(0, 16)
  return join(relayDir, `${digest}.sock`)
}

/**
 * The conversation the door is keyed on, worked out without reading a secret — so a relay and
 * its producers, sharing one environment and one filesystem, derive one door whether or not
 * either has authenticated yet. Told; else bound by a link on the same walk the secret takes;
 * else the registry's own formula for the folder the legacy walk would find a token in, which is
 * the id its link names once it has one — so a project's door does not move the day its link is
 * written; else that formula for the main tree; else nothing, when there is no git and no link.
 */
export function conversationKeyFor(conversation: string | null, home: string | null, projectDir: string, facts: Facts): string | null {
  if (conversation) return conversation
  // No git and no conversation told is no derived door, whatever links or tokens are here: a wall
  // is told its door or given a private one (§9, §13.3), and a door derived under the host's
  // runtime directory is one nothing in a wall could open.
  if (!facts.mainTop) return null
  const bound = home ? boundConversationFor(home, projectDir, facts) : null
  if (bound) return bound.id
  const tokenAt = legacyTokenFolder(projectDir, facts)
  if (tokenAt) return seedIdOf(tokenAt)
  return seedIdOf(facts.mainTop)
}

/**
 * Read the environment once, and refuse rather than guess.
 *
 * `cwd` is a parameter rather than a call to `process.cwd()` so that the one place a directory may
 * be inferred is visible from the caller. It is NOT evaluated unless a configuration actually asked
 * for it: this product's own unit of work is a `git worktree`, `git worktree remove` under a live
 * session is the ordinary way a working directory disappears, and `process.cwd()` throws when it
 * has. Thrown from a default parameter that runs on every start, that is an MCP server which exits
 * before it can say a word — no refusal, no sentence for the agent, just a stack trace on a stream
 * nobody reads, which is the failure this whole reader exists to stop repeating.
 */
export function readConfig(env0: Record<string, string | undefined> = process.env, cwd?: string): Read {
  const problem = (why: string, note: string): Read => ({ problem: { why, note } })

  // `-` is not a value, it is the absence of one — see AS_IF_UNSET. Stripped here, before anything
  // else looks, so that every rule below sees exactly what it would have seen had the variable
  // never been inherited: the empty-string rule does not fire on it, and a `-` address falls back
  // to the default derived from THIS session's own directory rather than speaking as an outer one.
  const env: Record<string, string | undefined> = { ...env0 }
  for (const name of NAMESPACE) if (env[name] === AS_IF_UNSET) delete env[name]

  // Every variable here has a working default when it is UNSET, so an empty one must be its own
  // refusal rather than a fall-through: a configuration that set it meant to override the default,
  // and the value it meant to pass was not there. Falling through hands back the default the caller
  // was deliberately replacing — a container told exactly where its secret is goes looking for one
  // instead, and attaches with whatever it happens to find.
  //
  // `KICKOFF_HUB_PROJECT_DIR` is in this list even though unset refuses on its own, because unset
  // does NOT refuse on its own: it falls through to the engine's `CLAUDE_PROJECT_DIR` first, and
  // that variable being set is the whole environment the fall-through order exists for. An empty
  // one there is the second incident exactly — another repository's secret, silently, because it
  // really does find one.
  //
  // The one variable NOT in this list is the one where empty genuinely means "no": unset
  // `KICKOFF_HUB_RELAY` means "I hold the claim myself", which is a real answer.
  const blank = [
    'KICKOFF_HUB_PROJECT_DIR',
    'KICKOFF_HUB_ADDRESS',
    'KICKOFF_HUB_CONVERSATION',
    'KICKOFF_HUB_TOKEN_FILE',
    'KICKOFF_HUB_SOCKET',
    'KICKOFF_HUB_RELAY_SOCKET',
    'KICKOFF_HUB_RELAY_DIR',
    'KICKOFF_HUB_RELAY_GRACE_MS',
  ].filter(name => env[name] !== undefined && env[name] === '')
  if (blank.length) {
    const list = blank.join(' and ')
    return problem(
      `Something started this session with ${list} set to nothing at all, which usually means a setting asked for a value that was not there. Nothing here reaches him until ${blank.length === 1 ? 'it is' : 'they are'} given a real value or left out entirely.`,
      `${list} ${blank.length === 1 ? 'is' : 'are'} set to the empty string; a variable set to nothing is not a value`,
    )
  }

  // ── the credential travels as a PATH, never a value ───────────────────────────────────────────
  //
  // The environment is promiscuous: every variable here is inherited by every descendant of every
  // process, across engines and wrappers (§5's two incidents). A secret pasted into a variable is
  // therefore a secret in every child's environment and every crash dump — so there is no variable
  // for the secret's value, and there never will be. `KICKOFF_HUB_TOKEN_FILE` names a FILE, which
  // the filesystem protects at 0600. A promiscuous box can still leave `KICKOFF_HUB_TOKEN` behind,
  // so refuse it loudly rather than ever authenticate with a by-value secret. `-` is the one value
  // that means "as if unset" (AS_IF_UNSET), so it is not a real setting and is not refused.
  const byValue = env0.KICKOFF_HUB_TOKEN
  if (byValue !== undefined && byValue.length && byValue !== AS_IF_UNSET) {
    return problem(
      'The secret for this session was handed to it as a value in KICKOFF_HUB_TOKEN, and the secret must never travel that way — the environment is inherited by every process below this one. Put the secret in a file and name the file with KICKOFF_HUB_TOKEN_FILE.',
      'KICKOFF_HUB_TOKEN is set, and the secret never travels as a value; put it in a file and name the file with KICKOFF_HUB_TOKEN_FILE',
    )
  }

  // ── which directory ─────────────────────────────────────────────────────────────────────────
  //
  // The ORDER is the fix for the second incident in the header, not a preference. A dispatcher's
  // explicit word beats an engine's ambient one: an opencode server started from inside a Claude
  // Code session inherits `CLAUDE_PROJECT_DIR`, its MCP child inherits the server's whole
  // environment, and with the engine's variable on top the child resolved another repository's
  // secret — silently, because it really did find one.
  const given = named(env.KICKOFF_HUB_PROJECT_DIR)
  let projectDir: string | null = null
  if (given === '.') {
    // The one spelling that means "the folder I was started in, and whoever wrote this vouches for
    // it". A bare fallback to cwd is what this refuses: cwd is the PLUGIN folder under Claude Code
    // in both layouts, and guessing it is the defect that made every message an agent believed it
    // had sent go nowhere. A literal dot is a claim somebody made in a file; it also cannot be
    // produced by opencode substituting a variable that was not there.
    //
    // Asked for here and nowhere else, and a failure becomes words rather than a stack trace: the
    // folder a session was started in can be deleted while the session is still running, which for
    // a repo whose unit of work is a `git worktree` is an ordinary Tuesday.
    let here: string | null = cwd ?? null
    if (here === null) {
      try {
        here = process.cwd()
      } catch {
        here = null
      }
    }
    if (here === null) {
      return problem(
        'This session was told its project is the folder it was started in, and that folder is no longer there. Nothing here reaches him until the session is started somewhere that exists, or until whoever starts it names the project folder outright with KICKOFF_HUB_PROJECT_DIR.',
        'KICKOFF_HUB_PROJECT_DIR is "." and the working directory this process was started in has been removed',
      )
    }
    projectDir = here
  } else if (given) {
    if (!isAbsolute(given)) {
      return problem(
        `This session was pointed at "${given}", which is neither a full path nor ".", so the bridge cannot tell which folder was meant. Set KICKOFF_HUB_PROJECT_DIR to the project folder, or to "." when the session starts in it.`,
        `KICKOFF_HUB_PROJECT_DIR is "${given}", which is neither absolute nor "."`,
      )
    }
    projectDir = given
  } else {
    const engines = named(env.CLAUDE_PROJECT_DIR)
    if (engines && !isAbsolute(engines)) {
      return problem(
        `The engine said this session lives in "${engines}", which is not a full path, so the bridge cannot tell which folder was meant. Set KICKOFF_HUB_PROJECT_DIR to the project folder.`,
        `CLAUDE_PROJECT_DIR is "${engines}", which is not absolute`,
      )
    }
    projectDir = engines
  }
  if (!projectDir) {
    return problem(
      'This session never said which project it belongs to, so the bridge cannot find the secret that proves who it is. Whoever starts it has to set KICKOFF_HUB_PROJECT_DIR to the project folder, or to "." when the session already starts in it.',
      'nothing named a project directory (KICKOFF_HUB_PROJECT_DIR), so there is no way to reach the operator from here',
    )
  }

  const facts = factsFor(projectDir)

  // ── which conversation ──────────────────────────────────────────────────────────────────────
  const wanted = env.KICKOFF_HUB_ADDRESS
  let address: string | null = null
  let addressWasGiven = false
  if (wanted !== undefined) {
    // This is the case the empty-string rule above exists for: the default is "speak as the
    // project", so quietly taking it for a dispatcher whose variable failed to expand puts one
    // conversation's words in another's topic and takes the project's claim with them. Empty is
    // caught up there; what is left here is a name that is real and still unaddressable.
    const wrong = addressProblem(wanted)
    if (wrong) {
      return problem(
        `The name this session was given for its conversation ("${wanted}") cannot be used, because ${wrong}. Nothing here reaches him until whoever set KICKOFF_HUB_ADDRESS gives it a different one.`,
        `KICKOFF_HUB_ADDRESS is "${wanted}", which cannot be addressed: ${wrong}`,
      )
    }
    address = wanted
    addressWasGiven = true
  } else if (facts.lane) {
    // The default for a session nobody dispatched — a developer opening one by hand. git's own name
    // for the worktree, which git guarantees unique across a repository, never the checkout's
    // folder name, which is not: `~/a/wip` and `~/b/wip` are two trees that presented one name and
    // therefore resolved to ONE conversation at the hub.
    const wrong = addressProblem(facts.lane)
    if (wrong) {
      return problem(
        `This worktree's own name ("${facts.lane}") cannot be used as a conversation name, because ${wrong}. Nothing from here reaches him until it is remade under a plainer name, or until whoever starts it names the conversation with KICKOFF_HUB_ADDRESS.`,
        `the derived address "${facts.lane}" cannot be addressed: ${wrong}`,
      )
    }
    address = facts.lane
  }

  // ── which conversation, when a dispatcher said so ───────────────────────────────────────────
  //
  // An id becomes a path segment under the channel's home, so it is refused at THIS door — before
  // a path is joined, before anything is dialled — on the exact shape the registry mints. Set and
  // unreadable is a permanent refusal later, in `secretFor`, never a fall-through: a fall-through
  // under a failed bind-mount is a session speaking as the wrong conversation.
  const conversation = named(env.KICKOFF_HUB_CONVERSATION)
  if (conversation && !isConversationId(conversation)) {
    return problem(
      `This session was told it is conversation "${conversation}", which is not the shape a conversation's id has, so the bridge cannot look it up. Whoever set KICKOFF_HUB_CONVERSATION has to give it the id the terminal printed.`,
      `KICKOFF_HUB_CONVERSATION is "${conversation}", which is not a conversation id`,
    )
  }
  // The channel's home is read off the process's own environment the way its uid is — these are
  // the operating system's variables, not this namespace's — and it is null only on a box that
  // has neither, where a told path or the legacy walk are the only ways left to a secret.
  const home = channelHome(env0.XDG_STATE_HOME, env0.HOME)

  // ── the secret ──────────────────────────────────────────────────────────────────────────────
  const tokenFile = named(env.KICKOFF_HUB_TOKEN_FILE)
  // Two answers to one question. A told path is used verbatim (§5, the container answer) and a
  // told conversation is read from the channel's home; with both set, one would have to outrank
  // the other in silence, and silence about which secret a session presents is the one thing
  // this reader exists to refuse.
  if (tokenFile && conversation) {
    return problem(
      'This session was given both a path to its secret (KICKOFF_HUB_TOKEN_FILE) and a conversation to be (KICKOFF_HUB_CONVERSATION), which are two answers to one question. Whoever started it has to keep one and drop the other.',
      'both KICKOFF_HUB_TOKEN_FILE and KICKOFF_HUB_CONVERSATION are set; keep one',
    )
  }
  // A path, not the secret. 64 hex characters is what a token IS, and a token in the variable that
  // names the token's file is the by-value credential wearing the path variable's name — caught
  // with its own sentence rather than the generic "not a full path" below, because the fix is
  // different: this one is not a mistyped path, it is a secret that must go in a file.
  if (tokenFile && /^[0-9a-f]{64}$/i.test(tokenFile)) {
    return problem(
      'KICKOFF_HUB_TOKEN_FILE was set to what looks like the secret itself rather than the path to the file that holds it. It takes the path to the file; the secret never travels as a value.',
      'KICKOFF_HUB_TOKEN_FILE looks like the secret itself; it takes the path to the file',
    )
  }
  if (tokenFile && !isAbsolute(tokenFile)) {
    return problem(
      `The secret was said to be at "${tokenFile}", which is not a full path, so the bridge cannot find it. Set KICKOFF_HUB_TOKEN_FILE to the whole path.`,
      `KICKOFF_HUB_TOKEN_FILE is "${tokenFile}", which is not absolute`,
    )
  }

  // ── where to dial ───────────────────────────────────────────────────────────────────────────
  //
  // A relative path here is not resolved, it is refused, for the same reason a relative project
  // directory is: it would be worked out against whichever folder this process happens to be
  // sitting in, which under a Claude Code plugin manifest is the plugin folder — the exact
  // directory this interface exists to stop guessing from. Nothing listens at the answer, so the
  // link reports itself as temporarily down and every message the agent sends is "waiting in line"
  // for ever, with a bare filename on stderr as the only clue.
  for (const name of ['KICKOFF_HUB_SOCKET', 'KICKOFF_HUB_RELAY_SOCKET', 'KICKOFF_HUB_RELAY_DIR'] as const) {
    const where = named(env[name])
    if (where && !isAbsolute(where)) {
      return problem(
        `This session was pointed at "${where}" for the way to his phone, which is not a full path, so the bridge cannot tell where that is. Set ${name} to the whole path.`,
        `${name} is "${where}", which is not absolute`,
      )
    }
  }

  // Derived rather than read from `XDG_RUNTIME_DIR`, which does not survive an `env -i` boundary:
  // the two sides would then derive different paths with neither being wrong.
  const hubSocket = named(env.KICKOFF_HUB_SOCKET) ?? `/run/user/${uid()}/kickoff/hub.sock`
  const relayDir = named(env.KICKOFF_HUB_RELAY_DIR) ?? `/run/user/${uid()}/kickoff/fanin`
  const toldSocket = named(env.KICKOFF_HUB_RELAY_SOCKET)
  const conversationKey = conversationKeyFor(conversation, home, projectDir, facts)
  const relaySocket = toldSocket ?? (conversationKey ? relaySocketPath(relayDir, conversationKey, address) : null)

  const relayFlag = env.KICKOFF_HUB_RELAY
  if (relayFlag !== undefined && relayFlag.length && relayFlag !== '1') {
    return problem(
      `This session was told to reach him through a relay by a setting that says "${relayFlag}", and the only value that means yes is 1. Nothing here reaches him until KICKOFF_HUB_RELAY is 1 or unset.`,
      `KICKOFF_HUB_RELAY is "${relayFlag}"; the only value that turns it on is 1`,
    )
  }
  const viaRelay = relayFlag === '1'
  if (viaRelay && !relaySocket) {
    // Falling back to the hub's own socket here would put this adapter and whatever else speaks for
    // this conversation in a race for one claim — the entire thing the claim exists to prevent.
    return problem(
      'This session was told to reach him through a relay, and the bridge cannot work out where that relay is because this folder is not inside a repository and no conversation was named. Name the relay with KICKOFF_HUB_RELAY_SOCKET, name the conversation with KICKOFF_HUB_CONVERSATION, or start the session inside the project.',
      'a relay was asked for, and neither KICKOFF_HUB_RELAY_SOCKET, KICKOFF_HUB_CONVERSATION nor a repository says where it is',
    )
  }

  // ── the relay's grace ───────────────────────────────────────────────────────────────────────
  //
  // Fail-closed on purpose: `Number("")` is 0 and `Number("x")` is NaN, so the old parse turned a
  // garbled value into either withdrawing every question the instant a producer blinked, or never
  // withdrawing any at all — and said nothing either way.
  const graceGiven = env.KICKOFF_HUB_RELAY_GRACE_MS
  let relayGraceMs = 90_000
  if (graceGiven !== undefined && graceGiven.length) {
    if (!/^[0-9]+$/.test(graceGiven) || Number(graceGiven) <= 0) {
      return problem(
        `The time a departed session is given to come back was set to "${graceGiven}", which is not a number of milliseconds. Nothing here runs until KICKOFF_HUB_RELAY_GRACE_MS is a whole number greater than zero.`,
        `KICKOFF_HUB_RELAY_GRACE_MS is "${graceGiven}", which is not a positive whole number`,
      )
    }
    relayGraceMs = Number(graceGiven)
  }

  return {
    config: {
      projectDir,
      facts,
      address,
      addressWasGiven,
      viaRelay,
      dial: viaRelay ? relaySocket! : hubSocket,
      hubSocket,
      relaySocket,
      relaySocketWasGiven: toldSocket !== null,
      relayDir,
      tokenFile,
      conversation,
      channelHome: home,
      conversationKey,
      relayGraceMs,
    },
  }
}

/**
 * The door a tool server that works its own out from git would look for, in this directory — or
 * null when git says nothing here.
 *
 * With the address it would derive, `facts.lane`, and NOT the address this configuration holds: a
 * dispatcher can mint an address that is not git's name, and then the two doors differ. That is the
 * split-brain nobody would diagnose in under an hour — the prompts reach the phone and the agent's
 * own `reply` says "not said yet" for ever — so both `--check` and attach's own start-up ask this
 * one function, and cannot disagree about the answer.
 *
 * The conversation gets the same treatment, with one difference: under `--run` the child IS handed
 * the conversation attach was told (§13.3 pins it, and the overlay of §2 never blanks it), so a
 * tool server there derives from it; hand-started beside a bare attach it has only git, which
 * names the folder's own conversation and never a room. The first version keyed this on the told
 * conversation in both shapes, and so compared attach's door with itself.
 */
export function doorDerivedFromGit(c: Attachment, childKeepsConversation: boolean): string | null {
  const key = conversationKeyFor(childKeepsConversation ? c.conversation : null, c.channelHome, c.projectDir, c.facts)
  return key ? relaySocketPath(c.relayDir, key, c.facts.lane) : null
}

/**
 * The conversation this adapter proves itself as — resolved AFRESH on every attempt.
 *
 * Never resolved once: the operator may run `herdr-tg open` or `enroll` while the adapter is
 * running, and that is the documented recovery from "this project is not enrolled". An adapter
 * that cached the absence of a secret would make that recovery a lie.
 *
 * Four terms, strict order — the ladder of `docs/ATTACHING.md` §5:
 *
 *   1. **Told** — a path (`KICKOFF_HUB_TOKEN_FILE`), used verbatim with no search: the container
 *      answer, and the only way to attach from a machine where git is not a fact; or a
 *      conversation (`KICKOFF_HUB_CONVERSATION`), whose secret is read from the channel's home.
 *      Both at once is refused as two answers to one question. Told and unreadable is a REFUSAL,
 *      never a fall-through — the repo's link and the repo's token may both be right there, and
 *      either would attach this session as a conversation nobody opened for it.
 *   2. **Bound**: a link the channel wrote, `by-repo/<hash>`, names the conversation a folder
 *      defaults to — looked for on exactly the walk term 3 takes, so it is found wherever a
 *      token could have been. A lane worktree crosses to the same main tree, so a lane and its
 *      main tree read one credential by construction rather than by a special case.
 *   3. **Legacy**: the upward walk to `<repo>/.kickoff/hub.token`, kept for the whole migration so
 *      a bridge from before conversations existed and one from after both work.
 *   4. **Nothing.** The caller refuses, naming a verb and never a path (`notEnrolled`).
 */
export function secretFor(c: Attachment): Project | null {
  if (c.tokenFile) {
    const token = readSecret(c.tokenFile)
    return token ? { repo: c.projectDir, tokenFile: c.tokenFile, token, how: 'told' } : null
  }
  if (c.conversation) {
    const found = c.channelHome ? conversationSecret(c.channelHome, c.conversation) : null
    return found ? { repo: c.projectDir, ...found, conversation: c.conversation, how: 'named' } : null
  }
  if (c.channelHome) {
    const bound = boundConversationFor(c.channelHome, c.projectDir, c.facts)
    const found = bound ? conversationSecret(c.channelHome, bound.id) : null
    if (bound && found) return { repo: bound.repo, ...found, conversation: bound.id, how: 'bound' }
  }
  return findProject(c.projectDir, c.facts)
}

/**
 * What an agent, or a person, reads when `secretFor` found nothing — in the two registers every
 * refusal here has. It names a VERB and never a path: the hint used to carry a folder, and in a
 * lane worktree that folder was the worktree, so following it minted a second project for the
 * same repository and moved a live conversation somewhere new on his phone. The person at the
 * keyboard knows which folder; the agent does not get to guess.
 */
export function notEnrolled(c: Attachment): Unattachable {
  if (c.tokenFile) {
    return {
      why: 'The secret this session was pointed at is not there or cannot be read, so the hub has no way to know which project it is. Mount the secret where KICKOFF_HUB_TOKEN_FILE says, or open the project at a terminal:  herdr-tg open',
      note: `no secret at ${c.tokenFile} (KICKOFF_HUB_TOKEN_FILE); mount it there, or at a terminal: herdr-tg open`,
    }
  }
  if (c.conversation) {
    return {
      why: `This session was told it is conversation ${c.conversation}, and the channel holds no secret for that conversation, so the hub has no way to know which conversation it is. Nothing here reaches him until that conversation is opened at a terminal — herdr-tg open for a project, herdr-tg grant for a room — or KICKOFF_HUB_CONVERSATION names one that is.`,
      note: `no secret for conversation ${c.conversation}${c.channelHome ? '' : ', and no channel home on this box'}; open it at a terminal (herdr-tg open / herdr-tg grant), or fix KICKOFF_HUB_CONVERSATION`,
    }
  }
  return {
    why: 'This project is not enrolled, so the hub has no way to know which project it is. Open it at a terminal:  herdr-tg open <the project folder>',
    note: 'no secret for this project; open it at a terminal with herdr-tg open',
  }
}
