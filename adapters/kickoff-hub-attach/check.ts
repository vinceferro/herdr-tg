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

import { randomBytes } from 'crypto'
import { statSync } from 'fs'
import { join } from 'path'

import { notEnrolled, readConfig, secretFor } from '../../plugins/kickoff-channel/attach.ts'
import { HubLink } from '../../plugins/kickoff-channel/hub-link.ts'
import { THE_VERSION_LINE, attachedAs, bindingFileProblem, bindingGenerationProblem, opencodeUrlProblem, privateDoorPlan, producerFlagProblem, readBindingFile, sameDirectory, toolServerFact, typedWordsFact } from './plan.ts'

export type CheckOptions = {
  /** The opencode URL, if `--opencode` was on the line — checked for a port, as the start does. */
  opencodeUrl: string | null
  /** The file naming the worker's own session, if `--opencode-binding-file` was on the line. */
  bindingFile: string | null
  /** The oldest binding that file may name, as it was typed, if `--opencode-binding-generation` was. */
  bindingGeneration: string | null
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
            // Random and then the millisecond, never the pid: this is the one string the hub keys
            // a run by, and a pid means something only to this kernel and is issued again once its
            // numbers wrap. Same shape as `ledger.ts` and `server.ts`; see `docs/ATTACHING.md` §3b.
            instance: `${randomBytes(8).toString('hex')}-check-${Date.now()}`,
            // Both LOCAL DETAIL and both kept deliberately for one release: a hub older than
            // 8 September still REQUIRES them, and nothing routes on either.
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

  // ── the binding naming the worker's own session — only with --opencode-binding-file ───────────
  //
  // Two things can be known here and no more: that the path is one attach can use, and that what is
  // written there — if anything is yet — is a shape it can read. Whether the session is OPEN is the
  // server's answer and it is asked afresh on every line the operator types; asking it here would
  // print "not open" for the ordinary case, which is a wall checked before its engine is running.
  //
  // Absence is never a NOT: the order a wall is started in is check, start, write the binding.
  if (opts.bindingFile !== null || opts.bindingGeneration !== null) {
    const wrong = bindingFileProblem(opts.bindingFile, opts.opencodeUrl) ?? bindingGenerationProblem(opts.bindingGeneration, opts.bindingFile)
    if (wrong) {
      not(wrong)
    } else if (opts.bindingFile !== null) {
      const binding = readBindingFile(opts.bindingFile)
      if ('binding' in binding) {
        // Two of the binding's own claims are decided WITHOUT a server — the number it was written
        // at, and the project it says it is for — and the start refuses on both, on every line. A
        // check that read the shape and stopped blessed a wall where every typed line is refused
        // and no question is ever shown; the likeliest by hand is a launcher copying the wrong
        // project into the binding. Whether the session is OPEN stays the server's answer and is
        // still not asked here.
        const { canonicalProjectDir, conversation, generation, agent } = binding.binding
        // Which conversation the note was written for is the third such claim, and the one no
        // server could ever settle: a sibling room's session resolves, sits in a room tree, and
        // runs the expected agent. Checked here for the same reason as the other two — what this
        // command blesses, the worker can do.
        const here = attachedAs(CONFIG)
        // Weighed in the order the WORKER weighs them (`bindingNow`), and the conversation is first
        // there. One note can carry two faults at once, and what this command names first is the
        // fault somebody mends: naming a different one from the worker's sends them to fix the
        // thing the worker was never going to reach, and every typed line still refused after it.
        if (conversation !== null && here === null && CONFIG.tokenFile !== null) {
          // A wall pointed at its secret BY PATH cannot also be told which conversation it is —
          // this same program refuses the two variables together — so the way out has to name the
          // path it would be replacing, not a variable that would stop the wall starting at all.
          // Written out whole rather than assembled from a condition: the sentence a person acts on
          // is the thing to read in one piece, here and in the document that quotes it.
          not(`the worker's session: ${opts.bindingFile} is written for conversation ${conversation}, and this worker was pointed at its secret by path (KICKOFF_HUB_TOKEN_FILE), which does not say which conversation that secret is for, so the claim cannot be checked and every line the operator types would be refused; start it with KICKOFF_HUB_CONVERSATION in place of KICKOFF_HUB_TOKEN_FILE, or whatever writes the note must leave "conversation" out`)
        } else if (conversation !== null && here === null) {
          not(`the worker's session: ${opts.bindingFile} is written for conversation ${conversation}, and nothing here says which conversation this worker is, so the claim cannot be checked and every line the operator types would be refused; start it with KICKOFF_HUB_CONVERSATION, or whatever writes the note must leave "conversation" out`)
        } else if (conversation !== null && conversation !== here) {
          not(`the worker's session: ${opts.bindingFile} is written for conversation ${conversation} and this worker speaks for ${here}, so every line the operator types would be refused`)
        } else if (opts.bindingGeneration !== null && generation === null) {
          not(`the worker's session: ${opts.bindingFile} does not say how new it is, and this worker would be started for binding ${opts.bindingGeneration}; whatever writes it must number every writing`)
        } else if (opts.bindingGeneration !== null && generation !== null && generation < Number(opts.bindingGeneration)) {
          not(`the worker's session: ${opts.bindingFile} is older than the one this worker was started for; it is binding ${generation} and the number given is ${opts.bindingGeneration}, so every line the operator types would be refused`)
        } else if (canonicalProjectDir !== null && !sameDirectory(canonicalProjectDir, CONFIG.projectDir)) {
          not(`the worker's session: ${opts.bindingFile} is written for a different project than the one this is run in, so every line the operator types would be refused`)
        } else {
          // Whether the reverse half of the fence is RUNNING is a property of the note and not of
          // the flag: a question is checked against the agent its turn ran under only where the
          // note names one. A launcher writing `{version, session_id}` gets no turn-level check at
          // all, which is a perfectly good arrangement and a completely different one — and until
          // this line existed, nothing anywhere said which of the two a wall was in.
          ok(`the worker's session: ${opts.bindingFile} names a session, and every line the operator types goes to that one or is refused; ${
            agent === null
              ? 'it names no agent, so a question from that session is shown whatever agent its turn ran under'
              : `it names ${agent}, so a question is shown only where that session's turn ran under ${agent}`
          }`)
        }
      } else if (binding.state === 'not written yet') {
        ok(`the worker's session: ${opts.bindingFile} is not written yet; until whatever starts the engine writes it, every line the operator types is refused out loud rather than guessed at`)
      } else if (binding.state === 'not safe to read') {
        // Named here and nowhere else: the operator is told only that it could not be read, and the
        // person who can put it right is the one running this check.
        not(`the worker's session: ${opts.bindingFile} is not safe to read, so nothing it says is trusted — ${binding.why}; it must be a plain file this user owns that nobody else can read or write, in directories nobody else can write`)
      } else if (binding.state === 'unreadable') {
        not(`the worker's session: ${opts.bindingFile} is there and could not be read; attach must be able to read it, and it is written by whatever starts the engine`)
      } else if (binding.state === 'says no version') {
        // The one key this side asked the launcher for, and the whole of the fix: named with its
        // value, because whoever wrote the launcher cannot act on "add a version".
        not(`the worker's session: ${opts.bindingFile} does not say which form it is written in; whatever writes it must add ${THE_VERSION_LINE} to the object it writes, or every line the operator types is refused`)
      } else if (binding.state === 'a form it does not know') {
        // What was wrong with it, in the same line. "It holds one JSON object, saying …" describes
        // the shape and names nothing about THIS note, and a person handed only that about a file
        // they cannot see mends it a key at a time — which is four edits for the one note a
        // launcher from before the two halves agreed on a spelling actually writes.
        not(`the worker's session: ${opts.bindingFile} is written in a form attach does not know — ${binding.why}; it holds one JSON object, saying ${THE_VERSION_LINE} and naming the session as "session_id"`)
      } else {
        not(`the worker's session: ${opts.bindingFile} is not one attach can read — ${binding.why}; it holds one JSON object, saying ${THE_VERSION_LINE} and naming the session as "session_id"`)
      }
      // Only absence. A directory that is there and cannot be looked into has already been named
      // above, in the line that says what is wrong with it; saying "nothing has made it yet" in the
      // same run contradicts that line and sends whoever runs the wall after the wrong fix.
      const parent = join(opts.bindingFile, '..')
      try {
        statSync(parent)
      } catch (e) {
        if ((e as { code?: string })?.code === 'ENOENT') {
          warn(`the worker's session: nothing has made ${parent}/ yet, so whatever starts the engine must make it before it can write there`)
        }
      }
    }
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
    case 'stale_generation':
      // This command holds no place of its own — it dials once, is welcomed, and says goodbye
      // before the hub's ping is answered, so the hub gives back the number it granted and nothing
      // here ever carries one in. So the one thing this refusal cannot mean is what it says, and
      // the sentence says that and stops.
      //
      // It names no cause, because there is more than one left and this command cannot see which:
      // the two may disagree about the wire, or the hub may have kicked this connection for a
      // generation it holds on its own side. Naming one of them would send whoever runs the wall to
      // upgrade a binary over somebody else's defect. A sentence rather than the word, because the
      // word is machinery and this line is read by whoever runs the wall.
      return 'the hub says a newer run of this conversation took its place, which nothing this command does can cause; what went wrong is not something this command can see, and the hub\'s own log names the run it means'
    default:
      return `the hub refused for a reason this command does not know (${reason})`
  }
}
