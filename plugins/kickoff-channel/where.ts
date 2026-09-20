/**
 * Which repository, which worktree, and which socket — the machine facts every part of this
 * adapter has to agree on, derived in ONE place.
 *
 * Three processes now ask the same questions about the same directory — the tool server, the relay
 * and the opencode bridge — and a second copy of this arithmetic is a set of processes that
 * disagree about which conversation they are while every one of them looks right.
 *
 * Nothing here names an engine, and nothing here reads the environment: which directory to ask
 * about, and what to call the conversation, are `attach.ts`'s job. This file answers only what the
 * MACHINE says once a directory has been named.
 */

import { createHash } from 'crypto'
import { readFileSync, existsSync, lstatSync, realpathSync } from 'fs'
import { dirname, join, resolve } from 'path'

/** Ask git one question about a directory, or null when it will not answer. */
export function gitDir(from: string, flag: string): string | null {
  try {
    const git = Bun.spawnSync(['git', '-C', from, 'rev-parse', flag], {
      stdout: 'pipe',
      stderr: 'ignore',
      // `--git-common-dir` answers RELATIVELY in the main worktree ('.git') and absolutely in a
      // linked one, so it is resolved against the directory it was asked about and not against
      // this process's cwd — which under a plugin manifest is the plugin folder, nowhere near
      // either.
      cwd: from,
    })
    const out = new TextDecoder().decode(git.stdout).trim()
    return git.exitCode === 0 && out.length ? resolve(from, out) : null
  } catch {
    // git missing, or a tree it will not talk about. The boundary is unknown, and an unknown
    // boundary means the search stays where it started rather than inventing one.
    return null
  }
}

/** What a directory turns out to be, once git has been asked about it. */
export type Facts = {
  /**
   * The top of the working tree the session was launched in, or null when git will not say.
   *
   * This is the one boundary the secret search may cross, and it is what keeps that search from
   * being directory-guessing: it never leaves the repository the session was started in.
   */
  projectTop: string | null
  /**
   * The MAIN working tree of this repository, when it is not the one the session started in.
   *
   * kickoff runs its lanes in `git worktree` checkouts, and a lane worktree has NO
   * `.kickoff/hub.token`: the secret is gitignored, so it is never checked out into one. The
   * search stopped at the worktree, found nothing, and every lane failed closed with "this project
   * is not enrolled" — which made a topic per lane unreachable from a real lane.
   *
   * `--git-common-dir` is the machine-derived fact that joins them. In a linked worktree it is an
   * absolute path to the MAIN repo's `.git`; in the main worktree it is `.git` itself. Its parent
   * is the main working tree either way.
   */
  mainTop: string | null
  /**
   * Which worktree this is, when it is not the main one — the lane's own name.
   *
   * **git's own name for the worktree, not the folder it is checked out in.** In a linked worktree
   * `--git-dir` is `<main>/.git/worktrees/<name>`, and git guarantees that `<name>` is unique
   * across the repository: adding a second worktree whose folder is also called `wip` gives it
   * `wip1`.
   *
   * The folder's basename is what this read first, and it is not unique. git dedupes only its own
   * internal name, never the checkout path, so `~/a/wip` and `~/b/wip` are both legal — two
   * different trees with two different agents, presenting one name and therefore resolving to ONE
   * conversation at the hub. The second one's arrival then evicts the first's claim and sweeps its
   * still-open questions off the operator's phone as "the session that asked this restarted".
   * Neither tree restarted.
   */
  lane: string | null
}

/** Read repo and lane off the machine, for a directory something was launched in. */
export function factsFor(launchedIn: string | null): Facts {
  if (!launchedIn) return { projectTop: null, mainTop: null, lane: null }
  // Both through every link before they are compared. git answers `--show-toplevel` with the REAL
  // path and `--git-common-dir` relative to the directory it was asked about, so a main tree
  // reached through a symlinked parent compared unequal to itself and was taken for a linked
  // worktree — with a lane named after the link, a door of its own, and a topic of its own.
  const top = gitDir(launchedIn, '--show-toplevel')
  const projectTop = top ? realPath(top) : null
  const common = gitDir(launchedIn, '--git-common-dir')
  const mainTop = common ? realPath(dirname(common)) : null
  let lane: string | null = null
  if (projectTop && mainTop && projectTop !== mainTop) {
    const own = gitDir(projectTop, '--git-dir')
    const name = (own ?? projectTop).split('/').pop()
    lane = name && name.length ? name : null
  }
  return { projectTop, mainTop, lane }
}

/** The enrolled project a directory belongs to, or null when it is not inside one. */
export type Project = {
  repo: string
  tokenFile: string
  token: string
  /** The conversation the secret was read for, when it came from the channel's home. */
  conversation?: string
  /**
   * Which term of the ladder found it: told a path, named a conversation, bound by the repo's
   * own link, or the legacy walk to the repo's token. For a line a person reads; nothing routes
   * on it.
   */
  how?: 'told' | 'named' | 'bound' | 'legacy'
}

/**
 * The shape a conversation id has — `p-` and twelve hex characters for a project minted from its
 * repo path, `c-` and twelve for a room — and the ONLY string this file joins onto a path. An id
 * becomes a directory name under the channel's home, so anything else is refused before a path
 * exists: not a dot, not a slash, not an uppercase digit.
 */
export const CONVERSATION_ID = /^[pc]-[0-9a-f]{12}$/
export const isConversationId = (s: string): boolean => CONVERSATION_ID.test(s)

/**
 * The channel's home: the hub's own state directory, `$XDG_STATE_HOME/herdr-tg` or
 * `$HOME/.local/state/herdr-tg` — the same derivation the hub makes, so the two cannot disagree
 * about where a conversation's secret is. One home, not two.
 */
export function channelHome(xdgStateHome: string | undefined, home: string | undefined): string | null {
  if (xdgStateHome && xdgStateHome.length) return join(xdgStateHome, 'herdr-tg')
  if (home && home.length) return join(home, '.local', 'state', 'herdr-tg')
  return null
}

/** The `by-repo` key for a repository: sixteen hex characters of the SHA-256 of its real path. */
function repoKey(mainTop: string): string {
  return createHash('sha256').update(realPath(mainTop)).digest('hex').slice(0, 16)
}

/**
 * The key of the DOOR for a repository that has not been told which conversation it is: `p-` and
 * twelve hex characters of the same hash the link is keyed on.
 *
 * Named for what it is used for, and not for what it happens to equal. It is byte for byte the id
 * the hub's own registry mints for a seed, which is the whole reason it works — a relay and its
 * producers derive the same door from the same directory whether or not either has read a secret
 * yet, and the door does not move the day the link is finally written. But it is NOT a statement
 * that this directory IS that seed: a room, a moved checkout or a re-enrolment can all make the
 * hub's answer differ, and only the hub's answer names a conversation. Read the id off the
 * `welcome`; use this to find a socket.
 */
export function doorKeyFor(mainTop: string): string {
  return `p-${createHash('sha256').update(realPath(mainTop)).digest('hex').slice(0, 12)}`
}

/**
 * The real path, through every link — because the hub hashes the CANONICAL path and a checkout
 * reached through a symlinked parent would otherwise miss its own link and fall to the legacy
 * walk (fails closed: the same project or nothing, never another). The string as given when the
 * filesystem will not say.
 */
function realPath(p: string): string {
  try {
    return realpathSync(p)
  } catch {
    return p
  }
}

/**
 * The conversation ONE directory is bound to, by the link the channel wrote — or null when there
 * is no link, or the link names something that is not a conversation id.
 */
export function boundConversation(home: string, dir: string): string | null {
  const id = readSecret(join(home, 'by-repo', repoKey(dir)))
  return id && isConversationId(id) ? id : null
}

/**
 * The conversation a launch directory is bound to, and the folder whose link named it.
 *
 * A link is keyed on the folder the operator OPENED, and that is routinely not the top of the
 * repository: a folder with no git at all, or a project opened below the top of a monorepo. The
 * first version looked the link up by git's main working tree alone, so neither was ever found —
 * while `open` had just told the operator a session there would find it on its own — and a
 * project that had its repo copy taken away would have gone off the air with no sentence saying
 * why. So this walks exactly where the legacy walk to `.kickoff/hub.token` went, and finds a link
 * wherever a token could have been: upward from the launch directory to the top of the working
 * tree, then the one legal crossing to the main working tree, and with no git the named directory
 * alone. Two sibling projects opened in one repository stay distinct, because the walk stops at
 * the first link it meets.
 */
export function boundConversationFor(home: string, launchedIn: string, facts: Facts): { id: string; repo: string } | null {
  let dir = realPath(resolve(launchedIn))
  const top = facts.projectTop
  for (;;) {
    const id = boundConversation(home, dir)
    if (id) return { id, repo: dir }
    if (!top || dir === top) break
    const up = dirname(dir)
    if (up === dir) break
    dir = up
  }
  if (facts.mainTop && facts.mainTop !== top) {
    const id = boundConversation(home, facts.mainTop)
    if (id) return { id, repo: facts.mainTop }
  }
  return null
}

/**
 * The folder the legacy walk would find a token in, WITHOUT reading it — for keying a door on the
 * project the registry minted for that folder before its link exists. Null when no token is on the
 * walk.
 */
export function legacyTokenFolder(launchedIn: string, facts: Facts): string | null {
  let dir = realPath(resolve(launchedIn))
  const top = facts.projectTop
  for (;;) {
    if (existsSync(join(dir, '.kickoff', 'hub.token'))) return dir
    if (!top || dir === top) break
    const up = dirname(dir)
    if (up === dir) break
    dir = up
  }
  if (facts.mainTop && facts.mainTop !== top && existsSync(join(facts.mainTop, '.kickoff', 'hub.token'))) {
    return facts.mainTop
  }
  return null
}

/**
 * The secret of one conversation, read from the channel's home — or null when there is none
 * readable there. The id is shape-checked HERE, immediately before it becomes a path segment,
 * and the file is refused when it is a link: a credential read through a link out of the tree is
 * one nobody minted there.
 */
export function conversationSecret(home: string, id: string): { tokenFile: string; token: string } | null {
  if (!isConversationId(id)) return null
  const tokenFile = join(home, 'conversations', id, 'secret')
  try {
    if (lstatSync(tokenFile).isSymbolicLink()) return null
  } catch {
    return null
  }
  const token = readSecret(tokenFile)
  return token ? { tokenFile, token } : null
}

/**
 * Find the enrolled project this session is inside.
 *
 * Looked up afresh on every attempt, never resolved once: the operator may run `kickoff-channel enroll`
 * while the session is running, and that is the recovery a tool result tells him to perform.
 *
 * The search goes UPWARD from the launch directory, because the directory an engine names is
 * routinely a subfolder of the repo rather than its top. Joining `.kickoff` onto it
 * and stopping there was the original defect with a new wrong directory in it: a session started in
 * `crates/` looked for a secret nobody had enrolled, and the operator's phone stayed just as
 * silent. It stops at the top of the working tree, so it can never wander into someone else's.
 */
export function findProject(launchedIn: string | null, facts: Facts): Project | null {
  const { projectTop: PROJECT_TOP, mainTop: MAIN_TOP } = facts
  if (!launchedIn) return null
  const found = searchUpward(resolve(launchedIn), PROJECT_TOP)
  if (found) return found
  // A lane worktree holds no secret of its own, because the secret is gitignored and never checked
  // out into one. Its project is the MAIN working tree of the same repository — one repo, named by
  // git — and this is the only boundary the search may cross.
  if (MAIN_TOP && MAIN_TOP !== PROJECT_TOP) return searchUpward(MAIN_TOP, MAIN_TOP)
  return null
}

function searchUpward(from: string, top: string | null): Project | null {
  let dir = from
  for (;;) {
    const tokenFile = join(dir, '.kickoff', 'hub.token')
    const token = readSecret(tokenFile)
    if (token) return { repo: dir, tokenFile, token, how: 'legacy' }
    if (!top || dir === top) return null
    const up = dirname(dir)
    if (up === dir) return null
    dir = up
  }
}

/**
 * The secret in a file, or null when there is not one there.
 *
 * Exported because an adapter may be TOLD where its secret is rather than searching for it — the
 * container case, and the only way to attach from a machine where git is not a fact.
 */
export function readSecret(file: string): string | null {
  try {
    if (!existsSync(file)) return null
    const t = readFileSync(file, 'utf8').trim()
    return t.length ? t : null
  } catch {
    return null
  }
}
