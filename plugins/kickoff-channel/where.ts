/**
 * Which repository, which worktree, and which socket — the machine facts every part of this
 * adapter has to agree on, derived in ONE place.
 *
 * Two processes now derive the same address from the same directory: the tool server, which dials
 * it, and the fan-in, which listens on it. A second copy of this arithmetic is a pair of processes
 * that disagree about where to meet and fail by both being silently right — the failure this
 * project already had with `XDG_RUNTIME_DIR`, which does not survive an `env -i` boundary.
 *
 * Nothing here names an engine. The tool server is started by Claude Code and by opencode, and both
 * of them get repo and lane out of a directory and nothing else.
 */

import { createHash } from 'crypto'
import { readFileSync, existsSync } from 'fs'
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
  const projectTop = gitDir(launchedIn, '--show-toplevel')
  const common = gitDir(launchedIn, '--git-common-dir')
  const mainTop = common ? dirname(common) : null
  let lane: string | null = null
  if (projectTop && mainTop && projectTop !== mainTop) {
    const own = gitDir(projectTop, '--git-dir')
    const name = (own ?? projectTop).split('/').pop()
    lane = name && name.length ? name : null
  }
  return { projectTop, mainTop, lane }
}

const uid = () => process.getuid?.() ?? 0

/** `/run/user/<uid>/kickoff/hub.sock`, derived and never configured. */
export const hubSocket = (): string =>
  process.env.KICKOFF_HUB_SOCKET ?? `/run/user/${uid()}/kickoff/hub.sock`

/** Where the per-address fan-in sockets live. Overridable ONLY so a test can own a directory. */
export const faninDir = (): string =>
  process.env.KICKOFF_FANIN_DIR ?? `/run/user/${uid()}/kickoff/fanin`

/**
 * The socket for the fan-in that speaks for one addressable thing.
 *
 * Hashed rather than spelled out because `sun_path` caps at 108 bytes, and a repo path plus a lane
 * name goes past that easily — a limit this project has already been bitten by. Both sides derive
 * it from the same two machine facts they already compute, so neither has to be configured with
 * the other's answer.
 *
 * The two facts are joined by a NUL, which cannot occur in either, so no pair of (repo, lane) can
 * be spelled two ways and land on one socket.
 */
export function faninSocket(mainTop: string, lane: string | null): string {
  const digest = createHash('sha256').update(`${mainTop}\0${lane ?? ''}`).digest('hex').slice(0, 16)
  return join(faninDir(), `${digest}.sock`)
}

/** The enrolled project a directory belongs to, or null when it is not inside one. */
export type Project = { repo: string; tokenFile: string; token: string }

/**
 * Find the enrolled project this session is inside.
 *
 * Looked up afresh on every attempt, never resolved once: the operator may run `herdr-tg enroll`
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
    if (token) return { repo: dir, tokenFile, token }
    if (!top || dir === top) return null
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
