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
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'

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
const hangNext = { session: false, prompt: false }
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

/** Push a tap down to the bridge, the way a real tap arrives. */
function tap(askId: string, optionId: string): void {
  hubSock?.write(
    JSON.stringify({ v: 1, id: 'h9', t: 'choice', msg_id: 'm1', ask_id: askId, option_id: optionId }) + '\n',
  )
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
  // The script below is what an ordinary box produces: herdr-tg restarting with a lingering claim,
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
  tap(ask.ask_id, ask.options[0].option_id)
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

  console.log('\na tap on a question that is already answered answers nothing twice')
  const before = posted.length
  tap(ask.ask_id, ask.options[0].option_id)
  await new Promise(r => setTimeout(r, 300))
  check('nothing more is posted to opencode', posted.length === before, `${posted.length - before} were`)

  console.log('\na permission request offers only the three answers opencode accepts')
  pushEvent!({
    id: 'evt_2',
    type: 'permission.v2.asked',
    properties: { id: 'per_1', sessionID: 'ses_1', action: 'run a command', resources: ['rm -rf /'] },
  })
  check('it arrives as an ask', await until('the permission ask', () => frames('ask').length > 1))
  const perm = frames('ask')[1]
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
  await until('the second permission ask', () => frames('ask').length > 2)
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
  // ack becomes a line in the topic he typed in — is `crates/herdr-tg/src/hub/tests.rs`.
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
} finally {
  child.kill()
  hub.stop(true)
  oc.stop(true)
  rmSync(dir, { recursive: true, force: true })
}

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} FAILED`)
process.exit(failures === 0 ? 0 : 1)
