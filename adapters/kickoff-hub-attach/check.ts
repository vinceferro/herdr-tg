/**
 * `--check` — prove this environment can reach the hub, one plain line per fact, then exit.
 *
 * It is what a wrapper runs before trusting a wall, and it is the first thing a person runs when a
 * worker is silent: every failure `docs/ATTACHING.md` spends ten sections describing — the silent
 * close, the stale inode, the missing mount, the empty variable, the door nothing listens at —
 * prints here as one line that names the fix.
 *
 * # It creates no topic, and this is proven, not hoped
 *
 * The hub's admission is ordered (`hub.rs`, `admit` then `serve_connection`): peer credentials, uid
 * first; then version, secret, enabled, address shape; then the claim is taken and `welcome` is
 * sent; THEN `ping`; and the topic is created ONLY after the pong, inside the `if live` branch. A
 * connection that ends before the pong is released and audited, and nothing reaches Telegram. So
 * `--check` dials, sends `hello`, treats the arrival of `welcome` as proof — socket reachable, uid
 * admitted, secret resolved to an enabled project, address well-formed and echoed, claim free —
 * sends `bye`, and closes WITHOUT ever ponging. `hub-link.ts`'s `once` mode is what suppresses the
 * pong; see the note there.
 *
 * # What it blesses, the worker can do
 *
 * Every refusal the start makes, the check makes too — through the one reader for the namespace
 * and `plan.ts` for the rest — because a check that says "everything is in place" to a worker that
 * then dies with exit 2 is a check a wrapper learns not to run. The first version blessed three
 * such environments.
 *
 * Two honest costs, so nobody is surprised: the check holds the claim for the length of one round
 * trip (the hub releases it the instant the connection ends, so a wrapper that checks and then
 * starts the worker is not refused), and it leaves one line in the hub's audit log — "connected but
 * never answered" — which is the hub's honest reading of a deliberate check.
 */

import { statSync } from 'fs'
import { join } from 'path'

import { notEnrolled, readConfig, secretFor } from '../../plugins/kickoff-channel/attach.ts'
import { HubLink } from '../../plugins/kickoff-channel/hub-link.ts'
import { opencodeUrlProblem, privateDoorPlan, producerFlagProblem, toolServerFact, typedWordsFact } from './plan.ts'

export type CheckOptions = {
  /** The opencode URL, if `--opencode` was on the line — checked for a port, as the start does. */
  opencodeUrl: string | null
  /** The command after `--run`, if any — enables the engine, private-door and PID-1 facts. */
  run: string[] | null
}

/** Prove reachability. Returns 0 when every fact is ok, 1 when any is not. Creates no topic. */
export async function runCheck(opts: CheckOptions): Promise<number> {
  let fails = 0
  const ok = (s: string) => console.log(`ok   ${s}`)
  const not = (s: string) => {
    console.log(`NOT  ${s}`)
    fails++
  }
  // True and worth fixing, but a worker here can still reach him: not counted against the check,
  // because a wrapper that is refused a wall that works learns not to run the check.
  const warn = (s: string) => console.log(`warn ${s}`)

  // ── the configuration ───────────────────────────────────────────────────────────────────────
  // One reader for the whole namespace, so what the check proves is what the worker would do. When
  // it refuses, it names the variable — the empty string, a relative path, an unaddressable name, a
  // secret handed by value. The reader's `note` register, not its `why`: `why` is written for an
  // agent reading a tool result ("this session … the bridge … him"), and whoever runs this has a
  // terminal, no session and no bridge. `note` is the sentence `main.ts` dies with.
  const READ = readConfig()
  if ('problem' in READ) {
    not(READ.problem.note)
    return done()
  }
  const CONFIG = READ.config
  const facts = CONFIG.facts

  // ── attach as a producer ────────────────────────────────────────────────────────────────────
  // The start refuses this; so does the check. It arrives by inheritance — every `--run` child has
  // it pinned — which is exactly the accident a check exists to catch.
  {
    const wrong = producerFlagProblem(CONFIG)
    if (wrong) not(wrong)
  }

  // ── the project ───────────────────────────────────────────────────────────────────────────────
  const where =
    !facts.mainTop
      ? '(not inside a repository)'
      : facts.mainTop === facts.projectTop
        ? '(the main tree of a repository)'
        : `(a linked worktree of ${facts.mainTop})`
  ok(`speaking for ${CONFIG.projectDir} ${where}`)

  // ── the conversation ──────────────────────────────────────────────────────────────────────────
  // Its shape was already checked by the reader above; a bad one is a refusal there, not here. So
  // this only narrates which conversation, and how it was chosen.
  if (CONFIG.address === null) {
    ok('the conversation: the project itself')
  } else if (CONFIG.addressWasGiven) {
    ok(`the conversation: ${CONFIG.address} (named by KICKOFF_HUB_ADDRESS)`)
  } else {
    ok(`the conversation: ${CONFIG.address} (git's name for this worktree)`)
  }

  // ── the secret ────────────────────────────────────────────────────────────────────────────────
  // Which term of the ladder found it is said, because "which conversation am I" has four answers
  // now and a person debugging a silent worker needs to know which one was taken.
  const project = secretFor(CONFIG)
  if (!project) {
    not(notEnrolled(CONFIG).note)
  } else {
    const how =
      project.how === 'told'
        ? 'told by KICKOFF_HUB_TOKEN_FILE'
        : project.how === 'named'
          ? `conversation ${project.conversation}, named by KICKOFF_HUB_CONVERSATION`
          : project.how === 'bound'
            ? `conversation ${project.conversation}, the one this repository is bound to`
            : `found above ${facts.mainTop ?? CONFIG.projectDir}, the older way`
    ok(`the secret: ${project.tokenFile} (${how})`)
  }

  // ── the hub's socket ──────────────────────────────────────────────────────────────────────────
  // The directory is the hub's and it is 0700, so a foreign uid cannot even look inside it — the
  // stat fails EACCES before any connect could. That is the first thing a rootless or remapped wall
  // hits, and it is not "the directory is not mounted": the mount is fine, the user is wrong.
  let socketThere = false
  const hubDir = join(CONFIG.hubSocket, '..')
  try {
    statSync(CONFIG.hubSocket)
    socketThere = true
    ok(`the hub's socket: ${CONFIG.hubSocket} is there`)
  } catch (e) {
    const code = (e as { code?: string })?.code
    if (code === 'EACCES' || code === 'EPERM') {
      not(`this user may not look inside ${hubDir}/ (it is the hub's, mode 0700); run as the same user as the hub`)
    } else {
      not(`nothing at ${CONFIG.hubSocket}; the hub is not running, or the directory ${hubDir}/ is not mounted here (mount the directory, never the socket file)`)
    }
  }

  // ── reached, and admitted ─────────────────────────────────────────────────────────────────────
  // One dial, `once`, so the hub is neither redialled nor ponged: `welcome` proves admission and the
  // topic is never made. Only attempted when there is a secret to present and a socket to reach.
  if (project && socketThere) {
    await new Promise<void>(resolve => {
      // One outcome per dial. The wire reports how the dial ended at most once, but a caller that
      // has already printed its line must not print another for the same dial — the close after a
      // refusal, or the timer firing after a welcome — so everything below is gated on this.
      let settled = false
      const finish = () => {
        if (settled) return
        settled = true
        resolve()
      }
      const link = new HubLink({
        framePrefix: 'c',
        note: () => {},
        once: true,
        whenUnreachable: 'the hub is not running',
        identify: () => ({
          socket: CONFIG.hubSocket,
          hello: {
            t: 'hello',
            project_id: 'unknown-until-the-hub-says',
            token: project.token,
            instance: `${process.pid}-check-${Date.now()}`,
            repo: project.repo,
            pid: process.pid,
            ...(CONFIG.address ? { lane: CONFIG.address } : {}),
          },
        }),
        onDial: end => {
          if (settled) return
          if ('code' in end) {
            if (end.code === 'EACCES') {
              not('the hub\'s socket refused this user; run as the same user as the hub')
            } else if (end.code === 'ECONNREFUSED') {
              not('a socket file is there but nothing is listening behind it; the hub is not running')
            } else {
              not(`could not reach the hub (${end.code})`)
            }
          } else {
            // Closed after the hello with no frame at all: a uid the hub reads as another user, or a
            // hello it could not decode. From outside the two cannot be told apart (§10).
            not('the hub took the hello and closed without a word; either this process is not running as the hub\'s user, or the hello was malformed, and from outside the two cannot be told apart')
          }
          finish()
        },
        onFrame: f => {
          if (settled) return
          if (f.t === 'welcome') {
            if (CONFIG.address && f.lane !== CONFIG.address) {
              not(`the hub did not give ${CONFIG.address} a place of its own; it is older than this command`)
            } else {
              ok('reached the hub')
              // The hub's title for the conversation already names the address when there is one
              // (`lane_title` in the registry composes "<project> · <address>", clipped from the
              // left); appending it again printed a lane's name twice, once with an ellipsis.
              ok(`admitted as "${f.project}"`)
            }
            // Prove the claim is released: say bye and close WITHOUT ponging. `markUp` records the
            // welcome so the close is not mistaken for a silent one.
            link.markUp()
            link.sendControl({ t: 'bye', reason: 'just checking' })
            setTimeout(() => {
              link.end()
              finish()
            }, 50)
            return
          }
          if (f.t === 'refused') {
            not(refusalSentence(String(f.reason), CONFIG.address, project))
            finish()
          }
        },
      })
      link.start()
      // A hub that accepts and then never speaks must not hang the check — and it is a hub that is
      // RUNNING (the connect succeeded) and wedged, which is a different fix from "not running".
      setTimeout(() => {
        if (settled) return
        not('the hub accepted the connection and said nothing for 6 seconds; it is running but wedged — restart herdr-tg')
        finish()
      }, 6000)
    })
  }

  // ── the door ──────────────────────────────────────────────────────────────────────────────────
  // Told, derived, or — under `--run` with neither — made in a private folder under TMPDIR when the
  // worker starts. That last one is planned here without making anything, with the same rule the
  // start applies: a TMPDIR that is not a path was blessed once and refused a second later.
  let door: string | null = CONFIG.relaySocket
  if (CONFIG.relaySocket) {
    const doorLive = await isLive(CONFIG.relaySocket)
    if (doorLive) {
      not(`another attach already holds the door at ${CONFIG.relaySocket}; this conversation has a worker already`)
    } else {
      const how = CONFIG.relaySocketWasGiven ? 'named by KICKOFF_HUB_RELAY_SOCKET' : 'worked out from git'
      ok(`the door: ${CONFIG.relaySocket} (${how}), free`)
    }
  } else if (opts.run) {
    const plan = privateDoorPlan(process.env.TMPDIR)
    if (plan.problem) {
      not(plan.problem)
      door = null
    } else {
      ok(`the door: will be made in a private folder under ${plan.tmp} when the worker starts, and handed to its engine`)
      door = join(plan.tmp, 'kickoff-hub-attach-…', 'door.sock')
    }
  } else {
    not('this folder is not inside a repository and nothing named a door; set KICKOFF_HUB_RELAY_SOCKET')
  }

  // ── the tool server ───────────────────────────────────────────────────────────────────────────
  // The same sentence attach prints as a warning at start, from the same function, so the check and
  // the worker cannot disagree about which door a tool server will look for.
  {
    const fact = toolServerFact(CONFIG, door, opts.run !== null)
    if (fact) (fact.ok ? ok : not)(fact.text)
  }

  // ── the engine's address — only with --opencode ───────────────────────────────────────────────
  if (opts.opencodeUrl !== null) {
    const wrong = opencodeUrlProblem(opts.opencodeUrl)
    if (wrong) not(wrong)
    else ok(`the engine's address: ${opts.opencodeUrl}`)
  }

  // ── the engine, and PID 1 — only under --run ─────────────────────────────────────────────────
  if (opts.run) {
    const bin = opts.run[0]
    const found = Bun.which(bin)
    if (found) ok(`the engine: ${bin}, found at ${found}`)
    else not(`the engine: ${bin} is not on PATH`)

    if (process.pid === 1) {
      not('this process is PID 1 and nothing reaps for it; put the wall\'s own init in front (docker run --init; bwrap without --as-pid-1, which reaps and forwards nothing — stop a bwrap wall by signalling attach itself)')
    } else {
      ok('not PID 1')
    }

    // Half a phone, said here and at the start from the same sentence.
    const half = typedWordsFact(opts.run, opts.opencodeUrl)
    if (half?.warn) warn(half.text)
  }

  return done()

  function done(): number {
    if (fails === 0) {
      console.log('everything a worker here needs is in place')
      return 0
    }
    console.log(`${fails} thing${fails === 1 ? '' : 's'} to fix before a worker here can reach him`)
    return 1
  }
}

/** Is something ANSWERING on this socket, as opposed to a leftover file? Dialled to tell them apart. */
async function isLive(path: string): Promise<boolean> {
  try {
    statSync(path)
  } catch {
    return false
  }
  return new Promise<boolean>(resolve => {
    Bun.connect({
      unix: path,
      socket: { open: s => { s.end(); resolve(true) }, data() {}, close() {}, error() {} },
    }).catch(() => resolve(false))
  })
}

function refusalSentence(reason: string, address: string | null, project: { how?: string; conversation?: string } | null): string {
  switch (reason) {
    case 'unknown_project':
    case 'bad_token': {
      // A secret the CHANNEL keeps that the hub refuses is most often a stale copy — a rotation
      // typed with a herdr-tg from before conversations existed rewrites the repo's copy alone —
      // and `open` on such a project says "already open", so the verb named is the one that
      // copies the current bytes across. A room has no folder to enrol at all.
      if (project?.how === 'named') {
        return `the secret the channel keeps for conversation ${project.conversation} is not one the hub knows; grant it again at a terminal (herdr-tg grant), or name a conversation that is with KICKOFF_HUB_CONVERSATION`
      }
      if (project?.how === 'bound') {
        return 'the secret the channel keeps for this project is not one the hub knows; at a terminal, copy the repo\'s current secret across: herdr-tg adopt-secrets --apply (or, if the repo holds none, enrol it again: herdr-tg enroll)'
      }
      return reason === 'unknown_project'
        ? 'the hub does not know this project; open it at a terminal: herdr-tg open (or, if it was enrolled before, enrol it again: herdr-tg enroll)'
        : 'the secret is not one the hub knows; enrol the project again at a terminal: herdr-tg enroll'
    }
    case 'not_enabled':
      return 'this project is enrolled but switched off'
    case 'version_skew':
      return 'this command and the hub do not speak the same version; upgrade one of them'
    case 'bad_lane':
      return `the hub will not address a conversation called ${address ?? 'this one'}; if the hub is older than this command, restart herdr-tg`
    case 'already_claimed':
      return 'another connection holds this conversation right now; if it is your own worker, run the check before it and not beside it; if nothing of yours is running, a stray process is squatting the claim'
    default:
      return `the hub refused for a reason this command does not know (${reason})`
  }
}
