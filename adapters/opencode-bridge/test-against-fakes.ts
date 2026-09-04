/**
 * The real bridge, against a fake hub on a real Unix socket and a fake opencode over real HTTP.
 *
 * Nothing here is mocked inside the bridge's own process: it is spawned as `bun bridge.ts`, exactly
 * as it will run, and everything is observed from outside. A test that reached into the module
 * would prove the mapping and miss the two things that have actually broken this project — the
 * handshake, and what goes on the wire before `welcome`.
 *
 *     bun test-against-fakes.ts
 */

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
    if (req.method === 'POST') {
      posted.push({ path: url.pathname, body: await req.json() })
      return new Response('{}', { headers: { 'content-type': 'application/json' } })
    }
    return new Response('not found', { status: 404 })
  },
})

// ── run the real bridge ────────────────────────────────────────────────────────────────────────

const child = Bun.spawn(['bun', join(import.meta.dir, 'bridge.ts')], {
  env: {
    ...process.env,
    KICKOFF_HUB_PROJECT_DIR: repo,
    KICKOFF_HUB_SOCKET: sockPath,
    OPENCODE_URL: `http://127.0.0.1:${oc.port}`,
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
const ADAPTERS = [
  'plugins/kickoff-channel/server.ts',
  'adapters/fanin/fanin.ts',
  'adapters/opencode-bridge/bridge.ts',
]
const sources = ADAPTERS.map(f => ({ f, src: readFileSync(join(REPO_TOP, f), 'utf8') }))
// Three marks of a file that has started writing the wire again, and each was on the fork. Not
// `Bun.connect` itself: the relay opens one to knock on its OWN door and tell a leftover socket file
// apart from a second relay still answering, and that connection never speaks a frame.
const sharing = sources.filter(x => /from '[^']*hub-link\.ts'/.test(x.src)).map(x => x.f)
const answering = sources.filter(x => /t: 'pong'/.test(x.src)).map(x => x.f)
const redeclaring = sources
  .filter(x => /const (PROTOCOL_VERSION|MAX_FRAME_BYTES|MAX_PENDING)\s*=/.test(x.src))
  .map(x => x.f)
check(
  'every_adapter_speaks_the_same_wire_from_the_same_file',
  sharing.length === ADAPTERS.length && answering.length === 0 && redeclaring.length === 0,
  `sharing hub-link.ts: ${sharing.length} of ${ADAPTERS.length}; answering a ping themselves: ${answering.join(', ') || 'none'}; declaring the protocol themselves: ${redeclaring.join(', ') || 'none'}`,
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
  const skewChild = Bun.spawn(['bun', join(import.meta.dir, 'bridge.ts')], {
    env: {
      ...process.env,
      KICKOFF_HUB_PROJECT_DIR: repo,
      KICKOFF_HUB_SOCKET: skewSock,
      // Deliberately nowhere. This bridge is here for its HUB behaviour, and pointing it at the fake
      // opencode would make it the last subscriber to that one event stream — every event the tests
      // below push would then go to this process instead of the one under test.
      OPENCODE_URL: 'http://127.0.0.1:9',
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
  const claimChild = Bun.spawn(['bun', join(import.meta.dir, 'bridge.ts')], {
    env: {
      ...process.env,
      KICKOFF_HUB_PROJECT_DIR: repo,
      KICKOFF_HUB_SOCKET: claimSock,
      OPENCODE_URL: `http://127.0.0.1:${claimOc.port}`,
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
  const beforeBogus = posted.length
  tap(perm.ask_id, 'an-option-nobody-minted')
  await new Promise(r => setTimeout(r, 300))
  check('a tap naming an option it never offered posts nothing', posted.length === beforeBogus, `${posted.length - beforeBogus} did`)

  tap(perm.ask_id, 'reject')
  check('a tap replies at the permission endpoint', await until('the permission reply', () => posted.length > before + 0 && posted.some(p => p.path.includes('permission'))))
  const pr = posted.find(p => p.path.includes('permission'))
  check(
    'at the endpoint the spec names, carrying the ids opencode published',
    pr?.path === '/api/session/ses_1/permission/per_1/reply',
    pr?.path,
  )
  check('with the enum opencode accepts', JSON.stringify(pr?.body) === JSON.stringify({ reply: 'reject' }), JSON.stringify(pr?.body))

  console.log('\na question answered at the keyboard has its buttons taken off the phone')
  pushEvent!({
    type: 'question.v2.asked',
    properties: {
      id: 'que_2',
      sessionID: 'ses_1',
      questions: [{ question: 'Ship it?', header: 'Ship', options: [{ label: 'Yes', description: 'go' }] }],
    },
  })
  await until('the second question', () => frames('ask').length > 2)
  const retiredBefore = frames('ask_resolved').length
  // Deliberately the `data` shape: the durable stream uses it, and the bridge must read both.
  pushEvent!({ type: 'question.v2.replied', data: { sessionID: 'ses_1', requestID: 'que_2', answers: [['Yes']] } })
  check(
    'the keyboard is retired without anyone tapping it',
    await until('the second retirement', () => frames('ask_resolved').length > retiredBefore),
  )
} finally {
  child.kill()
  hub.stop(true)
  oc.stop(true)
  rmSync(dir, { recursive: true, force: true })
}

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} FAILED`)
process.exit(failures === 0 ? 0 : 1)
