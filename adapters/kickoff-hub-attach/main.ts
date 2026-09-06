#!/usr/bin/env bun
/**
 * kickoff-hub-attach — ONE process, one command, that puts a worker on the operator's phone.
 *
 *     kickoff-hub-attach [--opencode <url>] [--run <command...>]
 *     kickoff-hub-attach --check [--opencode <url>] [--run <command...>]
 *
 * A Claude worker was always one command; an opencode worker used to be three hand-started
 * processes for one conversation — the relay that holds the slot, `opencode serve`, and the event
 * bridge that relays its prompts — and when one died the others hung. This command is all three
 * jobs, and it can start the engine as its child so a wall (bwrap or docker) has one entrypoint.
 *
 *   * it reads the namespace of `docs/ATTACHING.md` §2 through the one reader, `attach.ts`;
 *   * it holds the claim for `(project, address)` and opens the door — `relay.ts`;
 *   * with `--opencode`, it watches that server and relays its prompts — `opencode.ts`, attached to
 *     its own door as an in-process producer;
 *   * with `--run`, it starts the engine as its child so it can be a container's entrypoint —
 *     `run.ts`, the ONE place anything is spawned;
 *   * with `--check`, it proves the environment can reach the hub and creates no topic — `check.ts`.
 *
 * Which project, which conversation, where the secret is and what to dial come from the environment
 * and ONLY from there. There is no project flag: it is `KICKOFF_HUB_PROJECT_DIR`, and `.` means
 * "the directory I was started in, and whoever typed this vouches for it".
 */

import { chmodSync, mkdtempSync, rmSync } from 'fs'
import { join } from 'path'

import { notEnrolled, readConfig, secretFor } from '../../plugins/kickoff-channel/attach.ts'
import { createRelay } from './relay.ts'
import { runCheck } from './check.ts'
import { runChild } from './run.ts'
import { startWatcher } from './opencode.ts'
import { PRIVATE_DOOR_PREFIX, opencodeUrlProblem, privateDoorPlan, producerFlagProblem, toolServerFact, typedWordsFact } from './plan.ts'

/** Say something in this process's own transcript, prefixed so a journal tells it from the child's. */
function note(msg: string): void {
  process.stderr.write(`kickoff-hub-attach: ${msg}\n`)
}

/** Refuse to start. Exit 2 — the same 2 the relay and the bridge used, naming the fix on stderr. */
function die(msg: string): never {
  note(msg)
  process.exit(2)
}

type Args = { check: boolean; opencode: string | null; run: string[] | null }

/**
 * Parse the command line. Everything after `--run` is the command — nothing after it is read as a
 * flag, so the engine can take flags of its own (`serve --port 9711`) without them reaching here.
 */
function parseArgs(argv: string[]): Args {
  const a: Args = { check: false, opencode: null, run: null }
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === '--run') {
      a.run = argv.slice(i + 1)
      break
    }
    if (arg === '--check') {
      a.check = true
      continue
    }
    if (arg === '--opencode') {
      a.opencode = argv[++i] ?? null
      if (!a.opencode) die('--opencode needs a URL, e.g. --opencode http://127.0.0.1:9711')
      continue
    }
    die(`unknown argument "${arg}"; usage: kickoff-hub-attach [--opencode <url>] [--run <command...>] [--check]`)
  }
  if (a.run !== null && a.run.length === 0) die('--run needs a command, e.g. --run opencode serve --port 9711')
  return a
}

const ARGS = parseArgs(process.argv.slice(2))

// `--check` reads `--opencode` and `--run` for what it can verify and starts NOTHING, so a wrapper
// runs its real line with `--check` in front of it. Every refusal below this point is one the check
// makes too, through `plan.ts`, because a check that blesses what the start refuses is worse than
// no check.
if (ARGS.check) {
  process.exit(await runCheck({ opencodeUrl: ARGS.opencode, run: ARGS.run }))
}

// The watcher's address, before anything is opened: a URL with no port sent it to port 80 once,
// while the claim was held and the topic made.
if (ARGS.opencode) {
  const wrong = opencodeUrlProblem(ARGS.opencode)
  if (wrong) die(wrong)
}

const READ = readConfig()
if ('problem' in READ) die(READ.problem.note)
const CONFIG = READ.config

// A relay is what a producer attaches TO. attach as a WHOLE holds the claim; its watcher is a
// producer of its own door, but attach itself set to be a producer would dial its own door and hold
// a claim for a conversation twice over. This is about who holds the claim, and attach does.
{
  const wrong = producerFlagProblem(CONFIG)
  if (wrong) die(wrong)
}

// ── PID 1, before anything is opened ─────────────────────────────────────────────────────────────
// A process with no handler for SIGTERM at PID 1 ignores it, and a JavaScript runtime at PID 1 does
// not reap the orphans an engine leaves — both measured on this box. Reaping is one job an init does
// in a kilobyte of C, so attach declines it and names the fix rather than doing it badly. Only under
// `--run`, because only then is there a child whose orphans and signals attach would have to own.
// The init named is a reaper, nothing more: docker's forwards a signal, bwrap's does not (§13.5), so
// a bwrap wall is stopped by signalling attach itself.
if (ARGS.run && process.pid === 1) {
  die('this process is PID 1 and nothing reaps for it; put the wall\'s own init in front — docker run --init, or bwrap without --as-pid-1 (which reaps and forwards nothing: stop a bwrap wall by signalling attach itself)')
}

// ── where the door is ────────────────────────────────────────────────────────────────────────────
// Told (KICKOFF_HUB_RELAY_SOCKET) or derived from git — both fold into CONFIG.relaySocket. When
// neither can say and there is a child to hand it to, attach makes a PRIVATE door of its own; when
// there is no child, it refuses as the relay always did.
let door = CONFIG.relaySocket
let privateDir: string | null = null
if (!door) {
  if (!ARGS.run) {
    die('this folder is not inside a repository and nothing named a door; set KICKOFF_HUB_RELAY_SOCKET, or start the worker inside the project')
  }
  ;[door, privateDir] = makePrivateDoor()
}

/**
 * A door nobody outside the wall needs to reach, so nobody outside the wall needs to know its name.
 *
 * NOT derived from the mount path: two walls with different projects mounted at `/workspace` and the
 * same address would derive one door under the mounted `fanin/` directory, and the second would
 * refuse "another attach holds it" — true, and the wrong reason.
 *
 * And NOT named by pid, which the first version did: under `bwrap --unshare-pid` attach is PID 2 in
 * EVERY wall, so two walls sharing the host's /tmp derived the identical folder, and the second died
 * "another attach is already holding it" — again true, again the wrong reason. `mkdtemp` is unique
 * by construction, whatever any wall thinks its pid is. The folder is unlinked on exit; a wall that
 * is killed outright leaves it behind, and no later wall ever reuses it.
 */
function makePrivateDoor(): [string, string] {
  const plan = privateDoorPlan(process.env.TMPDIR)
  if (plan.problem) die(plan.problem)
  const folder = mkdtempSync(join(plan.tmp, PRIVATE_DOOR_PREFIX))
  // The runtime's mkdtemp makes the folder 0700; said here rather than assumed, because the door
  // inside it is what a producer authenticates through.
  chmodSync(folder, 0o700)
  return [join(folder, 'door.sock'), folder]
}

// ── the door, opened ─────────────────────────────────────────────────────────────────────────────
note(`speaking for ${CONFIG.conversation ? `conversation ${CONFIG.conversation} in ` : ''}${CONFIG.projectDir}${CONFIG.address ? ` · ${CONFIG.address}` : ''}`)
note(`the door is ${door}`)
// The split-brain named in words at start, not only by `--check`: when the door is not the one a
// tool server would work out from git here, a config that derives its own looks for a door that
// nothing opens, and the agent reads "not said yet" for ever with every other line looking right.
{
  const fact = toolServerFact(CONFIG, door, ARGS.run !== null)
  if (fact?.warn) note(fact.text)
  // Half a phone, said at the start as well as by `--check`, from the same sentence.
  const half = typedWordsFact(ARGS.run, ARGS.opencode)
  if (half?.warn) note(half.text)
}

/**
 * This run of the watcher, named HERE rather than inside it, because the door needs to know it:
 * of the producers behind the door the watcher is the one that carries his typed words, and when
 * every producer refuses them the door forwards the watcher's reason over a tool server's. Null
 * without `--opencode`, and then there is no carrier to prefer.
 */
const WATCHER_INSTANCE = ARGS.opencode ? `${process.pid}-w-${Date.now()}` : null

const relay = createRelay({
  projectDir: CONFIG.projectDir,
  facts: CONFIG.facts,
  address: CONFIG.address,
  listen: door,
  hubSocket: CONFIG.hubSocket,
  graceMs: CONFIG.relayGraceMs,
  carrier: WATCHER_INSTANCE,
  // Resolved AFRESH on every attempt: the operator may `herdr-tg open` or `enroll` while this runs.
  secretOf: () => secretFor(CONFIG),
  whenNotEnrolled: notEnrolled(CONFIG),
  note,
  die,
})

await relay.bind()
relay.start()

/** Say the whole process's goodbye: the relay's `bye` and door, plus any private folder it made. */
function goodbye(): void {
  relay.goodbye()
  if (privateDir) {
    try {
      rmSync(privateDir, { recursive: true, force: true })
    } catch {
      /* going away regardless */
    }
  }
}

// ── the engine, as a child (only under --run) ────────────────────────────────────────────────────
if (ARGS.run) {
  // The nine namespace variables, every one set explicitly, so nothing is derived twice and nothing
  // is inherited from above. The engine inherits this, and the tool server it spawns inherits the
  // engine's — so a Claude plugin with no environment block of its own, and any adapter a stranger
  // writes, finds the door with no further configuration (§13.3).
  //
  // The secret's path is the one attach was told OR the one it found — wherever the ladder found
  // it, the channel's home included: a lane worktree holds no secret of its own and attach finds
  // the main tree's, and a child that does not search (the stranger, §5) was refusing "no secret
  // at <lane>/.kickoff/hub.token" while attach above it had just authenticated with that very
  // file. Resolved here, at spawn time, as everywhere else; the child still reads the file on
  // every dial of its own.
  //
  // EXCEPT when attach was told a conversation: then the conversation is what is pinned, and the
  // path is "as if unset", because a child given both is refused as two answers to one question —
  // and because the documented overlay for a second engine (§2) blanks the path and never the
  // conversation. The first version pinned the path and blanked the conversation, so a tool
  // server behind that overlay derived the SEED's door and read the SEED's secret, and every word
  // of the room's agent landed in the seed's topic. The one variable an overlay must not blank is
  // the one that has to carry the answer.
  const pinned: Record<string, string> = {
    KICKOFF_HUB_PROJECT_DIR: CONFIG.projectDir,
    KICKOFF_HUB_ADDRESS: CONFIG.address ?? '-',
    KICKOFF_HUB_CONVERSATION: CONFIG.conversation ?? '-',
    KICKOFF_HUB_TOKEN_FILE: CONFIG.conversation ? '-' : (CONFIG.tokenFile ?? secretFor(CONFIG)?.tokenFile ?? '-'),
    KICKOFF_HUB_SOCKET: CONFIG.hubSocket,
    KICKOFF_HUB_RELAY: '1',
    KICKOFF_HUB_RELAY_SOCKET: door,
    KICKOFF_HUB_RELAY_DIR: '-',
    KICKOFF_HUB_RELAY_GRACE_MS: '-',
  }
  // The door is bound and the hub link is dialling before the child exists, so the first tool server
  // the engine spawns finds the door. The link need not be up: a child whose hub is down is told
  // "not said yet" by its tool server, which is the honest sentence, and the link keeps dialling.
  if (ARGS.opencode) startWatcher(watcherConfig(ARGS.opencode))
  runChild({
    command: ARGS.run,
    cwd: CONFIG.projectDir,
    env: { ...process.env, ...pinned },
    note,
    onBeforeExit: goodbye,
  })
} else {
  // No child: attach is the relay (and, with --opencode, the watcher), supervised by whatever
  // started it. A clean goodbye on a signal, best effort and time-boxed — `process.exit` on the same
  // tick as the `bye` loses it to a short write.
  if (ARGS.opencode) startWatcher(watcherConfig(ARGS.opencode))
  let stopping = false
  const onStop = (): void => {
    if (stopping) return
    stopping = true
    goodbye()
    setTimeout(() => process.exit(0), 200)
  }
  for (const sig of ['SIGTERM', 'SIGINT'] as const) process.on(sig, onStop)
}

function watcherConfig(url: string) {
  return {
    door,
    address: CONFIG.address,
    instance: WATCHER_INSTANCE!,
    opencodeUrl: url,
    secretOf: () => secretFor(CONFIG),
    whenNotEnrolled: notEnrolled(CONFIG),
    projectDir: CONFIG.projectDir,
    note,
  }
}
