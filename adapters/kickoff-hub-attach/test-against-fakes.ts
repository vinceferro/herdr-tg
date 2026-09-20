/**
 * The real attach watching a fake opencode, against a fake hub on a real Unix socket and a fake
 * opencode over real HTTP.
 *
 * Nothing here is mocked inside attach's own process: it is spawned as `bun main.ts --opencode …`,
 * exactly as it will run, and everything is observed from outside. attach holds the claim, opens its
 * door, and its in-process watcher turns the fake opencode's events into `ask` through that door —
 * so what the fake hub sees is what a wall's hub would see. A test that reached into the module
 * would prove the mapping and miss the two things that have actually broken this project — the
 * handshake, and what goes on the wire before `welcome`.
 *
 *     bun test-against-fakes.ts
 */

import { Glob } from 'bun'
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, renameSync, symlinkSync, writeFileSync, rmSync } from 'fs'

import { aSession as listedSession, claimingHub, fakeOpencode } from './test-harness.ts'
import { readBindingFile, sameDirectory } from './plan.ts'
import { tmpdir } from 'os'
import { join } from 'path'

// The channel's home, pointed at a directory of this run's own for every process spawned below,
// so nothing under test reads the operator's real one.
process.env.XDG_STATE_HOME = mkdtempSync(join(tmpdir(), 'kha-state-'))

let failures = 0
function check(what: string, ok: boolean, detail?: string): void {
  if (ok) {
    console.log(`  ok   ${what}`)
  } else {
    failures++
    console.log(`  FAIL ${what}${detail ? `\n       ${detail}` : ''}`)
  }
}

const SECRET = 'a'.repeat(64)

/** Frames the fake hub has received, in order. */
const seen: Record<string, any>[] = []
/** Requests the fake opencode has received, in order. */
const posted: { path: string; body: any }[] = []
/** Every `GET /session` the watcher asked, with its query string, so a test can see HOW it asked. */
const sessionQueries: string[] = []
/**
 * What the fake opencode holds, as `GET /session` lists it. Set per test.
 *
 * The shape is the one CAPTURED from opencode 1.18.25 on 5 September — `POST /session` in a git
 * repository, then `GET /session?directory=<that repo>&roots=true` — with the home path scrubbed
 * and the ids swapped for ones that name nothing (a tracked file carries no session id).
 * Two facts about it were measured rather than assumed, and the fake keeps both: `parentID` is
 * ABSENT on a root session and present on a child, and the server lists most recently updated
 * first. A fixture invented here would only prove this file agrees with itself, which is how the
 * event payload came to be read out of the wrong field once.
 */
let sessions: Record<string, any>[] = []
/**
 * What `POST …/prompt_async` answers. 204 is the captured answer. A test sets 404 for the session
 * that has gone between the list and the post — the body is the `NotFoundError` captured from
 * 1.18.25 on 5 September, with the id swapped for one that names nothing.
 */
let promptStatus = 204
/** Hang the NEXT request of the kind named: taken by the server and never answered. */
const hangNext = { session: false, prompt: false, reply: false }
/** What the two reply endpoints answer. 200 is what 1.18.25 answers a reply it took. */
let replyStatus = 200
/**
 * The messages this server holds, by id — `GET /session/{id}/message/{id}` answers `{info, parts}`,
 * and `info.agent` is the agent the TURN ran under.
 *
 * Captured from the 1.18.25 OpenAPI at `/doc`, not invented: `AssistantMessage` carries `agent`,
 * and the asked events do not — which is why the agent of a turn has to be asked for.
 */
const messages = new Map<string, Record<string, unknown>>()
const aSession = (id: string, updated: number, extra: Record<string, unknown> = {}) => ({
  id,
  slug: 'jolly-wizard',
  projectID: '0000000000000000000000000000000000000000',
  directory: repo,
  path: '',
  cost: 0,
  tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
  title: 'New session - 2026-09-05T11:26:25.115Z',
  version: '1.18.25',
  time: { created: 1788607585115, updated },
  ...extra,
})

// ── the fake hub ───────────────────────────────────────────────────────────────────────────────

const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'ocb-'))
const sockPath = join(dir, 'hub.sock')
const repo = join(dir, 'repo')
mkdirSync(join(repo, '.kickoff'), { recursive: true })
writeFileSync(join(repo, '.kickoff', 'hub.token'), SECRET)

let hubSock: import('bun').Socket | null = null
/** Set once the fake hub has sent `welcome`, so a test can prove nothing arrived before it. */
let welcomed = false
let beforeWelcome = 0

const hub = Bun.listen({
  unix: sockPath,
  socket: {
    open(s) {
      hubSock = s
    },
    data(s, chunk) {
      for (const line of chunk.toString().split('\n')) {
        if (!line.trim()) continue
        const f = JSON.parse(line)
        if (!welcomed && f.t !== 'hello') beforeWelcome++
        seen.push(f)
        if (f.t === 'hello') {
          // Resolve by secret, exactly as the hub does: the name comes from the registry, never
          // from anything the bridge sent.
          const ok = f.token === SECRET
          if (!ok) {
            s.write(JSON.stringify({ v: 1, id: 'h0', t: 'refused', reason: 'bad_token' }) + '\n')
            return
          }
          s.write(
            JSON.stringify({
              v: 1,
              id: 'h1',
              t: 'welcome',
              project: 'the-fake-project',
              // Echoed, because a bridge that names an address and is not given it back is talking
              // to a hub that admitted it AS THE WHOLE PROJECT, and it refuses rather than
              // impersonating one. A fake that swallowed the echo would keep this bridge down.
              ...(f.lane ? { lane: f.lane } : {}),
              limits: { max_frame: 65536, max_text: 4000, frames_per_min: 18 },
            }) + '\n',
          )
          welcomed = true
          return
        }
        // Every frame gets exactly one ack.
        s.write(JSON.stringify({ v: 1, id: `a${seen.length}`, t: 'ack', ref: f.id, delivered: 'yes' }) + '\n')
      }
    },
  },
})

/**
 * Push a tap down to the bridge, the way a real tap arrives, and hand back the id it went under.
 *
 * A fresh envelope id per tap, because the hub now waits for an answer NAMING that id — two taps
 * under one id would have the second's answer read as the first's, which is the one thing the id
 * is for.
 */
let tapSeq = 0
function tap(askId: string, optionId: string): string {
  const id = `h-tap-${++tapSeq}`
  hubSock?.write(
    JSON.stringify({ v: 1, id, t: 'choice', msg_id: 'm1', ask_id: askId, option_id: optionId }) + '\n',
  )
  return id
}

// ── the fake opencode ──────────────────────────────────────────────────────────────────────────

let pushEvent: ((e: unknown) => void) | null = null

const oc = Bun.serve({
  port: 0,
  hostname: '127.0.0.1',
  // Bun's server closes an idle request after ten seconds by default, which would end the hung
  // requests below from the server's side — the adapter's `fetch` then retried the GET and got a
  // real answer, and the hang under test never happened. The longest Bun allows.
  idleTimeout: 255,
  async fetch(req) {
    const url = new URL(req.url)
    if (url.pathname === '/event') {
      return new Response(
        new ReadableStream({
          start(c) {
            const enc = new TextEncoder()
            pushEvent = e => c.enqueue(enc.encode(`data: ${JSON.stringify(e)}\n\n`))
          },
        }),
        { headers: { 'content-type': 'text/event-stream' } },
      )
    }
    if (url.pathname === '/session' && req.method === 'GET') {
      // Measured on 5 September against 1.18.25: `directory=` is an exact match on the session's
      // own directory (a trailing slash and a symlink both resolve to it, a subfolder does not),
      // `roots=true` drops every session that has a `parentID`, and the answer keeps the server's
      // own order — most recently updated first.
      sessionQueries.push(url.search)
      if (hangNext.session) {
        hangNext.session = false
        return new Promise<Response>(() => {})
      }
      const dir = (url.searchParams.get('directory') ?? '').replace(/\/+$/, '')
      const list = sessions
        .filter(s => !dir || s.directory === dir)
        .filter(s => url.searchParams.get('roots') !== 'true' || !s.parentID)
        .sort((a, b) => b.time.updated - a.time.updated)
      return Response.json(list)
    }
    const asMessage = /^\/session\/([^/]+)\/message\/([^/]+)$/.exec(url.pathname)
    if (asMessage && req.method === 'GET') {
      const info = messages.get(asMessage[2])
      if (!info) return new Response('not found', { status: 404 })
      return Response.json({ info, parts: [] })
    }
    if (req.method === 'POST') {
      posted.push({ path: url.pathname, body: await req.json() })
      // `prompt_async` answers 204 with no body — captured — and the rest answer `{}`.
      if (url.pathname.endsWith('/prompt_async')) {
        if (hangNext.prompt) {
          hangNext.prompt = false
          return new Promise<Response>(() => {})
        }
        if (promptStatus === 204) return new Response(null, { status: 204 })
        return Response.json(
          { name: 'NotFoundError', data: { message: 'Session not found: ses_000000000000000000theOne' } },
          { status: promptStatus },
        )
      }
      if (url.pathname.endsWith('/reply')) {
        if (hangNext.reply) {
          hangNext.reply = false
          return new Promise<Response>(() => {})
        }
        if (replyStatus !== 200) return Response.json({ name: 'NotFoundError' }, { status: replyStatus })
      }
      return new Response('{}', { headers: { 'content-type': 'application/json' } })
    }
    return new Response('not found', { status: 404 })
  },
})

// ── run the real attach, watching the fake opencode ──────────────────────────────────────────────
//
// `main.ts --opencode <url>` is the whole thing: attach dials the fake hub as the relay, and its
// in-process watcher dials attach's own door as a producer and turns the fake opencode's events into
// `ask`. So the fake hub sees ONE connection — attach's — and the watcher's asks reach it through
// the door, their ids namespaced on the way, exactly as any producer's would.
const child = Bun.spawn(['bun', join(import.meta.dir, 'main.ts'), '--opencode', `http://127.0.0.1:${oc.port}`], {
  env: {
    ...process.env,
    KICKOFF_HUB_PROJECT_DIR: repo,
    KICKOFF_HUB_SOCKET: sockPath,
    // The test repo is not a git checkout, so no door derives from it; attach is told its door
    // outright, which is the container answer §10 documents.
    KICKOFF_HUB_RELAY_SOCKET: join(dir, 'door.sock'),
  },
  stdout: 'pipe',
  stderr: 'pipe',
})

const until = async (what: string, cond: () => boolean, ms = 5000): Promise<boolean> => {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    if (cond()) return true
    await new Promise(r => setTimeout(r, 25))
  }
  console.log(`  (gave up waiting for ${what})`)
  return false
}

const frame = (t: string) => seen.find(f => f.t === t)
const frames = (t: string) => seen.filter(f => f.t === t)

// ── one wire, and one file that writes it ──────────────────────────────────────────────────────
//
// This file used to carry its own copy of the queue, the framing and the reconnect, and it drifted
// from the reviewed one by twelve invariants — every one of them a defect already found and fixed
// on the other side. Prose cannot hold that line; the moment somebody dials for themselves again
// the drift starts over, so the check is mechanical: no adapter opens its own connection, and every
// adapter takes the wire from the same module.
console.log('one wire, and one file that writes it')
const REPO_TOP = join(import.meta.dir, '..', '..')
// The files that ARE allowed to speak the wire — each holds a `HubLink` and a `hello`. Everything
// else under `adapters/` and `plugins/` must not.
const ADAPTERS = [
  'plugins/kickoff-channel/server.ts',
  'adapters/kickoff-hub-attach/relay.ts',
  'adapters/kickoff-hub-attach/opencode.ts',
  'adapters/kickoff-hub-attach/check.ts',
]
const sources = ADAPTERS.map(f => ({ f, src: readFileSync(join(REPO_TOP, f), 'utf8') }))
// Three marks of a file that has started writing the wire again, and each was on the fork. Not
// `Bun.connect` itself: the door opens one to knock on its OWN address and tell a leftover socket
// file apart from a second attach still answering, and that connection never speaks a frame.
const sharing = sources.filter(x => /from '[^']*hub-link\.ts'/.test(x.src)).map(x => x.f)
const answering = sources.filter(x => /t: 'pong'/.test(x.src)).map(x => x.f)
const redeclaring = sources
  .filter(x => /const (PROTOCOL_VERSION|MAX_FRAME_BYTES|MAX_PENDING)\s*=/.test(x.src))
  .map(x => x.f)
// And the other half of the rule, now that attach is several files: no OTHER `.ts` under
// `adapters/` or `plugins/` — test files and the harness aside — may mint a `hello`. A second file
// that says `t: 'hello'` is a second adapter dialling for itself, which is exactly how the drift
// began. The one listed hello per file above is the only wire mouth this repo has.
const others = [...new Glob('{adapters,plugins}/**/*.ts').scanSync(REPO_TOP)].filter(
  f =>
    !f.includes('node_modules') &&
    !/(^|\/)test-/.test(f) &&
    !ADAPTERS.includes(f),
)
const rogueHello = others.filter(f => /t: 'hello'/.test(readFileSync(join(REPO_TOP, f), 'utf8')))
check(
  'every_adapter_speaks_the_same_wire_from_the_same_file',
  sharing.length === ADAPTERS.length && answering.length === 0 && redeclaring.length === 0 && rogueHello.length === 0,
  `sharing hub-link.ts: ${sharing.length} of ${ADAPTERS.length}; answering a ping themselves: ${answering.join(', ') || 'none'}; declaring the protocol themselves: ${redeclaring.join(', ') || 'none'}; minting a hello elsewhere: ${rogueHello.join(', ') || 'none'}`,
)

try {
  console.log('\na refusal from a hub newer than this bridge is waited out, not given up on')
  // The fork treated any reason it did not recognise as permanent and stopped for the life of the
  // process — so a hub shipped after it, refusing for something recoverable, took the project's
  // voice off the phone until somebody noticed. The shared wire waits, and backs off while it does.
  const skewSock = join(dir, 'newer-hub.sock')
  const attempts: number[] = []
  const skewHub = Bun.listen({
    unix: skewSock,
    socket: {
      open() {},
      data(s, chunk) {
        for (const line of chunk.toString().split('\n')) {
          if (!line.trim()) continue
          if (JSON.parse(line).t !== 'hello') continue
          attempts.push(Date.now())
          s.write(JSON.stringify({ v: 1, id: 'r1', t: 'refused', reason: 'the-registry-is-resyncing' }) + '\n')
          s.end()
        }
      },
    },
  })
  const skewChild = Bun.spawn(['bun', join(import.meta.dir, 'main.ts'), '--opencode', 'http://127.0.0.1:9'], {
    env: {
      ...process.env,
      KICKOFF_HUB_PROJECT_DIR: repo,
      KICKOFF_HUB_SOCKET: skewSock,
      // The `--opencode` URL is deliberately nowhere. This attach is here for its HUB behaviour, and
      // pointing its watcher at the fake opencode would make it the last subscriber to that one
      // event stream — every event the tests below push would then go here instead of the process
      // under test.
      KICKOFF_HUB_RELAY_SOCKET: join(dir, 'skew-door.sock'),
    },
    stdout: 'ignore',
    stderr: 'ignore',
  })
  check(
    'an_unknown_refusal_is_waited_out_rather_than_ending_this_bridge_for_good',
    await until('a third attempt after an unknown refusal', () => attempts.length >= 3, 12000),
    `${attempts.length} attempt(s)`,
  )
  // And the waits GROW. The fork reset its backoff on connect rather than on `welcome`, so a hub
  // that accepted and then refused was redialled at a flat one second for ever — a claim squatted
  // by a stray process became a permanent 1 Hz hammer on the hub, each round costing it a full
  // admission.
  const gaps = attempts.slice(1).map((t, i) => t - attempts[i])
  check(
    'and each wait is longer than the last, rather than a flat one-second hammer',
    gaps.length >= 2 && gaps[1] > gaps[0] * 1.5,
    JSON.stringify(gaps),
  )
  skewChild.kill()
  skewHub.stop(true)

  console.log('\na claim waited out and then let go is not a claim held for ever')
  // The counter that decides "this holder is not letting go" has to count a RUN of consecutive
  // refusals, never a lifetime tally. Its sibling in `server.ts` clears it on `welcome`; the copy
  // here dropped that line, so two ordinary restarts months apart could add up to three and take
  // the bridge down for good — emptying its queue and giving up on the question an agent was
  // blocked on, while telling whoever read stderr to go and kill a process that does not exist.
  //
  // The script below is what an ordinary box produces: the hub restarting with a lingering claim,
  // this bridge restarting and racing its predecessor. The connection SUCCEEDS twice in between,
  // which is exactly what tells a run apart from a tally.
  const claimSock = join(dir, 'restarting-hub.sock')
  const script = ['refuse', 'refuse', 'welcome-then-close', 'refuse', 'welcome']
  let hellos = 0
  const claimSeen: Record<string, any>[] = []
  const claimHub = Bun.listen({
    unix: claimSock,
    socket: {
      open() {},
      data(s, chunk) {
        for (const line of chunk.toString().split('\n')) {
          if (!line.trim()) continue
          const f = JSON.parse(line)
          claimSeen.push(f)
          if (f.t !== 'hello') {
            s.write(JSON.stringify({ v: 1, id: `a${claimSeen.length}`, t: 'ack', ref: f.id, delivered: 'yes' }) + '\n')
            continue
          }
          const step = script[Math.min(hellos, script.length - 1)]
          hellos++
          if (step === 'refuse') {
            s.write(JSON.stringify({ v: 1, id: 'c1', t: 'refused', reason: 'already_claimed' }) + '\n')
            s.end()
            continue
          }
          s.write(JSON.stringify({ v: 1, id: 'c2', t: 'welcome', project: 'the-fake-project',
            limits: { max_frame: 65536, max_text: 4000, frames_per_min: 18 } }) + '\n')
          if (step === 'welcome-then-close') setTimeout(() => s.end(), 150)
        }
      },
    },
  })
  // Its own opencode: the bridge subscribes to one event stream and the last subscriber wins, so
  // sharing the one above would take every event away from the process the rest of this file tests.
  let claimPush: ((e: unknown) => void) | null = null
  const claimOc = Bun.serve({
    port: 0,
    hostname: '127.0.0.1',
    async fetch(req) {
      const url = new URL(req.url)
      if (url.pathname === '/event') {
        return new Response(
          new ReadableStream({
            start(c) {
              const enc = new TextEncoder()
              claimPush = e => c.enqueue(enc.encode(`data: ${JSON.stringify(e)}\n\n`))
            },
          }),
          { headers: { 'content-type': 'text/event-stream' } },
        )
      }
      if (req.method === 'POST') return new Response('{}', { headers: { 'content-type': 'application/json' } })
      return new Response('not found', { status: 404 })
    },
  })
  const claimChild = Bun.spawn(['bun', join(import.meta.dir, 'main.ts'), '--opencode', `http://127.0.0.1:${claimOc.port}`], {
    env: {
      ...process.env,
      KICKOFF_HUB_PROJECT_DIR: repo,
      KICKOFF_HUB_SOCKET: claimSock,
      KICKOFF_HUB_RELAY_SOCKET: join(dir, 'claim-door.sock'),
    },
    stdout: 'ignore',
    stderr: 'ignore',
  })
  // The question is minted in the gap after the LAST refusal, which is the moment a lifetime tally
  // has reached three and a run has reached one.
  await until('the fourth hello', () => hellos >= 4, 20000)
  await until('the fake opencode stream', () => claimPush !== null, 20000)
  claimPush!({
    id: 'evt_claim',
    type: 'permission.v2.asked',
    properties: { id: 'pper_x', sessionID: 'ses_c', action: 'run a command', resources: ['ls'] },
  })
  check(
    'a_question_asked_while_a_claim_is_being_waited_out_still_reaches_the_hub',
    await until('the ask through the mended link', () => claimSeen.some(f => f.t === 'ask'), 20000),
    `hellos=${hellos}  ask frames at the hub: ${claimSeen.filter(f => f.t === 'ask').length}`,
  )
  claimChild.kill()
  claimHub.stop(true)
  claimOc.stop(true)

  console.log('\nthe bridge and the hub agree on the handshake')
  check('it says hello with the secret and no name of its own', await until('hello', () => !!frame('hello')))
  check('the hello carries no display name', frame('hello') !== undefined && !('title' in (frame('hello') ?? {})))
  check('nothing at all went out before the hub said welcome', beforeWelcome === 0, `${beforeWelcome} frame(s) did`)

  console.log('\na question opencode asks becomes a question on the phone')
  await until('the stream', () => pushEvent !== null)
  pushEvent!({
    id: 'evt_1',
    type: 'question.v2.asked',
    // `properties`, not `data` — this is the shape captured from a real opencode /event stream on
    // 2 September. The first draft of this test invented `data`, the bridge read `data`, and both
    // agreed with each other and not with opencode.
    properties: {
      id: 'que_1',
      sessionID: 'ses_1',
      questions: [
        {
          question: 'Delete the staging database?',
          header: 'Destructive',
          options: [
            { label: 'Delete it', description: 'goes away' },
            { label: 'Leave it', description: 'stays' },
          ],
        },
      ],
    },
  })
  check('it arrives as an ask', await until('the ask', () => !!frame('ask')))
  const ask = frame('ask')
  check('it carries the question opencode published', ask?.text?.startsWith('Delete the staging database?') === true, JSON.stringify(ask?.text))
  check(
    'its buttons are the labels opencode published',
    JSON.stringify(ask?.options?.map((o: any) => o.label)) === JSON.stringify(['Delete it', 'Leave it']),
    JSON.stringify(ask?.options),
  )

  console.log('\na tap goes back to opencode as the label it published')
  const tookIt = tap(ask.ask_id, ask.options[0].option_id)
  check('opencode is told', await until('the reply', () => posted.length > 0))
  check(
    'at the endpoint the spec names for a v2 question',
    posted[0]?.path === '/api/session/ses_1/question/que_1/reply',
    posted[0]?.path,
  )
  check(
    'carrying the label, not the button id',
    JSON.stringify(posted[0]?.body) === JSON.stringify({ answers: [['Delete it']] }),
    JSON.stringify(posted[0]?.body),
  )
  // The hub takes the buttons off when it resolves the tap, and its note says the phone. A second
  // retirement from this side overwrites that with "answered at the terminal", which is false.
  await new Promise(r => setTimeout(r, 400))
  check('and the bridge does not retire it a second time', !frame('ask_resolved'), JSON.stringify(frame('ask_resolved')))

  // The hub has already put "Sent: <label>" under his tap, and it edits that line to what the
  // engine actually did with it. So an answer has to go back, and it has to go back only once
  // opencode has spoken — an `accepted` sent when the POST was merely started is the dead keyboard
  // this whole path exists to end, wearing a thumbs-up.
  const answerFor = (ref: string) => seen.find(f => f.t === 'ack' && f.ref === ref)
  check(
    'the_watcher_acks_a_choice_only_after_opencode_has_answered',
    (await until('the answer to the tap', () => !!answerFor(tookIt), 8000)) &&
      answerFor(tookIt)?.status === 'accepted',
    JSON.stringify(answerFor(tookIt) ?? null),
  )

  console.log('\na tap on a question that is already answered answers nothing twice')
  const before = posted.length
  tap(ask.ask_id, ask.options[0].option_id)
  await new Promise(r => setTimeout(r, 300))
  check('nothing more is posted to opencode', posted.length === before, `${posted.length - before} were`)

  console.log('\nwhat the operator is told when the worker will not take his answer')
  {
    pushEvent!({
      id: 'evt_no',
      type: 'question.v2.asked',
      properties: {
        id: 'que_refused',
        sessionID: 'ses_1',
        questions: [{ question: 'Roll it forward?', header: 'Migrate', options: [{ label: 'Roll it' }] }],
      },
    })
    await until('the question', () => frames('ask').some(f => f.text?.startsWith('Roll it forward?')))
    const q = frames('ask').find(f => f.text?.startsWith('Roll it forward?'))!
    replyStatus = 500
    const refusedRef = tap(q.ask_id, q.options[0].option_id)
    const got = async () => {
      await until('the answer', () => seen.some(f => f.t === 'ack' && f.ref === refusedRef), 8000)
      return seen.find(f => f.t === 'ack' && f.ref === refusedRef)
    }
    const said = await got()
    check(
      'a_tap_the_worker_would_not_take_is_answered_refused_and_never_as_taken',
      said?.status === 'refused' && /would not take it/.test(String(said?.reason)),
      JSON.stringify(said ?? null),
    )
    check(
      'and_the_reason_names_no_status_code_and_no_machinery',
      typeof said?.reason === 'string' && !/\d/.test(said.reason) && !/opencode|watcher|session/i.test(said.reason),
      String(said?.reason),
    )
    replyStatus = 200
  }

  console.log('\nwhen the worker takes the answer and never says so')
  {
    pushEvent!({
      id: 'evt_hang',
      type: 'question.v2.asked',
      properties: {
        id: 'que_hung',
        sessionID: 'ses_1',
        questions: [{ question: 'Wipe the cache?', header: 'Cache', options: [{ label: 'Wipe it' }] }],
      },
    })
    await until('the question', () => frames('ask').some(f => f.text?.startsWith('Wipe the cache?')))
    const q = frames('ask').find(f => f.text?.startsWith('Wipe the cache?'))!
    hangNext.reply = true
    const hungRef = tap(q.ask_id, q.options[0].option_id)
    check(
      'a_tap_the_worker_never_answers_is_answered_refused_once_the_deadline_passes',
      await until('the answer after the deadline', () => seen.some(f => f.t === 'ack' && f.ref === hungRef), 14000),
      JSON.stringify(seen.filter(f => f.t === 'ack' && f.ref === hungRef)),
    )
    const late = seen.find(f => f.t === 'ack' && f.ref === hungRef)
    check(
      'and_it_says_the_server_did_not_answer_in_time',
      late?.status === 'refused' && /did not answer in time/.test(String(late?.reason)),
      JSON.stringify(late ?? null),
    )
  }

  console.log('\na permission request offers only the three answers opencode accepts')
  pushEvent!({
    id: 'evt_2',
    type: 'permission.v2.asked',
    properties: { id: 'per_1', sessionID: 'ses_1', action: 'run a command', resources: ['rm -rf /'] },
  })
  // Found by its own shape, never by position: the questions above it are a moving target.
  const permAsks = () => frames('ask').filter(f => Array.isArray(f.options) && f.options.some((o: any) => o.option_id === 'once'))
  check('it arrives as an ask', await until('the permission ask', () => permAsks().length > 0))
  const perm = permAsks()[0]
  check('it says what is wanted, in plain words', perm?.text?.includes('run a command') === true, perm?.text)
  check(
    'three buttons and no more',
    perm?.options?.length === 3 && JSON.stringify(perm.options.map((o: any) => o.option_id)) === JSON.stringify(['once', 'always', 'reject']),
    JSON.stringify(perm?.options),
  )
  // Tapped while the question is still open, so this proves the option lookup refuses it — not
  // merely that the record was already consumed, which is what an answered question would prove.
  // It is its OWN permission ask, because the door — like the real hub — resolves a question the
  // instant it is tapped and drops any second `choice` for it; a real tap cannot land twice, and
  // the reject below is a fresh question rather than a second tap on this one.
  const beforeBogus = posted.length
  tap(perm.ask_id, 'an-option-nobody-minted')
  await new Promise(r => setTimeout(r, 300))
  check('a tap naming an option it never offered posts nothing', posted.length === beforeBogus, `${posted.length - beforeBogus} did`)

  pushEvent!({
    id: 'evt_3',
    type: 'permission.v2.asked',
    properties: { id: 'per_2', sessionID: 'ses_1', action: 'delete a file', resources: ['build/'] },
  })
  await until('the second permission ask', () => permAsks().length > 1)
  const perm2 = frames('ask').filter(f => Array.isArray(f.options) && f.options.some((o: any) => o.option_id === 'reject')).at(-1)
  const beforeReject = posted.length
  tap(perm2!.ask_id, 'reject')
  check('a tap replies at the permission endpoint', await until('the permission reply', () => posted.length > beforeReject && posted.some(p => p.path.includes('permission'))))
  const pr = posted.find(p => p.path.includes('permission'))
  check(
    'at the endpoint the spec names, carrying the ids opencode published',
    pr?.path === '/api/session/ses_1/permission/per_2/reply',
    pr?.path,
  )
  check('with the enum opencode accepts', JSON.stringify(pr?.body) === JSON.stringify({ reply: 'reject' }), JSON.stringify(pr?.body))

  console.log('\na question answered at the keyboard has its buttons taken off the phone')
  const asksBeforeShip = frames('ask').length
  pushEvent!({
    type: 'question.v2.asked',
    properties: {
      id: 'que_2',
      sessionID: 'ses_1',
      questions: [{ question: 'Ship it?', header: 'Ship', options: [{ label: 'Yes', description: 'go' }] }],
    },
  })
  await until('the ship question', () => frames('ask').length > asksBeforeShip)
  const retiredBefore = frames('ask_resolved').length
  // Deliberately the `data` shape: the durable stream uses it, and the bridge must read both.
  pushEvent!({ type: 'question.v2.replied', data: { sessionID: 'ses_1', requestID: 'que_2', answers: [['Yes']] } })
  check(
    'the keyboard is retired without anyone tapping it',
    await until('the second retirement', () => frames('ask_resolved').length > retiredBefore),
  )

  // ── typed steering ───────────────────────────────────────────────────────────────────────────
  //
  // The other half of a phone. A tap has reached opencode since the bridge existed; the operator's
  // TYPED words reached a line on stderr saying it was not built. What arrives from the hub is
  // `message{msg_id, text, from, in_reply_to_ask?}`, and what the watcher does with it is decided
  // here, against the shapes captured from the real server on 5 September:
  //
  //   * the words become a PROMPT to the session, verbatim — `POST /session/{id}/prompt_async`
  //     with `{parts: [{type: 'text', text}]}`, which answered 204 and ran the agent with the
  //     session's own model. The v2 `/api/session/{id}/prompt` admitted the prompt and ran nothing.
  //   * WHICH session is the machine's answer, never the text's: `GET /session?directory=<the
  //     project directory attach speaks for>&roots=true`, most recently updated first.
  //   * every `message` is answered on the wire with `ack{ref, status, reason?}` — `refused`
  //     with a reason when there was nothing to hand the words to, so the hub can tell him.
  //
  // Every ack below is read off the wire the fake hub saw; the hub's own half — that a refused
  // ack becomes a line in the topic he typed in — is `crates/kickoff-channel/src/hub/tests.rs`.
  const typed = (id: string, text: string, extra: Record<string, unknown> = {}) =>
    hubSock?.write(
      JSON.stringify({ v: 1, id, t: 'message', msg_id: `m-${id}`, text, from: { chat_id: -1001, user_id: 7 }, ...extra }) + '\n',
    )
  const ackFor = (ref: string) => seen.find(f => f.t === 'ack' && f.ref === ref)
  const prompts = () => posted.filter(p => p.path.endsWith('/prompt_async'))

  console.log('\nwhat the operator types reaches the session as a prompt')
  sessions = [aSession('ses_000000000000000000theOne', 1788607585115)]
  const promptsBefore = prompts().length
  typed('h-m1', 'try it with --dry-run first')
  check(
    'what_the_operator_types_reaches_the_opencode_session_as_a_prompt',
    await until('the prompt', () => prompts().length > promptsBefore) &&
      prompts().at(-1)?.path === '/session/ses_000000000000000000theOne/prompt_async' &&
      JSON.stringify(prompts().at(-1)?.body) === JSON.stringify({ parts: [{ type: 'text', text: 'try it with --dry-run first' }] }),
    JSON.stringify(prompts().at(-1) ?? null),
  )
  check(
    'and the session was the one the server lists for the project directory, asked for by directory and roots',
    sessionQueries.length > 0 &&
      sessionQueries.every(q => new URLSearchParams(q).get('directory') === repo && new URLSearchParams(q).get('roots') === 'true'),
    JSON.stringify(sessionQueries),
  )
  check(
    'and the hub is told, on the wire, that the words were taken',
    (await until('the ack', () => ackFor('h-m1') !== undefined)) && ackFor('h-m1')?.status === 'accepted',
    JSON.stringify(ackFor('h-m1') ?? null),
  )

  console.log('\na file he sent reaches the session as a file part beside his words')
  // The frame carries the PATH the hub minted (`docs/ATTACHING.md` §14). opencode's v1 prompt has
  // a file part, read off the running server's own `/doc` (1.18.25) and not guessed: `FilePartInput`
  // requires `type`, `mime` and `url`, allows `filename`, and forbids anything else. For a `file:`
  // URL the SERVER reads the path from disk and hands the model a data URL — so the same-path
  // mount rule binds the server process, and nothing here reads a byte.
  const minted = '/state/media/p-9f3a1c2e5b7d/-/20260905-231455-9f3a1c2e.jpg'
  const promptsBeforeFile = prompts().length
  typed('h-m1f', 'this is what the login page looks like now', {
    files: [{ kind: 'photo', path: minted, mime: 'image/jpeg', bytes: 1183412 }],
  })
  check(
    'a_photo_he_sends_reaches_the_opencode_session_as_a_file_part_beside_his_words',
    (await until('the prompt with the file', () => prompts().length > promptsBeforeFile)) &&
      JSON.stringify(prompts().at(-1)?.body) ===
        JSON.stringify({
          parts: [
            { type: 'text', text: 'this is what the login page looks like now' },
            { type: 'file', mime: 'image/jpeg', url: `file://${minted}` },
          ],
        }),
    JSON.stringify(prompts().at(-1)?.body ?? null),
  )
  check(
    'and_the_ack_counts_the_file_it_handed_on',
    (await until('the ack', () => ackFor('h-m1f') !== undefined)) &&
      ackFor('h-m1f')?.status === 'accepted' &&
      ackFor('h-m1f')?.files === 1,
    JSON.stringify(ackFor('h-m1f') ?? null),
  )
  // The reported name travels in `filename`, as data, and never becomes the URL.
  const promptsBeforeNamed = prompts().length
  typed('h-m1n', '', {
    files: [{ kind: 'document', path: '/state/media/p-9f3a1c2e5b7d/-/20260905-231501-0a1b2c3d', mime: 'application/x-pem-file', bytes: 400, filename: '../../.ssh/id_ed25519' }],
  })
  check(
    'the_name_his_phone_reported_is_the_file_parts_filename_and_never_its_url',
    (await until('the prompt with the named file', () => prompts().length > promptsBeforeNamed)) &&
      JSON.stringify(prompts().at(-1)?.body) ===
        JSON.stringify({
          parts: [
            { type: 'file', mime: 'application/x-pem-file', url: 'file:///state/media/p-9f3a1c2e5b7d/-/20260905-231501-0a1b2c3d', filename: '../../.ssh/id_ed25519' },
          ],
        }),
    JSON.stringify(prompts().at(-1)?.body ?? null),
  )
  // A file that did not come through is one line in the text part, and no file part at all.
  const promptsBeforeNoFile = prompts().length
  typed('h-m1g', '', { files: [{ kind: 'document', filename: 'build.log', why: 'too-big' }] })
  check(
    'a_file_that_did_not_come_through_is_said_in_the_prompt_and_is_never_a_file_part',
    (await until('the prompt without the file', () => prompts().length > promptsBeforeNoFile)) &&
      prompts().at(-1)?.body?.parts?.length === 1 &&
      prompts().at(-1)?.body?.parts?.[0]?.type === 'text' &&
      /build\.log/.test(String(prompts().at(-1)?.body?.parts?.[0]?.text)) &&
      /did not come through/.test(String(prompts().at(-1)?.body?.parts?.[0]?.text)) &&
      /20 MB/.test(String(prompts().at(-1)?.body?.parts?.[0]?.text)),
    JSON.stringify(prompts().at(-1)?.body ?? null),
  )
  check(
    'and_its_ack_still_counts_the_entry_it_handed_on',
    (await until('the ack', () => ackFor('h-m1g') !== undefined)) &&
      ackFor('h-m1g')?.status === 'accepted' &&
      ackFor('h-m1g')?.files === 1,
    JSON.stringify(ackFor('h-m1g') ?? null),
  )
  // What the hub does about telling HIM is an ordinary send against a ceiling every project
  // shares, and it can be shed with only the journal knowing. Stated as a fact, it had the agent
  // answering "as you saw, the screenshot did not come through" to somebody who saw nothing.
  check(
    'and_the_agent_is_never_told_the_operator_has_already_read_something_nobody_confirmed',
    !/has been told/i.test(String(prompts().at(-1)?.body?.parts?.[0]?.text)),
    String(prompts().at(-1)?.body?.parts?.[0]?.text),
  )
  // A file this machine could not store: nothing was downloaded, so the advice is the opposite of
  // a failed download's, and a watcher that did not know the word would say "the hub did not
  // say why" for the one case where it said exactly why.
  const promptsBeforeNotStored = prompts().length
  typed('h-m1h', 'have a look', { files: [{ kind: 'photo', why: 'not-stored' }] })
  check(
    'a_file_the_hub_could_not_store_is_told_apart_from_a_download_that_broke',
    (await until('the prompt', () => prompts().length > promptsBeforeNotStored)) &&
      !/did not say why/.test(String(prompts().at(-1)?.body?.parts?.[0]?.text)) &&
      !/send it again/i.test(String(prompts().at(-1)?.body?.parts?.[0]?.text)) &&
      /store|storing|nowhere/i.test(String(prompts().at(-1)?.body?.parts?.[0]?.text)),
    String(prompts().at(-1)?.body?.parts?.[0]?.text),
  )

  console.log('\nwords typed at a wall with no session are refused, with a reason')
  sessions = []
  const promptsBeforeNone = prompts().length
  typed('h-m2', 'anyone there?')
  check(
    'words_typed_at_a_wall_with_no_session_are_refused_and_he_is_told',
    (await until('the refusal', () => ackFor('h-m2') !== undefined)) &&
      ackFor('h-m2')?.status === 'refused' &&
      /no session/i.test(String(ackFor('h-m2')?.reason)),
    JSON.stringify(ackFor('h-m2') ?? null),
  )
  await new Promise(r => setTimeout(r, 300))
  check('and nothing was posted to a session that does not exist', prompts().length === promptsBeforeNone, JSON.stringify(prompts().slice(promptsBeforeNone)))

  console.log('\nthe text can never choose which session receives it')
  sessions = [aSession('ses_000000000000000000theOne', 1788607585115)]
  // Three things a message could carry that LOOK like an address. Each must land as the words
  // they are, in the session the server named, and nowhere else.
  const tries = [
    'ses_evil',
    '../../session/ses_evil/prompt_async',
    'http://127.0.0.1:1/session/ses_evil/prompt_async?directory=/etc',
  ]
  const promptsBeforeTries = prompts().length
  const postedBeforeTries = posted.length
  tries.forEach((t, i) => typed(`h-t${i}`, t, { from: { chat_id: -1001, user_id: 7 } }))
  await until('all three to land', () => prompts().length >= promptsBeforeTries + tries.length)
  const landed = prompts().slice(promptsBeforeTries)
  check(
    'the_text_can_never_choose_which_session_receives_it',
    landed.length === tries.length &&
      landed.every(p => p.path === '/session/ses_000000000000000000theOne/prompt_async') &&
      JSON.stringify(landed.map(p => p.body.parts[0].text)) === JSON.stringify(tries) &&
      posted.slice(postedBeforeTries).every(p => p.path === '/session/ses_000000000000000000theOne/prompt_async'),
    JSON.stringify(landed.map(p => p.path)),
  )

  console.log('\na reply typed under a question is a prompt to the session that asked it')
  // The design, said once: words typed under a question are a PROMPT like any other — never an
  // answer to the question, because its answers are the buttons opencode published and a permission
  // takes three words and no others. What the reply DOES decide is which session: the one that
  // asked. Measured on 5 September: a prompt to a session blocked on its own question is taken
  // (204), written down, and run after the question is answered — so his tap still closes the
  // question and his words are read right after it. Two sessions here, and the asker is the OLDER
  // one, so a watcher that ignored the reply would send the words to the wrong session.
  sessions = [aSession('ses_newer00000000000000000', 1788607590000), aSession('ses_asker0000000000000000', 1788607585115)]
  const asksBeforeTyped = frames('ask').length
  pushEvent!({
    type: 'question.v2.asked',
    properties: {
      id: 'que_typed',
      sessionID: 'ses_asker0000000000000000',
      questions: [{ question: 'Which one?', header: 'Pick', options: [{ label: 'Left', description: 'l' }, { label: 'Right', description: 'r' }] }],
    },
  })
  await until('the question', () => frames('ask').length > asksBeforeTyped)
  const asked = frames('ask').at(-1)!
  const promptsBeforeReply = prompts().length
  const retiredBeforeReply = frames('ask_resolved').length
  typed('h-r1', 'the left one, but only for staging', { in_reply_to_ask: asked.ask_id })
  check(
    'a_reply_typed_under_a_question_is_handled_the_way_the_design_says',
    (await until('the prompt to the asker', () => prompts().length > promptsBeforeReply)) &&
      prompts().at(-1)?.path === '/session/ses_asker0000000000000000/prompt_async' &&
      prompts().at(-1)?.body?.parts?.[0]?.text === 'the left one, but only for staging' &&
      !posted.some(p => p.path.includes('/question/que_typed/')),
    JSON.stringify(prompts().at(-1) ?? null),
  )
  await new Promise(r => setTimeout(r, 300))
  check('and the question stays open — nothing answered it and nothing retired it', frames('ask_resolved').length === retiredBeforeReply && !posted.some(p => p.path.includes('/question/que_typed/')))
  const promptsBeforePlain = prompts().length
  typed('h-r2', 'and a word for whoever is current')
  check(
    'while words typed under nothing go to the most recently updated session',
    (await until('the plain prompt', () => prompts().length > promptsBeforePlain)) &&
      prompts().at(-1)?.path === '/session/ses_newer00000000000000000/prompt_async',
    prompts().at(-1)?.path,
  )
  tap(asked.ask_id, asked.options[0].option_id)
  check(
    'and a tap still answers it, at the question endpoint, with the label',
    await until('the tap', () => posted.some(p => p.path === '/api/session/ses_asker0000000000000000/question/que_typed/reply')) &&
      JSON.stringify(posted.find(p => p.path.includes('/question/que_typed/'))?.body) === JSON.stringify({ answers: [['Left']] }),
  )

  // ── what reaches his phone is in his words ───────────────────────────────────────────────────
  console.log('\na refusal reaches his phone in his words, with no status code and no internal name')
  // The reason goes VERBATIM into the topic he typed in — the hub only strips control characters
  // and clips — so an HTTP status in it is jargon on his phone, and "watcher" is a thing he has
  // never been told exists. The number belongs on stderr, where it still is.
  sessions = [aSession('ses_000000000000000000theOne', 1788607585115)]
  promptStatus = 404
  typed('h-w1', 'is anyone reading this?')
  const worded = (await until('the refusal', () => ackFor('h-w1') !== undefined)) ? ackFor('h-w1') : null
  check(
    'a_refusal_reaches_his_phone_in_his_words_with_no_status_code_and_no_internal_name',
    worded?.status === 'refused' &&
      !/\b\d{3}\b/.test(String(worded.reason)) &&
      !/watcher/i.test(String(worded.reason)) &&
      /would not take it/.test(String(worded.reason)),
    JSON.stringify(worded ?? null),
  )
  promptStatus = 204
  sessions = []
  typed('h-w2', 'and now?')
  const none = (await until('the refusal', () => ackFor('h-w2') !== undefined)) ? ackFor('h-w2') : null
  // The hub's sentence is "What you typed did not reach the agent — <reason>.", singular; a reason
  // written apart from it said "them".
  check(
    'and the reason agrees in number with the sentence the hub puts it in',
    none?.status === 'refused' && /hand it to/.test(String(none.reason)) && !/them/.test(String(none.reason)),
    JSON.stringify(none ?? null),
  )

  // ── a server that takes a request and never answers ──────────────────────────────────────────
  console.log('\na request the server takes and never answers cannot silence every later line')
  // Typed lines are carried one after another so their order is kept, and Bun's `fetch` waits for
  // ever by default — so one request the server accepted and never answered parked that line AND
  // every line typed after it, none of them acked: the hub went on believing each was read.
  // Measured before the fix: two messages, twelve seconds, no ack for either. The server here
  // hangs exactly one request of each kind; the line typed behind it must still be carried.
  sessions = [aSession('ses_000000000000000000theOne', 1788607585115)]
  hangNext.session = true
  typed('h-h1', 'first, into the void')
  typed('h-h2', 'second, typed behind it')
  const hung = (await until('the first line to be given up on', () => ackFor('h-h1') !== undefined, 14000)) ? ackFor('h-h1') : null
  check(
    'a_session_lookup_the_server_never_answers_is_given_up_on_and_refused_in_time',
    hung?.status === 'refused' && /did not answer in time/.test(String(hung.reason)),
    JSON.stringify(hung ?? null),
  )
  const behind = (await until('the second line', () => ackFor('h-h2') !== undefined, 5000)) ? ackFor('h-h2') : null
  check(
    'and the line typed behind it is carried rather than parked for ever',
    behind?.status === 'accepted' && prompts().some(p => p.body?.parts?.[0]?.text === 'second, typed behind it'),
    JSON.stringify(behind ?? null),
  )
  hangNext.prompt = true
  typed('h-h3', 'third, taken and never answered')
  typed('h-h4', 'fourth, typed behind that')
  const hungPrompt = (await until('the third line to be given up on', () => ackFor('h-h3') !== undefined, 14000)) ? ackFor('h-h3') : null
  check(
    'a_prompt_the_server_takes_and_never_answers_is_given_up_on_and_refused_in_time',
    hungPrompt?.status === 'refused' && /did not answer in time/.test(String(hungPrompt.reason)),
    JSON.stringify(hungPrompt ?? null),
  )
  const behindThat = (await until('the fourth line', () => ackFor('h-h4') !== undefined, 5000)) ? ackFor('h-h4') : null
  check('and the line typed behind that is carried too', behindThat?.status === 'accepted', JSON.stringify(behindThat ?? null))

  // ── words taken, then not acted on ───────────────────────────────────────────────────────────
  console.log('\nwords the agent then could not act on are said so, in the topic')
  // 204 from `prompt_async` means opencode wrote the words down; the agent has not run. When it
  // then cannot — a gateway down, a provider key expired, the context overflowed — opencode says
  // so on the event stream as `session.error`, and the watcher had no case for it: the hub posted
  // nothing, and the only record was a stack trace in a stream nobody watches. The shape below was
  // CAPTURED from 1.18.25 on 5 September (a prompt naming a provider that does not exist; no model
  // call is made): two `session.error` events for one failure, the second carrying the stack,
  // with `session.idle` between them.
  sessions = [aSession('ses_000000000000000000theOne', 1788607585115)]
  typed('h-e1', 'hello from the phone')
  await until('the words to be taken', () => ackFor('h-e1')?.status === 'accepted')
  const saysBefore = frames('say').length
  const failed = (sessionID: string, message: string) => ({
    id: 'evt_00000000000000000000000001',
    type: 'session.error',
    properties: { sessionID, error: { name: 'UnknownError', data: { message } } },
  })
  pushEvent!(failed('ses_000000000000000000theOne', 'Model not found: no-such-provider/no-such-model.'))
  const toldHim = (await until('the line', () => frames('say').length > saysBefore)) ? frames('say').at(-1) : null
  check(
    'words_accepted_that_the_agent_then_could_not_act_on_are_said_so_in_the_topic',
    /could not act on what you typed/.test(String(toldHim?.text)) &&
      /Model not found: no-such-provider\/no-such-model/.test(String(toldHim?.text)),
    JSON.stringify(toldHim ?? null),
  )
  pushEvent!({ id: 'evt_00000000000000000000000002', type: 'session.idle', properties: { sessionID: 'ses_000000000000000000theOne' } })
  pushEvent!(
    failed(
      'ses_000000000000000000theOne',
      'ProviderModelNotFoundError: Model not found: no-such-provider/no-such-model.\n    at <anonymous> (/$bunfs/root/chunk-gt0nh583.js:439:95275)',
    ),
  )
  await new Promise(r => setTimeout(r, 400))
  check(
    'and it is said once for one failure, not once per event opencode emits about it',
    frames('say').length === saysBefore + 1,
    `${frames('say').length - saysBefore} line(s)`,
  )
  pushEvent!(failed('ses_000000000000000000nobody', 'Model not found: x/y.'))
  await new Promise(r => setTimeout(r, 400))
  check('and a session nobody typed at failing says nothing on his phone', frames('say').length === saysBefore + 1)

  // One of those failures is this adapter's own doing, and it must not reach him in the server's
  // register. Measured against a real opencode 1.18.25 on 7 September: a prompt naming an agent the
  // server cannot resolve is answered 204, NO user message is created — his words are gone — and
  // the failure comes back as `Agent not found: "<name>". Available agents: build, explore,
  // general, plan`. Forwarded verbatim that is a quoted identifier and an internal roster, on a
  // phone. Since this watcher is what names the agent, it is also what knows what that sentence
  // means, and it says it in his words.
  const saysBeforeAgent = frames('say').length
  typed('h-e2', 'and this one names an agent')
  await until('the words to be taken', () => ackFor('h-e2')?.status === 'accepted')
  pushEvent!(failed('ses_000000000000000000theOne',
    'Agent not found: "kickoff-room-steering". Available agents: build, explore, general, plan'))
  const aboutTheAgent = (await until('the line about the agent', () => frames('say').length > saysBeforeAgent)) ? frames('say').at(-1) : null
  check(
    'an_agent_the_workers_server_cannot_resolve_is_said_in_his_own_words_and_never_in_the_servers',
    /could not act on what you typed/.test(String(aboutTheAgent?.text)) &&
      /agent its server does not know/.test(String(aboutTheAgent?.text)) &&
      !/Available agents|Agent not found|"/.test(String(aboutTheAgent?.text)),
    JSON.stringify(aboutTheAgent ?? null),
  )

  // ── the session the note names, and nothing else ──────────────────────────────────────────────
  //
  // Without `--opencode-binding-file` the watcher takes the most recently active root session the
  // server lists for the project directory — a guess whenever a wall runs more than one. Measured
  // on this box on 6 September: for a steering room, the only root session in the room's directory
  // was an unrestricted coordinator, while the session the room actually steers was a different one
  // — so the operator's typed words would have gone to a session nobody meant him to steer.
  //
  // With the flag, whatever starts the engine writes a note saying which session is this worker's
  // own, and every line is delivered to THAT session or refused out loud. The note is read at
  // delivery time, never cached, so replacing it retargets the next line; and it is validated
  // against the server on every line, because a note naming a session that has gone, been
  // archived, been started as a subagent's, or belongs to another project must refuse rather
  // than fall back to the guess this flag exists to remove.
  //
  // Its own rig: a hub, an opencode and an attach of its own, because the process above it was
  // started without the flag and the flag is read once, at start.
  console.log('\nthe session the note names is the only session the operator can reach')
  {
    const noteDir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'ocn-'))
    const noteRepo = join(noteDir, 'repo')
    const elsewhere = join(noteDir, 'another-project')
    mkdirSync(join(noteRepo, '.kickoff'), { recursive: true })
    mkdirSync(elsewhere, { recursive: true })
    writeFileSync(join(noteRepo, '.kickoff', 'hub.token'), SECRET)
    const notePath = join(noteDir, 'opencode-session')
    /** Written the way the contract says a launcher writes it: whole, by rename, 0600. */
    const writeNote = (text: string | null): void => {
      if (text === null) {
        rmSync(notePath, { force: true })
        return
      }
      writeFileSync(`${notePath}.new`, text, { mode: 0o600 })
      renameSync(`${notePath}.new`, notePath)
    }

    // The rig is attached AS a conversation, because the launcher's note says which conversation
    // the session belongs to and the whole point of that key is that it is compared with this one.
    // Written the way the channel keeps it: the hub's own state directory, one secret per room.
    const ROOM = 'c-0d0d0d0d0d0d'
    const ANOTHER_ROOM = 'c-0e0e0e0e0e0e'
    const noteXdg = join(noteDir, 'xdg')
    mkdirSync(join(noteXdg, 'herdr-tg', 'conversations', ROOM), { recursive: true, mode: 0o700 })
    writeFileSync(join(noteXdg, 'herdr-tg', 'conversations', ROOM, 'secret'), SECRET, { mode: 0o600 })

    const BOUND = 'ses_theBoundOne00000000000'
    const NEWER = 'ses_theNewerOne00000000000'
    const OTHER = 'ses_theOtherOne00000000000'

    const nHub = claimingHub(join(noteDir, 'hub.sock'))
    const nOc = fakeOpencode()
    nOc.sessions = []
    const nChild = Bun.spawn(
      ['bun', join(import.meta.dir, 'main.ts'), '--opencode', nOc.url, '--opencode-binding-file', notePath],
      {
        env: {
          ...process.env,
          KICKOFF_HUB_PROJECT_DIR: noteRepo,
          KICKOFF_HUB_CONVERSATION: ROOM,
          XDG_STATE_HOME: noteXdg,
          KICKOFF_HUB_SOCKET: join(noteDir, 'hub.sock'),
          KICKOFF_HUB_RELAY_SOCKET: join(noteDir, 'door.sock'),
        },
        stdout: 'ignore',
        stderr: 'ignore',
      },
    )
    try {
      const nFrames = (t: string) => nHub.got.filter(f => f.t === t)
      const nAck = (ref: string) => nHub.got.find(f => f.t === 'ack' && f.ref === ref)
      let typedN = 0
      const typeAt = (text: string, extra: Record<string, unknown> = {}): string => {
        const id = `n-${++typedN}`
        nHub.to({ v: 1, id, t: 'message', msg_id: `mn-${id}`, text, from: { chat_id: -1001, user_id: 7 }, ...extra })
        return id
      }
      const reasonFor = async (id: string): Promise<string> => {
        await until(`the answer about ${id}`, () => nAck(id) !== undefined, 8000)
        const a = nAck(id)
        return a?.status === 'refused' ? String(a.reason) : `NOT REFUSED: ${JSON.stringify(a ?? null)}`
      }
      const prompts = () => nOc.prompts()
      const attached = await until('the watcher to attach and subscribe', () => nOc.pushing && nHub.got.some(f => f.t === 'hello'), 15000)
      check('a_watcher_given_a_session_note_still_attaches_to_its_door_and_its_hub', attached)
      // Everything below needs a live watcher, and a rig that limped on without one would report a
      // dozen failures that all say the same thing.
      if (!attached) throw new Error('attach never came up with --opencode-binding-file')

      // A wall boots before its launcher has written the note; every line typed meanwhile is
      // refused out loud rather than queued or guessed at.
      nOc.sessions = [listedSession(NEWER, noteRepo, 1788607590000), listedSession(BOUND, noteRepo, 1788607585115)]
      const beforeAbsent = prompts().length
      const absent = await reasonFor(typeAt('anyone home?'))
      check(
        'a_session_note_that_is_not_there_yet_refuses_the_line_rather_than_guessing',
        /has not yet said which session/.test(absent) && prompts().length === beforeAbsent,
        `${absent} · ${prompts().length - beforeAbsent} prompt(s)`,
      )
      writeNote('')
      const empty = await reasonFor(typeAt('and now?'))
      check(
        'an_empty_session_note_reads_as_one_that_has_not_been_written_yet',
        /has not yet said which session/.test(empty),
        empty,
      )

      // Half a write, a word that is not an id, and a shape with no session in it. None of them may
      // fall through to the guess.
      const malformed: string[] = []
      // The last of these is a note from a launcher that knows something this attach does not: a
      // key it cannot read may be a NARROWING of what the session must be, and obeying the rest of
      // the note while dropping it would be delivering his words on a rule nobody checked.
      for (const bad of ['the coordinator', '{"version":1,"session_id":"ses_theBou', '{"version":1,"canonical_project_dir":"/somewhere"}', 'ses_the one\nses_two', '{"version":1,"session_id":"ses_theBoundOne00000000000","must_be_titled":"the room"}']) {
        writeNote(bad)
        malformed.push(await reasonFor(typeAt('take this')))
      }
      check(
        'a_session_note_written_half_way_or_holding_anything_but_a_session_id_refuses_the_line',
        malformed.every(r => /is not one it can read/.test(r)),
        JSON.stringify(malformed),
      )

      // The whole point: two root sessions in one directory, the note names the OLDER, and the
      // most-recent rule must lose.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      const beforeBound = prompts().length
      const boundId = typeAt('try it with --dry-run first')
      check(
        'typed_words_go_to_the_session_the_note_names_and_never_to_the_most_recently_active_one',
        (await until('the prompt', () => prompts().length > beforeBound, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          JSON.stringify(prompts().at(-1)?.body) ===
            JSON.stringify({ parts: [{ type: 'text', text: 'try it with --dry-run first' }] }) &&
          nAck(boundId)?.status === 'accepted',
        `${prompts().at(-1)?.path} · ${JSON.stringify(nAck(boundId) ?? null)}`,
      )
      check(
        'and_the_note_is_still_checked_against_the_server_by_directory_and_roots',
        nOc.sessionQueries.length > 0 &&
          nOc.sessionQueries.some(q => new URLSearchParams(q).get('directory') === noteRepo && new URLSearchParams(q).get('roots') === 'true'),
        JSON.stringify(nOc.sessionQueries.slice(-3)),
      )

      // A note left behind by a previous run of the wall.
      writeNote(JSON.stringify({ version: 1, session_id: 'ses_theGoneOne0000000000000' }))
      const beforeStale = prompts().length
      const stale = await reasonFor(typeAt('are you there'))
      check(
        'a_session_note_naming_a_session_the_server_no_longer_lists_refuses_rather_than_guessing',
        /is not open on its server/.test(stale) && prompts().length === beforeStale,
        `${stale} · ${prompts().length - beforeStale} prompt(s)`,
      )

      // Archived: the session exists, and speaking to it is not the same as speaking to the worker.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115, { time: { created: 1, updated: 1788607585115, archived: 1788607586000 } }), listedSession(NEWER, noteRepo, 1788607590000)]
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      const beforeArchived = prompts().length
      const archived = await reasonFor(typeAt('still there?'))
      check(
        'a_session_note_naming_an_archived_session_says_so_and_never_falls_back_to_another',
        /has been archived/.test(archived) && prompts().length === beforeArchived,
        `${archived} · ${prompts().length - beforeArchived} prompt(s)`,
      )

      // A subagent's session: a direct listing resolves the id, and it is still not the
      // conversation on his phone.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115, { parentID: NEWER }), listedSession(NEWER, noteRepo, 1788607590000)]
      const helper = await reasonFor(typeAt('you there?'))
      check(
        'a_session_note_naming_a_subagents_session_is_refused_rather_than_spoken_to',
        /helper/.test(helper),
        helper,
      )

      // The cross-directory id the operator's brief names: the server WILL resolve it, and this
      // conversation still must not speak to it.
      nOc.sessions = [listedSession(BOUND, elsewhere, 1788607585115), listedSession(NEWER, noteRepo, 1788607590000)]
      const beforeElsewhere = prompts().length
      const otherProject = await reasonFor(typeAt('hello over there'))
      check(
        'a_session_note_naming_a_session_in_another_directory_is_refused_though_the_server_would_resolve_it',
        /belongs to a different project/.test(otherProject) && prompts().length === beforeElsewhere,
        `${otherProject} · ${prompts().length - beforeElsewhere} prompt(s)`,
      )

      // The defect measured on this box: the only root session in the room's directory was an
      // unrestricted coordinator, and the room's own session was a different one. A note that names
      // the agent it expects refuses the coordinator instead of steering it.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115, { agent: 'coordinator' })]
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, agent: 'kickoff-room-steering' }))
      const beforeAgent = prompts().length
      const wrongAgent = await reasonFor(typeAt('do the thing'))
      check(
        'a_session_note_naming_the_agent_it_expects_refuses_a_session_running_a_different_one',
        /different agent/.test(wrongAgent) && prompts().length === beforeAgent,
        `${wrongAgent} · ${prompts().length - beforeAgent} prompt(s)`,
      )
      // A session from before agents were named carries none at all, which is not a match either.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      const noAgent = await reasonFor(typeAt('and now'))
      check(
        'and_a_session_that_names_no_agent_at_all_is_not_taken_for_the_one_expected',
        /agent/.test(noAgent),
        noAgent,
      )
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115, { agent: 'kickoff-room-steering' }), listedSession(NEWER, noteRepo, 1788607590000, { agent: 'coordinator' })]
      const beforeMatch = prompts().length
      typeAt('go on then')
      check(
        'and_the_session_whose_agent_matches_is_the_one_the_words_reach',
        (await until('the prompt', () => prompts().length > beforeMatch, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async`,
        String(prompts().at(-1)?.path),
      )

      // Proving the SESSION's agent is not enough, and the room tree that shipped an opencode.json
      // naming the org coordinator as its default is why: the prompt endpoint resolves an agent of
      // its own when the body does not name one, so a session bound to one agent can run its turn
      // under another. The body names it. (`agent` is an optional top-level string on
      // `POST /session/{id}/prompt_async`, read off the 1.18.25 OpenAPI at `/doc`.)
      check(
        'the_prompt_that_carries_his_words_names_the_agent_the_note_binds',
        prompts().at(-1)?.body?.agent === 'kickoff-room-steering',
        JSON.stringify(prompts().at(-1)?.body),
      )

      // The agent is named, and it also has to EXIST. Measured on a real 1.18.25 on 7 September:
      // `prompt_async` naming an agent the server cannot resolve answers 204 and writes no message
      // at all — his words are gone — so the ack for them said "taken" and his line carried the
      // thumb, with the contradiction arriving seconds later as a line in the topic. The session
      // check cannot see it: a session goes on naming an agent that has been dropped from the
      // tree's configuration while it lives, which is the case the server's own error names.
      //
      // RED, before the fix:
      //   FAIL words_for_a_worker_whose_agent_its_server_cannot_resolve_are_refused_rather_than_lost
      //        NOT REFUSED: {"v":1,"id":"r17","t":"ack","ref":"n-16","status":"accepted"} · 1 prompt(s)
      {
        nOc.agents = ['build', 'plan']
        const beforeGone = prompts().length
        const gone = await reasonFor(typeAt('is anyone still there?'))
        check(
          'words_for_a_worker_whose_agent_its_server_cannot_resolve_are_refused_rather_than_lost',
          /agent its server does not know/.test(gone) && prompts().length === beforeGone,
          `${gone} · ${prompts().length - beforeGone} prompt(s)`,
        )
        // A set from BEFORE is not evidence about a name that is not in it. The refusal above
        // remembered {build, plan}; a server that will not say now — an older one with no such
        // route, or one that answered 500 — leaves this side unable to find out, and an agent added
        // since the last good listing is precisely the name a remembered set is wrong about.
        // Refusing his words on it is a line silenced for a fact nobody observed, and the refusal
        // it imitates is the one the server would give if the agent really were gone.
        //
        // The count of questions PUT is checked, not just the words arriving: the words arrive
        // whenever the name is in the remembered set, so a check that never made this side look
        // things up again would pass whatever the server now says.
        //
        // RED, before the fix:
        //   FAIL an_agent_missing_from_the_last_good_listing_is_not_refused_when_the_server_cannot_say_now
        //        the worker is set to run as an agent its server does not know · 0 prompt(s) · 1 asked
        nOc.agents = null
        const askedBeforeStale = nOc.agentQueries
        const beforeStale = prompts().length
        const stale = await reasonFor(typeAt('is the door still open?'))
        check(
          'an_agent_missing_from_the_last_good_listing_is_not_refused_when_the_server_cannot_say_now',
          prompts().length > beforeStale && nOc.agentQueries > askedBeforeStale,
          `${stale} · ${prompts().length - beforeStale} prompt(s) · ${nOc.agentQueries - askedBeforeStale} asked`,
        )
        // The set the server answers with is the whole server's, so it is asked once for a run and
        // remembered — and a name missing from what was remembered is asked about again before
        // anything is refused on it, or an agent added while the wall ran would be refused for ever.
        nOc.agents = ['build', 'kickoff-room-steering']
        const beforeKnown = prompts().length
        typeAt('and now?')
        check(
          'and_an_agent_the_server_does_resolve_still_carries_his_words',
          await until('the prompt', () => prompts().length > beforeKnown, 8000),
          `${prompts().length - beforeKnown} prompt(s)`,
        )
        // And once the server HAS said the name resolves, that is remembered for the run: the
        // ordinary typed line costs no request at all. Proved by the question NOT being put again
        // rather than by the words arriving — the words arrive either way, which is how a check
        // written here on a server that had stopped answering passed without ever reaching it.
        nOc.agents = null
        const askedBeforeRemembered = nOc.agentQueries
        const beforeRemembered = prompts().length
        typeAt('and at a server that has stopped saying?')
        check(
          'and_a_name_the_server_already_resolved_comes_from_what_was_remembered_without_asking_again',
          (await until('the prompt', () => prompts().length > beforeRemembered, 8000)) &&
            nOc.agentQueries === askedBeforeRemembered,
          `${prompts().length - beforeRemembered} prompt(s) · ${nOc.agentQueries - askedBeforeRemembered} asked`,
        )
      }

      // And the same rule on the way back, or it is a rule in one direction only: a turn that ran
      // under another agent is another worker's turn, and its question belongs on nobody's phone
      // under this project's name.
      {
        const asksBeforeStray = nFrames('ask').length
        nOc.messages.set('msg_stray', { id: 'msg_stray', sessionID: BOUND, role: 'assistant', agent: 'coordinator' })
        nOc.push({
          type: 'question.v2.asked',
          properties: {
            id: 'que_stray',
            sessionID: BOUND,
            tool: { messageID: 'msg_stray', callID: 'call_1' },
            questions: [{ question: 'Push to production?', header: 'Deploy', options: [{ label: 'Push it' }] }],
          },
        })
        await new Promise(r => setTimeout(r, 900))
        check(
          'a_turn_running_under_an_agent_the_note_does_not_bind_never_reaches_his_phone',
          nFrames('ask').length === asksBeforeStray,
          JSON.stringify(nFrames('ask').slice(asksBeforeStray).map(f => f.text)),
        )
        // Withheld is not the same as handled. Nothing rejects the request on opencode, so the
        // agent that asked blocks on a keyboard that will never be drawn — for ever, on a wall
        // whose tree names another agent by default, which is the very configuration the fence was
        // written for. The session was already proved to be this conversation's own, so turning the
        // question down is this watcher's to do and nobody else's.
        check(
          'and_the_worker_is_not_left_blocked_on_a_keyboard_that_will_never_be_drawn',
          await until('the question to be turned down',
            () => nOc.posted.some(p => p.path.endsWith('/question/que_stray/reject')), 8000),
          JSON.stringify(nOc.posted.map(p => p.path)),
        )
        nOc.messages.set('msg_ours', { id: 'msg_ours', sessionID: BOUND, role: 'assistant', agent: 'kickoff-room-steering' })
        nOc.push({
          type: 'question.v2.asked',
          properties: {
            id: 'que_ours',
            sessionID: BOUND,
            tool: { messageID: 'msg_ours', callID: 'call_2' },
            questions: [{ question: 'Tag the release?', header: 'Release', options: [{ label: 'Tag it' }] }],
          },
        })
        check(
          'and_a_turn_running_under_the_agent_it_does_bind_is_shown',
          await until('the bound agent\'s question', () => nFrames('ask').some(f => f.text?.startsWith('Tag the release?')), 8000),
          JSON.stringify(nFrames('ask').map(f => f.text)),
        )

        // ── and when the agent CANNOT be told, the fence still holds ──────────────────────────
        //
        // The fence let a question through whenever the agent could not be established: no message
        // named in the event, a lookup answered with an error, a lookup that never answered, a
        // message carrying no agent. Every one of those is the fence's own subject saying nothing,
        // and showing the question anyway put a turn nobody has proved is this worker's on the
        // operator's phone under this project's name — the one authority the fence exists to hold.
        //
        // Two kinds of not knowing, and they are not the same fact. A lookup that failed can come
        // good a second later, so the question is KEPT and offered again, exactly as one asked
        // while the note could not be read is. An event that names no message, or a message that
        // names no agent, can never become knowable, so it is withheld for good, the worker is
        // turned down rather than left blocked, and he is told once.
        {
          const asksBeforeNoMessage = nFrames('ask').length
          const saysBeforeNoMessage = nFrames('say').length
          nOc.push({
            type: 'question.v2.asked',
            properties: {
              id: 'que_nomessage',
              sessionID: BOUND,
              questions: [{ question: 'Restore last night\'s backup?', header: 'Restore', options: [{ label: 'Restore it' }] }],
            },
          })
          await new Promise(r => setTimeout(r, 1500))
          check(
            'a_question_whose_event_names_no_message_to_look_up_is_never_shown_under_this_projects_name',
            nFrames('ask').length === asksBeforeNoMessage,
            JSON.stringify(nFrames('ask').slice(asksBeforeNoMessage).map(f => f.text)),
          )
          check(
            'and_a_question_whose_agent_can_never_be_told_is_turned_down_and_said_out_loud_once',
            (await until('the question to be turned down',
              () => nOc.posted.some(p => p.path.endsWith('/question/que_nomessage/reject')), 8000)) &&
              nFrames('say').length > saysBeforeNoMessage,
            `${JSON.stringify(nOc.posted.map(p => p.path).slice(-3))} · ${JSON.stringify(nFrames('say').slice(saysBeforeNoMessage).map(f => f.text))}`,
          )

          // The message is there and answers, and says nothing about which agent ran the turn. The
          // same permanent fact, one hop further on.
          const asksBeforeSilentMessage = nFrames('ask').length
          nOc.messages.set('msg_silent', { id: 'msg_silent', sessionID: BOUND, role: 'assistant' })
          nOc.push({
            type: 'question.v2.asked',
            properties: {
              id: 'que_silentmessage',
              sessionID: BOUND,
              tool: { messageID: 'msg_silent', callID: 'call_3' },
              questions: [{ question: 'Drop the index?', header: 'Schema', options: [{ label: 'Drop it' }] }],
            },
          })
          check(
            'a_question_whose_message_says_nothing_about_which_agent_ran_the_turn_is_never_shown_either',
            (await until('that question to be turned down',
              () => nOc.posted.some(p => p.path.endsWith('/question/que_silentmessage/reject')), 8000)) &&
              nFrames('ask').length === asksBeforeSilentMessage,
            JSON.stringify(nFrames('ask').slice(asksBeforeSilentMessage).map(f => f.text)),
          )

          // A lookup the server would not answer — 404 here, and a 500 or a proxy's 502 is the same
          // fact. It can come good, so the question is kept: not shown, and not turned down either,
          // because the worker may yet be told to carry on with it.
          const asksBefore404 = nFrames('ask').length
          nOc.push({
            type: 'question.v2.asked',
            properties: {
              id: 'que_lookupfailed',
              sessionID: BOUND,
              tool: { messageID: 'msg_notheldyet', callID: 'call_4' },
              questions: [{ question: 'Cut the release?', header: 'Release', options: [{ label: 'Cut it' }] }],
            },
          })
          await new Promise(r => setTimeout(r, 2500))
          check(
            'a_question_the_server_would_not_answer_the_agent_lookup_for_is_kept_rather_than_shown_or_turned_down',
            nFrames('ask').length === asksBefore404 &&
              !nOc.posted.some(p => p.path.endsWith('/question/que_lookupfailed/reject')),
            `${nFrames('ask').length - asksBefore404} ask(s) · ${JSON.stringify(nOc.posted.map(p => p.path).slice(-3))}`,
          )
          // And kept means offered again: the server holds the message a moment later, it says the
          // agent the note binds, and the question the operator would never have seen appears.
          nOc.messages.set('msg_notheldyet', { id: 'msg_notheldyet', sessionID: BOUND, role: 'assistant', agent: 'kickoff-room-steering' })
          check(
            'and_the_question_kept_while_the_agent_could_not_be_told_is_shown_once_the_server_says_it',
            await until('the kept question', () => nFrames('ask').some(f => String(f.ask_id).endsWith('qque_lookupfailed')), 20000),
            JSON.stringify(nFrames('ask').slice(asksBefore404).map(f => f.ask_id)),
          )

          // The worst of the four, because it ends in the fence's own case: the lookup times out,
          // and the turn it could not ask about was another agent's all along. Shown on the timeout,
          // this is another worker's keyboard on his phone under this project's name.
          const asksBeforeSlow = nFrames('ask').length
          nOc.messages.set('msg_slow', { id: 'msg_slow', sessionID: BOUND, role: 'assistant', agent: 'coordinator' })
          nOc.delayNextMessageMs = 11_000 // past the ten seconds any one request here is given
          nOc.push({
            type: 'question.v2.asked',
            properties: {
              id: 'que_slowlookup',
              sessionID: BOUND,
              tool: { messageID: 'msg_slow', callID: 'call_5' },
              questions: [{ question: 'Wipe the staging bucket?', header: 'Destructive', options: [{ label: 'Wipe it' }] }],
            },
          })
          const turnedDownWhenTold = await until('the slow lookup to be decided on',
            () => nOc.posted.some(p => p.path.endsWith('/question/que_slowlookup/reject')), 40000)
          check(
            'a_lookup_that_timed_out_never_lets_another_agents_question_through_when_the_answer_arrives',
            turnedDownWhenTold && !nFrames('ask').some(f => String(f.ask_id).endsWith('qque_slowlookup')),
            `${JSON.stringify(nFrames('ask').slice(asksBeforeSlow).map(f => f.ask_id))} · ${JSON.stringify(nOc.posted.map(p => p.path).slice(-3))}`,
          )

          // ── and a server that is merely SLOW must not put one question on his phone twice ────
          //
          // A re-offer is queued behind every event already waiting, so it can be in flight for as
          // long as the lookup takes — and the timer that started it was cleared the moment it
          // fired. Any question kept in that window starts a second timer, which picks the oldest
          // kept question, which is still the one being offered. Two keyboards for one question:
          // two of the twenty sends a minute, and his second tap answered "the worker no longer has
          // that question open", which he can see is untrue because he has just answered it.
          {
            const asksBeforeSlowServer = nFrames('ask').length
            // Well inside the ten seconds one request is given, and longer than the five between
            // one re-offer and the next. Neither message is on the server yet, so the first look at
            // each is the "could not find out" that keeps the question.
            nOc.messageDelayMs = 6000
            const askSlowly = (id: string, messageID: string, text: string) =>
              nOc.push({
                type: 'question.v2.asked',
                properties: {
                  id,
                  sessionID: BOUND,
                  tool: { messageID, callID: `call_${id}` },
                  questions: [{ question: text, header: 'Slow', options: [{ label: 'go on' }] }],
                },
              })
            askSlowly('que_twice1', 'msg_twice1', 'Shall the first one be done?')
            await new Promise(r => setTimeout(r, 50))
            askSlowly('que_twice2', 'msg_twice2', 'Shall the second one be done?')
            // While the first one's re-offer is queued, the server starts holding its message, so
            // that re-offer succeeds and the question is one he SHOULD see — exactly once.
            await new Promise(r => setTimeout(r, 11_500))
            nOc.messages.set('msg_twice1', { id: 'msg_twice1', sessionID: BOUND, role: 'assistant', agent: 'kickoff-room-steering' })
            await until('the kept question to be offered',
              () => nFrames('ask').some(f => String(f.ask_id).endsWith('qque_twice1')), 25000)
            await new Promise(r => setTimeout(r, 12_000))
            const shown = nFrames('ask').filter(f => String(f.ask_id).endsWith('qque_twice1'))
            check(
              'a_question_kept_while_the_server_was_slow_is_offered_to_him_exactly_once',
              shown.length === 1,
              `${shown.length} ask frame(s) for the one question: ${JSON.stringify(nFrames('ask').slice(asksBeforeSlowServer).map(f => f.ask_id))}`,
            )
            nOc.messageDelayMs = 0
            nOc.messages.set('msg_twice2', { id: 'msg_twice2', sessionID: BOUND, role: 'assistant', agent: 'kickoff-room-steering' })
            // Left in a state a later block can build on: the second one goes through, which is
            // also what clears the record of what he has already been told.
            await until('the other kept question', () => nFrames('ask').some(f => String(f.ask_id).endsWith('qque_twice2')), 25000)
          }

          // ── and a permission nothing can place is turned down too, not left hanging ─────────
          //
          // This branch used to withhold a permission request and stop there, on the ground that
          // refusing one is the operator's own No to a tool call he was never shown. But withheld
          // is not handled: nothing else answers the request, so the tool call that asked for it
          // waits for ever — and it waits on exactly the wall this fence was written for, where
          // every `permission.v2.asked` names no message to look up and so lands here. Turning it
          // down is the only answer that can be given without showing it to him: it can never let
          // an action happen, it is what the sibling branch already does when the turn is placed
          // under somebody else's agent, and he is told in the same breath that it was given.
          //
          // RED, before the fix (the three paths are the three QUESTIONS that were turned down;
          // nothing at all was posted about the permission):
          //   FAIL a_permission_whose_turn_nothing_can_place_is_turned_down_rather_than_left_waiting_for_ever
          //        [… "/question/que_nomessage/reject", … "/question/que_silentmessage/reject", … "/question/que_slowlookup/reject"]
          //   FAIL and_the_word_it_is_turned_down_with_is_the_one_the_workers_server_takes
          //        []
          //   FAIL and_it_is_withheld_and_he_is_told_it_was_turned_down_rather_than_that_it_is_still_waiting
          //        0 ask(s) · ["… Nothing here has answered it, because answering it here would be
          //        answering for you. The worker is still waiting."]
          {
            const asksBeforePermission = nFrames('ask').length
            const saysBeforePermission = nFrames('say').length
            nOc.push({
              type: 'permission.v2.asked',
              properties: { id: 'per_unplaceable', sessionID: BOUND, action: 'run a command', resources: ['rm -rf /'] },
            })
            check(
              'a_permission_whose_turn_nothing_can_place_is_turned_down_rather_than_left_waiting_for_ever',
              await until('the permission to be turned down',
                () => nOc.posted.some(p => p.path.endsWith('/permission/per_unplaceable/reply')), 8000),
              JSON.stringify(nOc.posted.map(p => p.path).slice(-3)),
            )
            // The word is opencode's own, from the closed set of three its reply endpoint takes. A
            // fourth invented here is a request the server answers 400 to, and the worker would go
            // on waiting with the topic saying it had been released.
            check(
              'and_the_word_it_is_turned_down_with_is_the_one_the_workers_server_takes',
              nOc.posted.find(p => p.path.endsWith('/permission/per_unplaceable/reply'))?.body?.reply === 'reject',
              JSON.stringify(nOc.posted.filter(p => p.path.includes('per_unplaceable'))),
            )
            check(
              'and_it_is_withheld_and_he_is_told_it_was_turned_down_rather_than_that_it_is_still_waiting',
              nFrames('ask').length === asksBeforePermission &&
                nFrames('say').slice(saysBeforePermission).some(f =>
                  // The permission's OWN clause, not merely the words "turned down": a question's
                  // sentence carries those too, so grepping for them let the one sentence this
                  // change added be replaced by the question's and nothing go red.
                  /nothing here can allow what you were never shown/.test(String(f.text))) &&
                !nFrames('say').slice(saysBeforePermission).some(f => /still waiting/.test(String(f.text))),
              `${nFrames('ask').length - asksBeforePermission} ask(s) · ${JSON.stringify(nFrames('say').slice(saysBeforePermission).map(f => f.text))}`,
            )
          }

          // ── and the sentence about a turn-down says whether it was actually taken ────────────
          //
          // "It has been turned down, so the worker is not left waiting on it" was said whether the
          // server took the refusal or answered 500, and the two failures are correlated: the
          // branch that gives up on placing a turn is the branch reading an event shape nobody
          // understands, which is the same event whose id the refusal needs.
          {
            // A question that goes through first, which is what clears the record of what he has
            // already been told — otherwise the sentence under test is never reached.
            nOc.messages.set('msg_clears', { id: 'msg_clears', sessionID: BOUND, role: 'assistant', agent: 'kickoff-room-steering' })
            nOc.push({
              type: 'question.v2.asked',
              properties: {
                id: 'que_clears',
                sessionID: BOUND,
                tool: { messageID: 'msg_clears', callID: 'call_clears' },
                questions: [{ question: 'Anything at all?', header: 'H', options: [{ label: 'yes' }] }],
              },
            })
            await until('a question to go through', () => nFrames('ask').some(f => String(f.ask_id).endsWith('qque_clears')), 8000)

            const saysBeforeRefused = nFrames('say').length
            nOc.refusalStatus = 500
            nOc.push({
              type: 'question.v2.asked',
              properties: {
                id: 'que_norefusal',
                sessionID: BOUND,
                questions: [{ question: 'Roll the database forward?', header: 'Schema', options: [{ label: 'Roll it' }] }],
              },
            })
            const said = await until('the word about it',
              () => nFrames('say').length > saysBeforeRefused, 8000)
            const words = nFrames('say').slice(saysBeforeRefused).map(f => String(f.text)).join(' ')
            check(
              'a_turn_down_the_workers_server_would_not_take_is_not_reported_to_him_as_done',
              said && !words.includes('has been turned down'),
              words,
            )
            check(
              'and_he_is_told_the_worker_may_still_be_waiting_on_it',
              words.includes('may still be waiting'),
              words,
            )
            nOc.refusalStatus = 200
          }

          // ── and the second withheld request of a spell is still reported when it ends
          //    differently from the first ─────────────────────────────────────────────────────
          //
          // "Once per spell" is meant to be per SENTENCE: the one thing he would act on is whether
          // the worker was released or is still stopped, and one of those standing in for the
          // other is a false report. The key was the reason and the shape and nothing else, so the
          // second permission of a spell was swallowed however differently it ended — and on this
          // wall that is the steady state rather than an edge, because a permission never gets
          // through, so nothing ever clears the record of what he has been told.
          //
          // RED, before the fix:
          //   FAIL a_second_withheld_request_that_ends_differently_from_the_first_is_still_reported
          //        tried=true · []
          {
            const saysBeforeReleased = nFrames('say').length
            nOc.push({
              type: 'permission.v2.asked',
              properties: { id: 'per_released', sessionID: BOUND, action: 'read a file', resources: ['notes.md'] },
            })
            await until('the first permission of the spell', () =>
              nFrames('say').slice(saysBeforeReleased).some(f => /turned down/.test(String(f.text))), 8000)

            const saysBeforeStuck = nFrames('say').length
            nOc.refusalStatus = 500
            nOc.push({
              type: 'permission.v2.asked',
              properties: { id: 'per_stuck', sessionID: BOUND, action: 'run a command', resources: ['drop the table'] },
            })
            const tried = await until('the second permission to be refused', () =>
              nOc.posted.some(p => p.path.endsWith('/permission/per_stuck/reply')), 8000)
            check(
              'a_second_withheld_request_that_ends_differently_from_the_first_is_still_reported',
              tried &&
                nFrames('say').slice(saysBeforeStuck).some(f => /may still be waiting/.test(String(f.text))),
              `tried=${tried} · ${JSON.stringify(nFrames('say').slice(saysBeforeStuck).map(f => f.text))}`,
            )
            nOc.refusalStatus = 200
          }

          // ── a request that carries nothing to turn it down BY is not reported as one the
          //    worker's server refused ────────────────────────────────────────────────────────
          //
          // Nothing is sent at all in that case — there is no id to send it about — so telling him
          // the server would not take the refusal names a machine that was never asked, and sends
          // him to look at a server that is perfectly well.
          //
          // RED, before the fix:
          //   FAIL a_request_with_nothing_to_turn_it_down_by_is_not_blamed_on_the_workers_server
          //        posted=0 · ["… The worker's server would not take the refusal, so it may still
          //        be waiting on it."]
          {
            const saysBeforeNoId = nFrames('say').length
            const postedBeforeNoId = nOc.posted.length
            nOc.push({ type: 'permission.v2.asked', properties: { sessionID: BOUND, action: 'something' } })
            const said = await until('a word about the request that named nothing', () =>
              nFrames('say').length > saysBeforeNoId, 8000)
            const words = nFrames('say').slice(saysBeforeNoId).map(f => String(f.text)).join(' ')
            check(
              'a_request_with_nothing_to_turn_it_down_by_is_not_blamed_on_the_workers_server',
              said &&
                !/would not take the refusal/.test(words) &&
                /may still be waiting/.test(words) &&
                nOc.posted.length === postedBeforeNoId,
              `posted=${nOc.posted.length - postedBeforeNoId} · ${JSON.stringify(nFrames('say').slice(saysBeforeNoId).map(f => f.text))}`,
            )
          }

          // ── the older event shapes are turned down at the endpoints THEY have ───────────────
          //
          // Two of the four turn-down URLs had never been driven by anything. The fake answers any
          // POST, so a wrong path here would have looked exactly like a turn-down that worked, and
          // the operator would have been told a worker was released while it went on waiting.
          {
            const postedBeforeV1 = nOc.posted.length
            nOc.push({
              type: 'permission.asked',
              properties: { id: 'per_v1', sessionID: BOUND, action: 'read a file', resources: ['old.md'] },
            })
            const permV1 = await until('the older permission shape to be turned down', () =>
              nOc.posted.slice(postedBeforeV1).some(p => p.path === '/permission/per_v1/reply'), 8000)
            nOc.push({
              type: 'question.asked',
              properties: {
                id: 'que_v1',
                sessionID: BOUND,
                questions: [{ question: 'Old shape?', header: 'H', options: [{ label: 'yes' }] }],
              },
            })
            check(
              'the_event_shapes_from_before_v2_are_turned_down_at_the_endpoints_those_shapes_have',
              permV1 &&
                (await until('the older question shape to be turned down', () =>
                  nOc.posted.slice(postedBeforeV1).some(p => p.path === '/question/que_v1/reject'), 8000)),
              JSON.stringify(nOc.posted.slice(postedBeforeV1).map(p => p.path)),
            )
          }

          // ── a permission whose own session the note says is gone is turned down too ─────────
          //
          // One branch upstream of the fence: the note names this worker's session and the server
          // says that session has been archived. Nothing was turned down there and the sentence he
          // got ended with no statement of what became of the request at all, so a launcher that
          // archives a session — or a note left stale by a restart — while a tool call sits on a
          // permission left that call waiting for ever with nothing on the phone saying so.
          //
          // Only when the refusal is about the very session the request came from. A note naming
          // ANOTHER project's session, or a helper's, refuses for a reason that says the request is
          // not ours to answer, and turning one of those down would be answering in a conversation
          // nobody here is part of.
          //
          // RED, before the fix:
          //   FAIL a_permission_from_the_very_session_the_note_says_is_archived_is_turned_down
          //        [] · ["The worker asked something and it cannot be shown here — the session
          //        named for this worker has been archived."]
          {
            const saysBeforeArchived = nFrames('say').length
            const postedBeforeArchived = nOc.posted.length
            nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115, { time: { created: 1, updated: 1788607585115, archived: 1788607586000 } })]
            nOc.push({
              type: 'permission.v2.asked',
              properties: { id: 'per_archived', sessionID: BOUND, action: 'run a command', resources: ['ls'] },
            })
            const turned = await until('the permission in the archived session to be turned down', () =>
              nOc.posted.slice(postedBeforeArchived).some(p => p.path.endsWith('/permission/per_archived/reply')), 8000)
            check(
              'a_permission_from_the_very_session_the_note_says_is_archived_is_turned_down',
              turned &&
                nFrames('say').slice(saysBeforeArchived).some(f => /has been archived/.test(String(f.text)) && /turned down/.test(String(f.text))),
              `${JSON.stringify(nOc.posted.slice(postedBeforeArchived).map(p => p.path))} · ${JSON.stringify(nFrames('say').slice(saysBeforeArchived).map(f => f.text))}`,
            )
          }

          // And a refusal that says the request is somebody ELSE's is still left alone: turning it
          // down would be this project answering inside another project's conversation.
          {
            const postedBeforeElsewhere = nOc.posted.length
            const saysBeforeElsewhere = nFrames('say').length
            nOc.sessions = [listedSession(BOUND, elsewhere, 1788607585115)]
            nOc.push({
              type: 'permission.v2.asked',
              properties: { id: 'per_elsewhere', sessionID: BOUND, action: 'run a command', resources: ['ls'] },
            })
            await until('a word about the session in another project', () =>
              nFrames('say').slice(saysBeforeElsewhere).some(f => /different project/.test(String(f.text))), 8000)
            check(
              'but_a_request_the_note_says_belongs_to_another_project_is_never_answered_from_here',
              !nOc.posted.slice(postedBeforeElsewhere).some(p => p.path.includes('per_elsewhere')),
              JSON.stringify(nOc.posted.slice(postedBeforeElsewhere).map(p => p.path)),
            )
            nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
          }
        }
      }

      // Read at delivery time, never cached: the launcher rewrites the note and the NEXT line goes
      // to the new session with nothing restarted.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115), listedSession(OTHER, noteRepo, 1788607580000)]
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      const beforeFirst = prompts().length
      typeAt('first line')
      await until('the first line', () => prompts().length > beforeFirst, 8000)
      const firstPath = prompts().at(-1)?.path
      writeNote(JSON.stringify({ version: 1, session_id: OTHER }))
      const beforeSecond = prompts().length
      typeAt('second line')
      check(
        'a_session_note_replaced_between_two_lines_takes_effect_on_the_second_with_nothing_restarted',
        (await until('the second line', () => prompts().length > beforeSecond, 8000)) &&
          firstPath === `/session/${BOUND}/prompt_async` &&
          prompts().at(-1)?.path === `/session/${OTHER}/prompt_async`,
        `${firstPath} then ${prompts().at(-1)?.path}`,
      )

      // A generation is how a launcher says which note is newer. A wall that lost a race and
      // rewrites the note with an older one must not take the conversation back.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, generation: 7 }))
      const beforeFenced = prompts().length
      typeAt('under the seventh')
      await until('the fenced line', () => prompts().length > beforeFenced, 8000)
      const atSeven = prompts().at(-1)?.path
      writeNote(JSON.stringify({ version: 1, session_id: OTHER, generation: 6 }))
      const beforeStaleGen = prompts().length
      const wentBack = await reasonFor(typeAt('under the sixth'))
      check(
        'a_session_note_that_goes_backwards_in_generation_cannot_retarget_a_newer_one',
        atSeven === `/session/${BOUND}/prompt_async` &&
          /older than the one already in use/.test(wentBack) &&
          prompts().length === beforeStaleGen,
        `${atSeven} · ${wentBack} · ${prompts().length - beforeStaleGen} prompt(s)`,
      )
      writeNote(JSON.stringify({ version: 1, session_id: OTHER, generation: 8 }))
      const beforeEight = prompts().length
      typeAt('under the eighth')
      check(
        'and_a_generation_ahead_of_it_does_retarget',
        (await until('the eighth line', () => prompts().length > beforeEight, 8000)) &&
          prompts().at(-1)?.path === `/session/${OTHER}/prompt_async`,
        String(prompts().at(-1)?.path),
      )

      // A question from the bound session, and a reply typed under it: the words still go to the
      // session that asked, which is the bound one, and the question stays open for his tap.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, generation: 9 }))
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115), listedSession(NEWER, noteRepo, 1788607590000)]
      const asksBefore = nFrames('ask').length
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_bound',
          sessionID: BOUND,
          questions: [{ question: 'Which one?', header: 'Pick', options: [{ label: 'Left', description: 'l' }, { label: 'Right', description: 'r' }] }],
        },
      })
      await until('the question from the bound session', () => nFrames('ask').length > asksBefore, 8000)
      const boundAsk = nFrames('ask').at(-1)!
      const beforeReply = prompts().length
      const retiredBefore = nFrames('ask_resolved').length
      typeAt('the left one, but only for staging', { in_reply_to_ask: boundAsk.ask_id })
      check(
        'a_reply_typed_under_an_open_question_still_goes_to_the_session_that_asked_it',
        (await until('the reply', () => prompts().length > beforeReply, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          nFrames('ask_resolved').length === retiredBefore &&
          !nOc.posted.some(p => p.path.includes('/question/que_bound/')),
        String(prompts().at(-1)?.path),
      )

      // An event from a session this conversation is not bound to is not this conversation's
      // question: relaying it would put another worker's keyboard on the operator's phone, and his
      // tap would answer into a session nobody bound.
      const asksBeforeStranger = nFrames('ask').length
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_stranger',
          sessionID: NEWER,
          questions: [{ question: 'Delete the staging database?', header: 'Destructive', options: [{ label: 'Delete it' }] }],
        },
      })
      nOc.push({ type: 'permission.v2.asked', properties: { id: 'per_stranger', sessionID: NEWER, action: 'run a command', resources: ['rm -rf /'] } })
      await new Promise(r => setTimeout(r, 600))
      check(
        'a_question_from_a_session_the_note_does_not_name_never_reaches_the_operator',
        nFrames('ask').length === asksBeforeStranger,
        JSON.stringify(nFrames('ask').slice(asksBeforeStranger).map(f => f.text)),
      )

      // A heartbeat says THIS conversation's worker is between turns. On a server shared by two
      // walls, another session going idle says nothing about this one, and letting it through has
      // the hub reading a worker as settled while it is still mid-turn.
      const beatsBefore = nFrames('beat').length
      nOc.push({ type: 'session.idle', properties: { sessionID: NEWER } })
      await new Promise(r => setTimeout(r, 400))
      const strangerBeats = nFrames('beat').length
      nOc.push({ type: 'session.idle', properties: { sessionID: BOUND } })
      check(
        'a_turn_ending_in_a_session_the_note_does_not_name_is_not_this_conversations_heartbeat',
        strangerBeats === beatsBefore &&
          (await until('the bound session\'s beat', () => nFrames('beat').length > strangerBeats, 8000)),
        `${strangerBeats - beatsBefore} stranger beat(s), ${nFrames('beat').length - strangerBeats} of its own`,
      )

      // And a tap for a question the bound session asked before a rollover must not be answered
      // into the session that is bound now, nor into the one that has stopped being bound.
      writeNote(JSON.stringify({ version: 1, session_id: OTHER, generation: 10 }))
      const postedBeforeTap = nOc.posted.length
      nHub.to({ v: 1, id: 'n-tap', t: 'choice', msg_id: 'mn-tap', ask_id: boundAsk.ask_id, option_id: boundAsk.options[0].option_id })
      await new Promise(r => setTimeout(r, 600))
      check(
        'a_tap_is_never_answered_into_a_session_the_note_no_longer_names',
        nOc.posted.length === postedBeforeTap,
        JSON.stringify(nOc.posted.slice(postedBeforeTap).map(p => p.path)),
      )

      // ── enforced BOTH ways ──────────────────────────────────────────────────────────────────
      //
      // Refusing his typed words for a session while drawing that same session's questions on his
      // phone is two rules, not one: the question arrives under this project's name and his tap is
      // posted into a session the very next typed line is refused for. Both directions ask the
      // same question of the server, so a note the delivery rule refuses is a note the drawing
      // rule refuses too.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, generation: 11 }))
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      const asksBeforeBoth = nFrames('ask').length
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_bothways',
          sessionID: BOUND,
          questions: [{ question: 'Which branch?', header: 'Pick', options: [{ label: 'main' }, { label: 'next' }] }],
        },
      })
      await until('the question while the binding holds', () => nFrames('ask').length > asksBeforeBoth, 8000)
      const bothAsk = nFrames('ask').at(-1)!
      // The same note, and now the server says that session is another project's work.
      nOc.sessions = [listedSession(BOUND, elsewhere, 1788607585115)]
      const asksBeforeRefused = nFrames('ask').length
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_elsewhere',
          sessionID: BOUND,
          questions: [{ question: 'Drop the production database?', header: 'Destructive', options: [{ label: 'Drop it' }] }],
        },
      })
      await new Promise(r => setTimeout(r, 800))
      check(
        'a_question_from_a_session_the_note_names_but_the_binding_refuses_never_reaches_the_operator',
        nFrames('ask').length === asksBeforeRefused,
        JSON.stringify(nFrames('ask').slice(asksBeforeRefused).map(f => f.text)),
      )
      const postedBeforeBoth = nOc.posted.length
      const saysBeforeBoth = nFrames('say').length
      nHub.to({ v: 1, id: 'n-tap-both', t: 'choice', msg_id: 'mn-tap-both', ask_id: bothAsk.ask_id, option_id: bothAsk.options[0].option_id })
      await new Promise(r => setTimeout(r, 800))
      check(
        'and_his_tap_is_never_posted_into_a_session_the_binding_refuses_his_words_for',
        nOc.posted.length === postedBeforeBoth,
        JSON.stringify(nOc.posted.slice(postedBeforeBoth).map(p => p.path)),
      )
      check(
        'and_he_is_told_his_tap_went_nowhere_rather_than_left_with_a_sent_that_was_not',
        nFrames('say').length > saysBeforeBoth &&
          /different project/.test(String(nFrames('say').at(-1)?.text ?? '')),
        JSON.stringify(nFrames('say').slice(saysBeforeBoth).map(f => f.text)),
      )

      // A launcher that rolls the session over WHILE a line is being checked. The note is read,
      // the server is asked, and the answer comes back to a note that has since changed: carrying
      // the words on the id read before the wait puts them in the session he has stopped talking
      // to, and acks it `accepted`.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, generation: 12 }))
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115), listedSession(OTHER, noteRepo, 1788607580000)]
      const beforeRace = prompts().length
      nOc.delayNextListingMs = 900
      const racedId = typeAt('deploy it')
      await new Promise(r => setTimeout(r, 300))
      writeNote(JSON.stringify({ version: 1, session_id: OTHER, generation: 13 }))
      const raced = await reasonFor(racedId)
      check(
        'a_note_replaced_while_a_line_is_being_checked_does_not_land_it_in_the_session_it_named_first',
        prompts().length === beforeRace && /moved to another session/.test(raced),
        `${raced} · ${JSON.stringify(prompts().slice(beforeRace).map(p => p.path))}`,
      )

      // A generation orders two NUMBERED launchers against each other. A note carrying no number
      // has made no claim to be newer, so it cannot be "older" than one that has — and reading it
      // as older muted the wall for the life of the process, with a sentence naming a note the
      // operator has never been told exists.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      const beforePlain = prompts().length
      const plainAgain = typeAt('back to the plain note')
      check(
        'a_launcher_that_goes_back_to_a_note_with_no_number_is_still_heard',
        (await until('the plain line', () => prompts().length > beforePlain, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          nAck(plainAgain)?.status === 'accepted',
        `${prompts().at(-1)?.path} · ${JSON.stringify(nAck(plainAgain) ?? null)}`,
      )

      // A launcher writes the note a beat before the server lists the session, sees the line
      // refused, and corrects it. It has no reason to raise the number — nothing ever took the
      // first one — so the fence must only close behind a note the server actually confirmed.
      writeNote(JSON.stringify({ version: 1, session_id: 'ses_notYetListed0000000000', generation: 20 }))
      const tooEarly = await reasonFor(typeAt('are you up'))
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, generation: 20 }))
      const beforeCorrected = prompts().length
      const correctedId = typeAt('now then')
      check(
        'a_note_corrected_at_the_same_number_after_the_server_refused_the_first_one_is_taken',
        /is not open on its server/.test(tooEarly) &&
          (await until('the corrected line', () => prompts().length > beforeCorrected, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          nAck(correctedId)?.status === 'accepted',
        `${tooEarly} · ${JSON.stringify(nAck(correctedId) ?? null)}`,
      )

      // The silent half of the boot window. His typed line in it is refused in his own words; a
      // question asked in it reached nobody at all, and the agent waited on a keyboard that was
      // never drawn. A note that cannot be read is not the note saying no — it is the launcher a
      // beat behind its engine — so the question is KEPT, and shown the moment the note reads.
      writeNote('not a session id at all')
      const saysBeforeUnreadable = nFrames('say').length
      const asksBeforeUnreadable = nFrames('ask').length
      nOc.push({
        type: 'permission.v2.asked',
        properties: { id: 'per_unreadable', sessionID: BOUND, action: 'run a command', resources: ['rm -rf /'] },
      })
      await new Promise(r => setTimeout(r, 800))
      check(
        'a_question_asked_while_the_note_cannot_be_read_is_kept_rather_than_dropped_in_silence',
        nFrames('ask').length === asksBeforeUnreadable && nFrames('say').length === saysBeforeUnreadable,
        `${nFrames('ask').length - asksBeforeUnreadable} ask(s), ${nFrames('say').length - saysBeforeUnreadable} line(s)`,
      )

      // But only so many, and only for so long. A wall whose note never mends must not grow a queue
      // of questions nobody will ever see, so past what it will hold he is told out loud — which is
      // what he was told at once before, for every one of them.
      for (const id of ['per_full1', 'per_full2', 'per_full3', 'per_full4', 'per_full5', 'per_full6', 'per_full7', 'per_full8']) {
        nOc.push({ type: 'permission.v2.asked', properties: { id, sessionID: BOUND, action: 'run a command', resources: ['rm -rf /'] } })
      }
      const toldAboutTheOnesItCannotHold = await until('a word about a question it cannot keep', () => nFrames('say').length > saysBeforeUnreadable, 12000)
      check(
        'and_a_question_it_has_no_room_left_to_keep_is_said_out_loud_rather_than_dropped_in_silence',
        toldAboutTheOnesItCannotHold && nFrames('ask').length === asksBeforeUnreadable,
        `${nFrames('ask').length - asksBeforeUnreadable} ask(s), ${JSON.stringify(nFrames('say').slice(saysBeforeUnreadable).map(f => f.text))}`,
      )

      // An older event shape that names no session at all cannot be matched against the note. It
      // must not vanish: the agent that asked is blocked, and a stderr line is not somewhere he
      // looks.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      const shown = await until('the kept questions to be shown', () => nFrames('ask').some(f => String(f.ask_id).endsWith('pper_unreadable')), 20000)
      check(
        'and_the_question_kept_while_the_note_could_not_be_read_is_shown_once_it_can_be',
        shown,
        JSON.stringify(nFrames('ask').slice(asksBeforeUnreadable).map(f => f.ask_id)),
      )
      // Everything else it held comes through too, and the counts below are only meaningful once
      // that has finished.
      await until('the rest of them', () => nFrames('ask').length >= asksBeforeUnreadable + 8, 30000)
      await new Promise(r => setTimeout(r, 1200))

      const saysBeforeNameless = nFrames('say').length
      const asksBeforeNameless = nFrames('ask').length
      nOc.push({
        type: 'question.asked',
        properties: { id: 'que_nameless', questions: [{ question: 'Ship it?', options: [{ label: 'Ship' }] }] },
      })
      await new Promise(r => setTimeout(r, 800))
      check(
        'a_question_that_does_not_say_which_session_it_came_from_is_said_out_loud_rather_than_dropped',
        nFrames('ask').length === asksBeforeNameless && nFrames('say').length > saysBeforeNameless,
        `${nFrames('ask').length - asksBeforeNameless} ask(s), ${nFrames('say').length - saysBeforeNameless} line(s)`,
      )

      // Defence in depth: the acceptance must not rest on the server having obeyed `directory=`.
      // A server that answers the filtered listing with somebody else's session would otherwise
      // have the operator steering another project's worker, and the sentence-picking listing
      // already applies the rule the acceptance skips.
      nOc.ignoreDirectoryFilter = true
      nOc.sessions = [listedSession(BOUND, elsewhere, 1788607585115)]
      const beforeIgnored = prompts().length
      const ignored = await reasonFor(typeAt('over there then'))
      nOc.ignoreDirectoryFilter = false
      check(
        'a_session_the_server_lists_for_this_project_that_says_it_belongs_to_another_is_still_refused',
        /belongs to a different project/.test(ignored) && prompts().length === beforeIgnored,
        `${ignored} · ${prompts().length - beforeIgnored} prompt(s)`,
      )

      // ── one shape, one version, and a closed set of keys ───────────────────────────────────
      //
      // The note is a typed binding, not an id on a line: it says which session, and it may narrow
      // that to a project, an agent and a numbered writing. A reader that accepts several shapes
      // has to guess what a launcher meant by the one it did not write, and the guess is exactly
      // what this flag exists to remove — so there is ONE shape, it says which version it is, and
      // anything else is refused out loud rather than read as far as it goes.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      writeNote(BOUND)
      const beforeOldForm = prompts().length
      const oldForm = await reasonFor(typeAt('the old way'))
      check(
        'a_binding_written_in_a_form_this_worker_does_not_know_is_refused_rather_than_read_anyway',
        /form this worker does not know/.test(oldForm) && prompts().length === beforeOldForm,
        `${oldForm} · ${prompts().length - beforeOldForm} prompt(s)`,
      )
      writeNote(JSON.stringify({ version: 2, session_id: BOUND }))
      const laterVersion = await reasonFor(typeAt('from a newer launcher'))
      check(
        'a_binding_at_a_version_this_worker_does_not_know_is_refused_rather_than_read_as_far_as_it_goes',
        /form this worker does not know/.test(laterVersion),
        laterVersion,
      )
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, must_be_titled: 'the room' }))
      const unknownKey = await reasonFor(typeAt('with a rule it cannot read'))
      check(
        'a_binding_carrying_a_key_this_worker_does_not_know_is_refused_rather_than_half_obeyed',
        /is not one it can read/.test(unknownKey),
        unknownKey,
      )
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      const beforeShape = prompts().length
      const shapeId = typeAt('and the shape it knows')
      check(
        'and_the_one_shape_it_does_know_is_taken',
        (await until('the line under the one shape', () => prompts().length > beforeShape, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          nAck(shapeId)?.status === 'accepted',
        `${prompts().at(-1)?.path} · ${JSON.stringify(nAck(shapeId) ?? null)}`,
      )

      // The launcher is another org's program, and its shape shipped first: `conversation`,
      // `canonical_project_dir`, `session_id`, `agent`, `generation`, `verified_at`. Those are the
      // names read here, verbatim, because a repository that carries two spellings of one thing is
      // a repository where the note a launcher writes and the note a reader wants drift apart in
      // silence — and the drift shows up as every typed line refused, in a room, at the worst
      // possible moment. `verified_at` is the launcher's own record: known, read past, acted on by
      // nothing here.
      //
      // The object below is the launcher's, key for key: it is what its writer emits, and it is
      // here so that the next time the two shapes part company it is a red test rather than a room
      // where every line the operator types is refused. The two really did part company once — the
      // launcher shipped `{v, session, directory}` against a reader wanting `{version, session_id,
      // canonical_project_dir}` — and the way that was found was a live boot.
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115, { agent: 'kickoff-room-steering' })]
      writeNote(JSON.stringify({
        conversation: ROOM,
        canonical_project_dir: noteRepo,
        session_id: BOUND,
        agent: 'kickoff-room-steering',
        generation: 30,
        verified_at: '2026-09-07T08:00:00Z',
        version: 1,
      }))
      const beforeWhole = prompts().length
      const wholeId = typeAt('the whole shape, as it is written')
      check(
        'the_whole_shape_the_launcher_writes_is_read_key_for_key_and_his_words_reach_that_session',
        (await until('the line under the whole shape', () => prompts().length > beforeWhole, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          nAck(wholeId)?.status === 'accepted',
        `${prompts().at(-1)?.path} · ${JSON.stringify(nAck(wholeId) ?? null)}`,
      )

      // A launcher pointing this wall at a SIBLING room's session. The id resolves, the directory
      // may even match, and the agent may be the right one — the only thing that says it is not
      // this worker's is the conversation the note was written for. This is the failure the whole
      // binding exists to refuse, and it could not be caught at all until the note carried a room.
      writeNote(JSON.stringify({ version: 1, conversation: ANOTHER_ROOM, session_id: BOUND, generation: 31 }))
      const beforeAnotherRoom = prompts().length
      const anotherRoom = await reasonFor(typeAt('this one is not yours'))
      check(
        'a_binding_written_for_another_rooms_conversation_is_refused_rather_than_spoken_to',
        /belongs to a different conversation/.test(anotherRoom) && prompts().length === beforeAnotherRoom,
        `${anotherRoom} · ${prompts().length - beforeAnotherRoom} prompt(s)`,
      )

      // The version is what makes a LATER shape refuse rather than be half-obeyed by this reader,
      // so a note with no version at all is not a note this can read — and the sentence whoever
      // wrote the launcher gets names the one key to add and what to set it to.
      writeNote(JSON.stringify({ conversation: ROOM, session_id: BOUND, generation: 32 }))
      const beforeNoVersion = prompts().length
      const noVersion = await reasonFor(typeAt('and this one says nothing about its form'))
      check(
        'a_binding_that_says_no_version_at_all_is_refused_rather_than_read_as_far_as_it_goes',
        /does not say which form it is written in/.test(noVersion) && prompts().length === beforeNoVersion,
        `${noVersion} · ${prompts().length - beforeNoVersion} prompt(s)`,
      )
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, generation: 33 }))

      // The note is a private thing between the launcher and this process. One anybody else on the
      // box can read is one anybody else could have WRITTEN, and a note somebody else wrote is his
      // typed words in a session of their choosing.
      chmodSync(notePath, 0o644)
      const beforeOpen = prompts().length
      const openToAll = await reasonFor(typeAt('anyone could have written that'))
      chmodSync(notePath, 0o600)
      check(
        'a_binding_file_anybody_else_could_have_written_is_refused_rather_than_obeyed',
        /could not be read/.test(openToAll) && prompts().length === beforeOpen,
        `${openToAll} · ${prompts().length - beforeOpen} prompt(s)`,
      )

      // ── the order events happen in is the order he sees ────────────────────────────────────
      //
      // Deciding whether a question is this conversation's asks the server, so drawing a keyboard
      // takes as long as the server does. Two questions asked at once must still reach his phone
      // in the order they were asked — the second keyboard belonging to the first question is a
      // wrong answer sent to an agent — so the delayed listing here must not let the second
      // overtake the first.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      const asksBeforeOrder = nFrames('ask').length
      nOc.delayNextListingMs = 900
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_first',
          sessionID: BOUND,
          questions: [{ question: 'First: which branch?', header: 'Pick', options: [{ label: 'main' }, { label: 'next' }] }],
        },
      })
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_second',
          sessionID: BOUND,
          questions: [{ question: 'Second: which port?', header: 'Pick', options: [{ label: '80' }, { label: '443' }] }],
        },
      })
      await until('both questions', () => nFrames('ask').length >= asksBeforeOrder + 2, 12000)
      const drawn = nFrames('ask').slice(asksBeforeOrder).map(f => String(f.text))
      check(
        'two_questions_asked_at_once_reach_his_phone_in_the_order_they_were_asked',
        drawn.length >= 2 && /^First/.test(drawn[0]) && /^Second/.test(drawn[1]),
        JSON.stringify(drawn),
      )

      // And the event that RETIRES a question can arrive while the question itself is still being
      // decided on. Handled out of order it finds nothing to retire and says nothing, and the
      // keyboard for something already answered stays live on his phone for ever.
      const asksBeforeRetire = nFrames('ask').length
      const retiredBeforeRace = nFrames('ask_resolved').length
      nOc.delayNextListingMs = 900
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_racing',
          sessionID: BOUND,
          questions: [{ question: 'Racing: ship it?', header: 'Pick', options: [{ label: 'Ship' }, { label: 'Wait' }] }],
        },
      })
      nOc.push({ type: 'question.v2.replied', properties: { requestID: 'que_racing', sessionID: BOUND } })
      const retired = await until('the retirement', () => nFrames('ask_resolved').length > retiredBeforeRace, 12000)
      check(
        'a_question_answered_at_the_keyboard_the_instant_it_was_asked_still_has_its_buttons_taken_off_his_phone',
        retired &&
          nFrames('ask').length === asksBeforeRetire + 1 &&
          String(nFrames('ask_resolved').at(-1)?.ask_id ?? '').endsWith('qque_racing'),
        `${nFrames('ask').length - asksBeforeRetire} ask(s), ${JSON.stringify(nFrames('ask_resolved').slice(retiredBeforeRace))}`,
      )

      // The fleet ratchet: the id the note names is a local fact about one wall, and nothing about
      // it is anybody else's business — it is never in a hello, an ask, an ack or a topic.
      check(
        'the_session_the_note_names_never_appears_on_the_wire',
        !nHub.got.some(f => JSON.stringify(f).includes(BOUND) || JSON.stringify(f).includes(OTHER)),
        JSON.stringify(nHub.got.filter(f => JSON.stringify(f).includes(BOUND) || JSON.stringify(f).includes(OTHER))),
      )

      // ── a question the machine could not decide on is not lost ────────────────────────────
      //
      // Deciding whether a question is this conversation's asks the server, and the server can be
      // slow, down, or answering nonsense — and the note can be a beat behind the engine that has
      // just booted. None of those is the note saying no; they are the machine not saying. Dropped,
      // the agent waits on a keyboard that never appears and nothing ever retries, which is the
      // dead keyboard this adapter exists to end, inverted.
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115)]
      const asksBeforeStall = nFrames('ask').length
      nOc.delayNextListingMs = 11_000 // past the ten seconds any one request here is given
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_stalled',
          sessionID: BOUND,
          questions: [{ question: 'Stalled: drop the staging database?', header: 'Destructive', options: [{ label: 'Drop it' }, { label: 'No' }] }],
        },
      })
      const offeredAgain = await until('the question to be offered again', () => nFrames('ask').some(f => String(f.ask_id).endsWith('qque_stalled')), 40000)
      check(
        'a_question_the_machine_could_not_decide_on_is_offered_again_once_it_can_be',
        offeredAgain,
        `${nFrames('ask').length - asksBeforeStall} ask(s): ${JSON.stringify(nFrames('ask').slice(asksBeforeStall).map(f => f.ask_id))}`,
      )
      const stalledAsk = nFrames('ask').find(f => String(f.ask_id).endsWith('qque_stalled'))
      const repliesBeforeStalledTap = nOc.posted.length
      if (stalledAsk) {
        nHub.to({ v: 1, id: 'n-tap-stalled', t: 'choice', msg_id: 'mn-tap-stalled', ask_id: stalledAsk.ask_id, option_id: stalledAsk.options[0].option_id })
      }
      check(
        'and_the_agent_that_asked_it_is_not_left_waiting_on_a_keyboard_nobody_ever_drew',
        stalledAsk !== undefined &&
          (await until('the answer to reach opencode', () => nOc.posted.length > repliesBeforeStalledTap, 8000)),
        JSON.stringify(nOc.posted.slice(repliesBeforeStalledTap).map(p => p.path)),
      )

      // ── his own reply, while the note is being rewritten ──────────────────────────────────
      //
      // A question this watcher drew is a question whose session was proved against the note when
      // the keyboard went up, and a reply typed under it is the one time the operator has said
      // which session he means. Refusing it because the note cannot be read at that instant tells
      // him to "try again in a moment" about a thing no moment of his will mend — and the question
      // is his, the session is open, and only the note is missing. A note that has MOVED ON is a
      // different fact and stays refused.
      const asksBeforeRewrite = nFrames('ask').length
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_open',
          sessionID: BOUND,
          questions: [{ question: 'Open: which branch?', header: 'Pick', options: [{ label: 'main' }, { label: 'next' }] }],
        },
      })
      await until('the open question', () => nFrames('ask').length > asksBeforeRewrite, 12000)
      const openAsk = nFrames('ask').at(-1)
      writeNote(null) // the launcher is mid-rewrite, or has not put it back yet
      const beforeRewriteReply = prompts().length
      const rewriteReply = typeAt('use next', { in_reply_to_ask: String(openAsk?.ask_id) })
      check(
        'a_reply_typed_under_a_question_this_watcher_holds_open_reaches_the_session_that_asked_it_while_the_note_is_gone',
        (await until('the reply', () => prompts().length > beforeRewriteReply, 8000)) &&
          prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          nAck(rewriteReply)?.status === 'accepted',
        `${prompts().at(-1)?.path} · ${JSON.stringify(nAck(rewriteReply) ?? null)}`,
      )
      const repliesBeforeOpenTap = nOc.posted.length
      if (openAsk) {
        nHub.to({ v: 1, id: 'n-tap-open', t: 'choice', msg_id: 'mn-tap-open', ask_id: openAsk.ask_id, option_id: openAsk.options[0].option_id })
      }
      check(
        'and_his_tap_on_it_reaches_that_session_too_rather_than_dying_with_the_note',
        openAsk !== undefined && (await until('the tap', () => nOc.posted.length > repliesBeforeOpenTap, 8000)),
        JSON.stringify(nOc.posted.slice(repliesBeforeOpenTap).map(p => p.path)),
      )

      // The agent has to travel with the QUESTION, not with the watcher. Held as one value for the
      // whole process — the agent of whichever note was last obeyed — it is paired at the hatch
      // with the OLDER session that asked: after a rollover onto a session running a different
      // agent, a reply typed under the old question goes into the old session naming the NEW
      // session's agent. That is a turn under an agent nothing ever bound to it, which is the one
      // failure the binding exists to refuse, caused by the fix for the note going missing.
      nOc.sessions = [
        listedSession(BOUND, noteRepo, 1788607585115, { agent: 'kickoff-room-steering' }),
        listedSession(OTHER, noteRepo, 1788607580000, { agent: 'coordinator' }),
      ]
      writeNote(JSON.stringify({ version: 1, session_id: BOUND, agent: 'kickoff-room-steering' }))
      const asksBeforeTwoAgents = nFrames('ask').length
      // The event names the message its tool call belongs to, as a real one does: with the note
      // binding an agent, a question whose turn cannot be placed is not shown at all.
      nOc.messages.set('msg_twoagents', { id: 'msg_twoagents', sessionID: BOUND, role: 'assistant', agent: 'kickoff-room-steering' })
      nOc.push({
        type: 'question.v2.asked',
        properties: {
          id: 'que_twoagents',
          sessionID: BOUND,
          tool: { messageID: 'msg_twoagents', callID: 'call_6' },
          questions: [{ question: 'Which agent asked this?', header: 'Pick', options: [{ label: 'this one' }] }],
        },
      })
      await until('the question from the bound agent', () => nFrames('ask').length > asksBeforeTwoAgents, 12000)
      const twoAgentsAsk = nFrames('ask').at(-1)!
      // The launcher rolls the wall over onto a session running a different agent, and that note is
      // obeyed once — which is all it takes to become "the last agent a note named".
      writeNote(JSON.stringify({ version: 1, session_id: OTHER, agent: 'coordinator' }))
      const beforeRollover = prompts().length
      typeAt('a line under the new agent')
      await until('the line under the new agent', () => prompts().length > beforeRollover, 8000)
      // Then the note goes missing, which is the only reason the hatch exists at all.
      writeNote(null)
      const beforeUnderOld = prompts().length
      typeAt('and back to the old question', { in_reply_to_ask: String(twoAgentsAsk.ask_id) })
      await until('the reply under the old question', () => prompts().length > beforeUnderOld, 8000)
      check(
        'a_reply_to_a_question_one_agent_asked_is_never_carried_into_that_session_under_another_agent',
        prompts().at(-1)?.path === `/session/${BOUND}/prompt_async` &&
          prompts().at(-1)?.body?.agent === 'kickoff-room-steering',
        `${prompts().at(-1)?.path} · agent ${JSON.stringify(prompts().at(-1)?.body?.agent)}`,
      )
      writeNote(JSON.stringify({ version: 1, session_id: BOUND }))

      // ── the fence must survive a restart ──────────────────────────────────────────────────
      //
      // The monotonic rule above lives in this process's memory, and a wall is restarted — by
      // systemd, by a crash, by a redeploy. A watcher that comes back with no memory of which
      // binding it had already obeyed takes a stale launcher's note from before the rollover as
      // the newest thing it has seen, which is the rollback the fence exists to refuse. So the
      // number the wall was STARTED for is a floor on the command line: absolute, unchanged for
      // the life of the process, and nothing below it is ever obeyed again.
      const helloesBefore = nHub.got.filter(f => f.t === 'hello').length
      nChild.kill()
      await until('the first watcher to let go of the claim', () => nHub.live === 0, 8000)
      nOc.sessions = [listedSession(BOUND, noteRepo, 1788607585115), listedSession(OTHER, noteRepo, 1788607580000)]
      writeNote(JSON.stringify({ version: 1, session_id: OTHER, generation: 22 }))
      const nRestarted = Bun.spawn(
        [
          'bun', join(import.meta.dir, 'main.ts'),
          '--opencode', nOc.url,
          '--opencode-binding-file', notePath,
          '--opencode-binding-generation', '22',
        ],
        {
          env: {
            ...process.env,
            KICKOFF_HUB_PROJECT_DIR: noteRepo,
            KICKOFF_HUB_SOCKET: join(noteDir, 'hub.sock'),
            KICKOFF_HUB_RELAY_SOCKET: join(noteDir, 'door2.sock'),
          },
          stdout: 'ignore',
          stderr: 'ignore',
        },
      )
      try {
        const cameBack = await until(
          'the restarted watcher',
          () => nHub.got.filter(f => f.t === 'hello').length > helloesBefore && nHub.live === 1,
          15000,
        )
        check('a_watcher_started_for_a_numbered_binding_takes_the_claim_as_before', cameBack)
        if (!cameBack) throw new Error('attach never came back with --opencode-binding-generation')
        writeNote(JSON.stringify({ version: 1, session_id: BOUND, generation: 21 }))
        const beforeRollback = prompts().length
        const rolledBack = await reasonFor(typeAt('take it back'))
        check(
          'a_binding_older_than_the_one_this_watcher_was_started_for_is_refused_though_this_process_never_saw_the_newer_one',
          /older than the one already in use/.test(rolledBack) && prompts().length === beforeRollback,
          `${rolledBack} · ${prompts().length - beforeRollback} prompt(s)`,
        )
        writeNote(JSON.stringify({ version: 1, session_id: BOUND }))
        const unnumbered = await reasonFor(typeAt('and this one'))
        check(
          'and_a_binding_that_does_not_say_how_new_it_is_cannot_be_taken_where_a_number_was_named_at_the_start',
          /does not say how new it is/.test(unnumbered),
          unnumbered,
        )
        writeNote(JSON.stringify({ version: 1, session_id: OTHER, generation: 22 }))
        const beforeAtTheFloor = prompts().length
        const atTheFloor = typeAt('and now then')
        check(
          'and_the_binding_the_watcher_was_started_for_is_the_one_his_words_reach',
          (await until('the line at the floor', () => prompts().length > beforeAtTheFloor, 8000)) &&
            prompts().at(-1)?.path === `/session/${OTHER}/prompt_async` &&
            nAck(atTheFloor)?.status === 'accepted',
          `${prompts().at(-1)?.path} · ${JSON.stringify(nAck(atTheFloor) ?? null)}`,
        )
        // This watcher was started the older way — a secret found by the upward walk, no
        // conversation told — so it is attached perfectly well and cannot say which conversation
        // it is. A note that names one is then a claim nobody can check, and an unprovable claim
        // is not a check: it is refused, rather than obeyed on the strength of the keys around it.
        writeNote(JSON.stringify({ version: 1, conversation: ROOM, session_id: OTHER, generation: 23 }))
        const beforeUnprovable = prompts().length
        const unprovable = await reasonFor(typeAt('whose room is this'))
        check(
          'a_binding_naming_a_conversation_a_wall_cannot_prove_is_its_own_is_refused_rather_than_obeyed',
          /cannot tell whether that is this one/.test(unprovable) && prompts().length === beforeUnprovable,
          `${unprovable} · ${prompts().length - beforeUnprovable} prompt(s)`,
        )

        // The same window, from the agent's side. Not being able to say which conversation this is
        // is a state that MENDS — the operator grants, or the launcher rewrites the note without a
        // claim nobody here can check — which is why the wall re-asks it on every line rather than
        // deciding once at boot. A question asked while it holds is therefore in exactly the state
        // a note that has not been written yet is in: nothing has been found out, so nothing has
        // been decided. Dropping it instead spends an agent's whole turn on a keyboard that is
        // never drawn, and says so to the operator once for however many questions are lost.
        const asksBeforeUnprovable = nFrames('ask').length
        nOc.push({
          type: 'permission.v2.asked',
          properties: { id: 'per_unprovable', sessionID: OTHER, action: 'run a command', resources: ['rm -rf /'] },
        })
        await new Promise(r => setTimeout(r, 1200))
        check(
          'a_question_asked_while_this_wall_cannot_say_which_conversation_it_is_is_not_shown_yet',
          nFrames('ask').length === asksBeforeUnprovable,
          `${nFrames('ask').length - asksBeforeUnprovable} ask(s)`,
        )
        writeNote(JSON.stringify({ version: 1, session_id: OTHER, generation: 23 }))
        check(
          'and_it_is_offered_again_once_the_note_stops_making_a_claim_this_wall_cannot_check',
          await until(
            'the kept question once the claim is gone',
            () => nFrames('ask').some(f => String(f.ask_id).endsWith('per_unprovable')),
            20000,
          ),
          `${nFrames('ask').length - asksBeforeUnprovable} ask(s)`,
        )

        // And the other half of "nothing was found out": his reply under a question this watcher
        // itself drew still reaches the session that asked it. That session was proved against the
        // note — conversation and all — when the keyboard went up; the question is his, the session
        // is open, and the only thing missing is a claim nobody here can check yet. Refusing him
        // there told him to try again in a moment about a thing no moment of his would mend, and
        // left the question he was answering open in front of him.
        const asksBeforeOpen = nFrames('ask').length
        nOc.push({
          type: 'question.v2.asked',
          properties: {
            id: 'que_unprovable',
            sessionID: OTHER,
            questions: [{ question: 'Unprovable: which branch?', header: 'Pick', options: [{ label: 'main' }, { label: 'next' }] }],
          },
        })
        await until('the question to draw', () => nFrames('ask').length > asksBeforeOpen, 12000)
        const unprovableAsk = nFrames('ask').at(-1)
        writeNote(JSON.stringify({ version: 1, conversation: ROOM, session_id: OTHER, generation: 24 }))
        const beforeUnprovableReply = prompts().length
        const unprovableReply = typeAt('use next', { in_reply_to_ask: String(unprovableAsk?.ask_id) })
        check(
          'and_his_reply_under_a_question_it_drew_still_reaches_that_session_while_the_claim_cannot_be_checked',
          (await until('the reply', () => prompts().length > beforeUnprovableReply, 8000)) &&
            prompts().at(-1)?.path === `/session/${OTHER}/prompt_async` &&
            nAck(unprovableReply)?.status === 'accepted',
          `${prompts().at(-1)?.path} · ${JSON.stringify(nAck(unprovableReply) ?? null)}`,
        )
      } finally {
        nRestarted.kill()
      }
    } catch (e) {
      check('the session note behaves as the contract says', false, String((e as Error)?.message ?? e))
    } finally {
      nChild.kill()
      nHub.stop()
      nOc.stop()
      rmSync(noteDir, { recursive: true, force: true })
    }
  }

  // ── the note as a path on a real filesystem ──────────────────────────────────────────────────
  //
  // Two properties of the reader itself, checked where they live rather than through the wire:
  // both are about what the path resolves to, and neither needs a hub, a server or a phone.
  console.log('\nthe note is read from a path, and a path can be anything')
  {
    const fsDir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'ocp-'))
    try {
      // A named pipe where the note should be. `readFileSync` on it blocks in the kernel until
      // somebody writes, and this process is single-threaded: the whole adapter — the event
      // stream, every typed line, and `--check` itself — stops for ever, while the process stays
      // alive holding the claim so nothing restarts it. Run in a child so a failure here cannot
      // wedge the suite that is proving it.
      const fifo = join(fsDir, 'fifo-note')
      Bun.spawnSync(['mkfifo', fifo])
      const probe = Bun.spawnSync(
        ['bun', '-e', `import { readBindingFile } from ${JSON.stringify(join(import.meta.dir, 'plan.ts'))}; console.log(JSON.stringify(readBindingFile(${JSON.stringify(fifo)})))`],
        { timeout: 5000, stdout: 'pipe', stderr: 'pipe' },
      )
      const answered = probe.stdout.toString().trim()
      check(
        'a_note_that_is_not_a_regular_file_is_refused_instead_of_wedging_the_process',
        /could not be read/.test(answered),
        answered.length ? answered : 'it never answered',
      )

      // The note itself, and the place it sits. Each of these is a way for somebody who is not the
      // launcher to say which session the operator is steering, and none of them is a thing this
      // process can tell apart from the launcher's own writing once it has read it — so they are
      // refused before a byte is read, not made sense of afterwards.
      const realNote = join(fsDir, 'a-real-note')
      writeFileSync(realNote, JSON.stringify({ version: 1, session_id: 'ses_theBoundOne00000000000' }), { mode: 0o600 })
      check(
        'a_binding_file_this_user_owns_that_nobody_else_can_read_is_the_one_shape_that_is_read',
        'binding' in readBindingFile(realNote),
        JSON.stringify(readBindingFile(realNote)),
      )
      // The one sentence a PERSON acts on. The operator is told only that the note cannot be read
      // — he has never been told the note exists — so the half naming the key to add goes where
      // whoever wrote the launcher looks: the journal, and `--check`. It is worth nothing unless it
      // says the key and the value, because "add a version" is a question, not an instruction.
      writeFileSync(realNote, JSON.stringify({ conversation: 'c-0d0d0d0d0d0d', session_id: 'ses_theBoundOne00000000000' }), { mode: 0o600 })
      const noVersionRead = readBindingFile(realNote)
      check(
        'a_binding_with_no_version_says_exactly_which_key_to_add_and_what_to_set_it_to',
        !('binding' in noVersionRead) &&
          noVersionRead.state === 'says no version' &&
          String(noVersionRead.why).includes('"version": 1'),
        JSON.stringify(noVersionRead),
      )
      writeFileSync(realNote, JSON.stringify({ version: 1, session_id: 'ses_theBoundOne00000000000' }), { mode: 0o600 })

      chmodSync(realNote, 0o640)
      check(
        'a_binding_file_a_group_can_read_is_refused_rather_than_trusted',
        !('binding' in readBindingFile(realNote)),
        JSON.stringify(readBindingFile(realNote)),
      )
      chmodSync(realNote, 0o600)
      const linkedNote = join(fsDir, 'a-linked-note')
      symlinkSync(realNote, linkedNote)
      check(
        'a_binding_file_that_is_a_link_to_another_file_is_refused_rather_than_followed',
        !('binding' in readBindingFile(linkedNote)),
        JSON.stringify(readBindingFile(linkedNote)),
      )
      // A directory anybody can write in is a directory anybody can put a note in, or move one out
      // of the way of their own.
      const openDir = join(fsDir, 'open-to-all')
      mkdirSync(openDir, { recursive: true })
      const noteInTheOpen = join(openDir, 'note')
      writeFileSync(noteInTheOpen, JSON.stringify({ version: 1, session_id: 'ses_theBoundOne00000000000' }), { mode: 0o600 })
      chmodSync(openDir, 0o777)
      check(
        'a_binding_file_in_a_directory_anybody_can_write_in_is_refused_rather_than_read',
        !('binding' in readBindingFile(noteInTheOpen)),
        JSON.stringify(readBindingFile(noteInTheOpen)),
      )
      chmodSync(openDir, 0o700)

      // The project reached through a symlink. opencode stores and reports the directory it
      // resolved, which is what a launcher copies into the note; attach was given the path on the
      // command line, which may be the link. Comparing the two as strings refuses the project for
      // ever, and the operator is told his words belong to a different project than the one he is
      // plainly in.
      const real = join(fsDir, 'repo')
      const link = join(fsDir, 'by-link')
      mkdirSync(real, { recursive: true })
      symlinkSync(real, link)
      check(
        'a_project_reached_through_a_symlink_is_the_same_project_as_the_one_it_points_at',
        sameDirectory(link, real) && sameDirectory(`${link}/`, real),
        `${link} vs ${real}`,
      )
      // And still not the same as a different directory that happens to sit beside it.
      const beside = join(fsDir, 'beside')
      mkdirSync(beside, { recursive: true })
      check(
        'and_a_different_directory_beside_it_is_still_a_different_project',
        !sameDirectory(link, beside),
        `${link} vs ${beside}`,
      )
    } finally {
      rmSync(fsDir, { recursive: true, force: true })
    }
  }
} finally {
  child.kill()
  hub.stop(true)
  oc.stop(true)
  rmSync(dir, { recursive: true, force: true })
}

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} FAILED`)
process.exit(failures === 0 ? 0 : 1)
