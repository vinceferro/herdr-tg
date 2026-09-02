#!/usr/bin/env bun
/**
 * The plugin, against a fake hub. The mirror image of the Rust side's
 * `an_ask_becomes_a_tap_becomes_a_choice`, which tests the hub against a fake bridge.
 *
 *     bun test-against-a-fake-hub.ts
 *
 * What is real: the plugin process, its MCP stdio handshake, the Unix socket, and the framing.
 * What is faked is the hub, because a test that needed a bot token would never run.
 *
 * Every bridge below is started the way Claude Code starts one — cwd is THIS directory, and the
 * project is named only by `CLAUDE_PROJECT_DIR`. That is not incidental. An earlier version of this
 * file handed the bridge its repo in a variable production sets nowhere, which is how a bridge that
 * could never find its secret passed every test it had.
 */

import { mkdtempSync, mkdirSync, writeFileSync, rmSync, existsSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'

const dir = mkdtempSync(join(tmpdir(), 'kc-'))
const sock = join(dir, 'hub.sock')
const repo = join(dir, 'repo')
mkdirSync(join(repo, '.kickoff'), { recursive: true })
writeFileSync(join(repo, '.kickoff', 'hub.token'), 'a'.repeat(64), { mode: 0o600 })
// A real working tree: the bridge searches upward for the secret and stops at the top of one, so a
// project that is not a repo would not exercise the search at all.
Bun.spawnSync(['git', '-C', repo, 'init', '-q'])

let failures = 0
const check = (name: string, ok: boolean, detail = '') => {
  if (ok) console.log(`  ok   ${name}`)
  else { console.log(`  FAIL ${name} ${detail}`); failures++ }
}

const until = async (what: string, cond: () => boolean, ms = 8000) => {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    if (cond()) return
    await Bun.sleep(25)
  }
  throw new Error(`timed out waiting for: ${what}`)
}

/**
 * One bridge, started as the plugin manifest starts it: `cwd` is the plugin directory, never the
 * project. Anything the bridge needs to know about the project it has to get from the environment.
 */
function startBridge(env: Record<string, string>) {
  const child = Bun.spawn(['bun', 'server.ts'], {
    cwd: import.meta.dir,
    env: { ...process.env, ...env },
    stdin: 'pipe',
    stdout: 'pipe',
    stderr: 'inherit',
  })
  const out: Record<string, any>[] = []
  ;(async () => {
    const dec = new TextDecoder()
    let acc = ''
    for await (const chunk of child.stdout as any) {
      acc += dec.decode(chunk)
      for (;;) {
        const nl = acc.indexOf('\n')
        if (nl < 0) break
        const l = acc.slice(0, nl)
        acc = acc.slice(nl + 1)
        if (l.trim()) { try { out.push(JSON.parse(l)) } catch { /* not json */ } }
      }
    }
  })()
  const to = (o: unknown) => child.stdin.write(JSON.stringify(o) + '\n')
  return { child, out, to }
}

async function handshake(b: ReturnType<typeof startBridge>) {
  b.to({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {
    protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'test', version: '0' } } })
  await until('the MCP handshake', () => b.out.some(l => l.id === 1))
  b.to({ jsonrpc: '2.0', method: 'notifications/initialized' })
}

/** What the bridge has told the agent in its own turn, on its own behalf. */
const noticesTo = (b: ReturnType<typeof startBridge>) =>
  b.out.filter(l => l.method === 'notifications/claude/channel' && l.params?.meta?.user === 'the channel itself')

/** Call one tool and give back what the agent would read. */
async function call(b: ReturnType<typeof startBridge>, id: number, name: string, args: unknown) {
  b.to({ jsonrpc: '2.0', id, method: 'tools/call', params: { name, arguments: args } })
  await until(`${name} to return`, () => b.out.some(l => l.id === id))
  const r = b.out.find(l => l.id === id)!.result
  return { text: String(r.content?.[0]?.text ?? ''), isError: r.isError === true }
}

// ── Part 1: what the tools say when the operator was NOT reached ──────────────────────────────
//
// The defect these pin: every tool returned success the moment its frame was handed to `send`,
// whether that wrote it to a socket, parked it in a queue for a link that had never once come up,
// or refused it outright. An agent read "asked", told the operator his phone had buzzed, and it
// had not — the bridge had no secret and had never connected to anything.
console.log('\nwith nothing to connect to:')

const strays = join(dir, 'not-a-project')
mkdirSync(strays, { recursive: true })

const orphan = startBridge({
  CLAUDE_PROJECT_DIR: strays,
  KICKOFF_HUB_SOCKET: join(dir, 'no-hub-here.sock'),
})
await handshake(orphan)

const askedInVain = await call(orphan, 10, 'ask',
  { text: 'Overwrite prod?', options: [{ id: 'y', label: 'Yes' }] })
check('a question asked by an unenrolled session never claims his phone buzzed',
  !/^asked \(/.test(askedInVain.text), askedInVain.text)
check('it says in plain words that he was not asked',
  /not asked/i.test(askedInVain.text), askedInVain.text)
check('it names the command that would fix it',
  askedInVain.text.includes('herdr-tg enroll'), askedInVain.text)
check('and a failure nothing can mend on its own comes back as an error',
  askedInVain.isError, askedInVain.text)

const saidInVain = await call(orphan, 11, 'reply', { text: 'halfway through' })
check('reply does not say it was said either', saidInVain.text !== 'said', saidInVain.text)
const doneInVain = await call(orphan, 12, 'done', { text: 'finished' })
check('done does not say it was sent either', doneInVain.text !== 'sent', doneInVain.text)
const offInVain = await call(orphan, 13, 'ask_resolved', { ask_id: 'a1', how: 'withdrawn' })
check('ask_resolved does not say buttons came off a phone that never had any',
  offInVain.text !== 'the buttons are coming off', offInVain.text)
orphan.child.kill()

// Enrolled, but the hub is not running. This one CAN mend itself, and the words have to say so:
// the frame is really queued and really will go out, so the agent must not be told to give up.
const waiting = startBridge({
  CLAUDE_PROJECT_DIR: repo,
  KICKOFF_HUB_SOCKET: join(dir, 'no-hub-here.sock'),
})
await handshake(waiting)
const queued = await call(waiting, 14, 'reply', { text: 'still going' })
check('a message queued while the hub is down is not reported as said',
  queued.text !== 'said', queued.text)
check('it says it is waiting rather than delivered',
  /waiting/i.test(queued.text), queued.text)
check('and a gap that can mend itself is not reported as an error',
  !queued.isError, queued.text)
waiting.child.kill()

// ── Part 2: the round trip, against a fake hub ────────────────────────────────────────────────
console.log('\nagainst a fake hub:')

const fromBridge: Record<string, any>[] = []
let bridge: any = null
let buf = ''

const hub = Bun.listen({
  unix: sock,
  socket: {
    open(s: any) { bridge = s },
    data(_s: any, chunk: any) {
      buf += chunk.toString()
      for (;;) {
        const nl = buf.indexOf('\n')
        if (nl < 0) break
        const line = buf.slice(0, nl)
        buf = buf.slice(nl + 1)
        if (line.trim()) fromBridge.push(JSON.parse(line))
      }
    },
    close() {},
    error() {},
  },
})

const toBridge = (o: Record<string, unknown>) => bridge.write(JSON.stringify(o) + '\n')

const b = startBridge({ CLAUDE_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: sock })
const toPlugin = b.to
const outLines = b.out
await handshake(b)

await until('hello', () => fromBridge.some(f => f.t === 'hello'))
const hello = fromBridge.find(f => f.t === 'hello')!
check('it says hello with the secret from the repo', hello.token === 'a'.repeat(64))
check('the envelope carries the protocol version', hello.v === 1)
check('hello carries no display name', !('name' in hello) && !('title' in hello))
check('hello names its own repo and pid', hello.repo === repo && typeof hello.pid === 'number')
check('it found that repo without being told its own directory',
  hello.repo !== import.meta.dir, hello.repo)

// The real hub admits a connection with exactly one `welcome` before anything else, and the bridge
// holds its frames until it arrives — so a fake hub that skips it is a fake the bridge would sit
// mute against forever.
toBridge({ v: 1, id: 'h-welcome', t: 'welcome', project: 'repo',
  limits: { max_frame_bytes: 65536, per_minute: 20 } })

toBridge({ v: 1, id: 'h-ping-1', t: 'ping' })
await until('pong', () => fromBridge.some(f => f.t === 'pong'))
check('a pong names the ping it answers', fromBridge.find(f => f.t === 'pong')!.ref === 'h-ping-1')

toPlugin({ jsonrpc: '2.0', id: 2, method: 'tools/list' })
await until('the tool list', () => outLines.some(l => l.id === 2))
const tools = outLines.find(l => l.id === 2)!.result.tools.map((t: any) => t.name).sort()
check('it offers reply, ask, done and ask_resolved',
  JSON.stringify(tools) === JSON.stringify(['ask','ask_resolved','done','reply']), JSON.stringify(tools))

const t0 = Date.now()
const asked = await call(b, 3, 'ask',
  { text: 'Overwrite deploy/prod.yaml?', options: [{ id: 'y', label: 'Yes' }, { id: 'n', label: 'No' }] })
check('ask returns without waiting for the operator', Date.now() - t0 < 2000)
check('and once the hub has the question, it says so', /^asked \(/.test(asked.text), asked.text)
await until('the ask frame', () => fromBridge.some(f => f.t === 'ask'))
const ask = fromBridge.find(f => f.t === 'ask')!
check('the question reaches the hub verbatim', ask.text === 'Overwrite deploy/prod.yaml?')
check('both options travel with it', ask.options?.length === 2 && ask.options[0].option_id === 'y')

const barred = await call(b, 4, 'ask', { text: 'ok?', options: [{ id: 'a|b', label: 'Yes' }] })
check('an option id containing "|" is refused', barred.isError)

toBridge({ v: 1, id: 'h9', t: 'choice', msg_id: 'm2', ask_id: ask.ask_id, option_id: 'y' })
await until('the choice notification', () => outLines.some(l => l.method === 'notifications/claude/channel'))
const note = outLines.find(l => l.method === 'notifications/claude/channel')!
check('the answer arrives as a channel message', note.params.meta.option_id === 'y')
check('and it names the question it answers', note.params.meta.ask_id === ask.ask_id)

toBridge({ v: 1, id: 'h10', t: 'message', msg_id: 'm3', text: 'try it with --dry-run first',
  from: { chat_id: -1, user_id: 1 } })
await until('the typed message',
  () => outLines.filter(l => l.method === 'notifications/claude/channel').length >= 2)
const typed = outLines.filter(l => l.method === 'notifications/claude/channel')[1]
check("the operator's words reach the agent verbatim",
  typed.params.content === 'try it with --dry-run first')

check('every tool tells the agent to read what it returns',
  outLines.find(l => l.id === 2)!.result.tools.every((t: any) => /Read what it returns/.test(t.description)),
  JSON.stringify(outLines.find(l => l.id === 2)!.result.tools.map((t: any) => t.name)))

// `how` goes on the wire, and hub-proto accepts exactly three spellings with no fallback. A fourth
// decodes to nothing, the hub drops it as one unreadable frame, and the buttons stay live on his
// phone — the stale keyboard this tool exists to remove — while the tool says they are coming off.
const misspelt = await call(b, 5, 'ask_resolved', { ask_id: ask.ask_id, how: 'Answered' })
check('a way of finishing a question that the hub cannot read is refused here', misspelt.isError, misspelt.text)
check('and nothing was put on the wire for it',
  !fromBridge.some(f => f.t === 'ask_resolved'), JSON.stringify(fromBridge.filter(f => f.t === 'ask_resolved')))

// The hub took the frame — which is all `send` can see — and then says the operator never got it.
// It used to say that to stderr, which is the channel that let the original defect run.
toBridge({ v: 1, id: 'h11', t: 'ack', ref: fromBridge.find(f => f.t === 'ask')!.id,
  delivered: 'no', why: 'too-fast' })
await until('the notice that he was not reached', () => noticesTo(b).length >= 1).catch(() => {})
const shed = String(noticesTo(b)[0]?.params?.content ?? '')
check('when the hub says he never got it, the agent is told in its own turn',
  /never got/.test(shed), shed)
check('and told that no answer to that question is coming',
  shed.includes(ask.ask_id) && /stop waiting/.test(shed), shed)

// The wire defines THREE delivery values and this bridge branched on two: anything that was not an
// explicit `no` counted as success, so `unseen` — the hub saying it put the message out and could
// not check whether it landed — read as "he got it". The agent's only record then said the operator
// had been asked and an answer was on its way, and no correction ever arrived.
const unconfirmed = await call(b, 6, 'ask',
  { text: 'Restart the database?', options: [{ id: 'y', label: 'Yes' }] })
check('the second question reaches the hub', /^asked \(/.test(unconfirmed.text), unconfirmed.text)
await until('the second ask frame', () => fromBridge.filter(f => f.t === 'ask').length >= 2)
const unsureAsk = fromBridge.filter(f => f.t === 'ask')[1]
toBridge({ v: 1, id: 'h12', t: 'ack', ref: unsureAsk.id, delivered: 'unseen' })
await until('the notice that the send could not be confirmed', () => noticesTo(b).length >= 2, 8000).catch(() => {})
const unsure = String(noticesTo(b)[1]?.params?.content ?? '')
check('a send the hub could not confirm tells the agent that an answer may never come',
  unsure !== '' && /could not confirm/i.test(unsure) &&
    unsure.includes(unsureAsk.ask_id) && /no answer/i.test(unsure), unsure)
check('and it does not claim he never got it either, because nobody knows that',
  unsure !== '' && !/never got/.test(unsure), unsure)
// The notice gives its own reason for not re-sending — two live menus for one question, only one of
// which can answer — and then told the agent to do exactly that, in the clause an agent skimming
// reads last. Worse than two menus: the hub writes no ledger record for a send it could not
// confirm, so if the first one DID land, its buttons are live, unregistered, and every tap on them
// is answered "I have no record of that question, so I will not answer it for you."
check('it tells the agent plainly NOT to ask the same question again',
  unsure !== '' && /do not ask it again/i.test(unsure), unsure)
check('and never offers asking again as a way forward',
  unsure !== '' && !/or ask (it )?again/i.test(unsure), unsure)
check('and it offers the one way back that is safe, which is plain words',
  unsure !== '' && /plain message|plain words/i.test(unsure), unsure)

// `clamped` is the one ack reason the hub only ever pairs with `delivered: yes`, so returning early
// on `yes` made it unreachable — the bridge carried a sentence for it that nothing could print. The
// operator sees the truncation on his phone; the agent believed it had delivered the whole thing.
const long = await call(b, 7, 'reply', { text: 'x'.repeat(200) })
check('the long message reaches the hub', !long.isError, long.text)
await until('the say frame', () => fromBridge.filter(f => f.t === 'say').length >= 1)
const longFrame = fromBridge.filter(f => f.t === 'say').at(-1)!
toBridge({ v: 1, id: 'h13', t: 'ack', ref: longFrame.id, delivered: 'yes', why: 'clamped' })
await until('the notice that it was shortened', () => noticesTo(b).length >= 3, 8000).catch(() => {})
const clipped = String(noticesTo(b)[2]?.params?.content ?? '')
check('a message the hub had to shorten says so, rather than reading as delivered in full',
  clipped !== '' && /clipped|shortened|too long/i.test(clipped), clipped)
check('and it still says he got something, because he did',
  clipped !== '' && !/never got|could not confirm/i.test(clipped), clipped)

b.child.kill()
hub.stop()

// ── Part 3: promises the bridge makes about frames it is holding ──────────────────────────────
console.log('\nabout what is waiting in line:')

/** A fake hub that records what reaches it, and answers `hello` however the test asks. */
function fakeHub(path: string, answer: (hello: any, s: any) => void) {
  const got: Record<string, any>[] = []
  let acc = ''
  const server = Bun.listen({
    unix: path,
    socket: {
      open() {},
      data(s: any, chunk: any) {
        acc += chunk.toString()
        for (;;) {
          const nl = acc.indexOf('\n')
          if (nl < 0) break
          const line = acc.slice(0, nl)
          acc = acc.slice(nl + 1)
          if (!line.trim()) continue
          const f = JSON.parse(line)
          got.push(f)
          if (f.t === 'hello') answer(f, s)
        }
      },
      close() {}, error() {},
    },
  })
  return { got, stop: () => server.stop(true) }
}

const welcome = (s: any) =>
  s.write(JSON.stringify({ v: 1, id: 'h-w', t: 'welcome', project: 'repo',
    limits: { max_frame_bytes: 65536, per_minute: 20 } }) + '\n')

// A question the agent was told was NEVER asked must not be sitting in a queue that will ask it.
// The recovery that message prescribes — `herdr-tg enroll` — is exactly what produces the welcome
// that would drain it, so the operator's own fix was what detonated it: his phone buzzed with a
// question the agent had given up on, nothing would ever take the buttons off it, and an answer
// came back for an ask that, as far as the agent knew, was never made.
const ghostSock = join(dir, 'ghost.sock')
let refuseOnce = true
const ghostHub = fakeHub(ghostSock, (_h, s) => {
  if (refuseOnce) { refuseOnce = false; s.write(JSON.stringify({ v: 1, id: 'r', t: 'refused', reason: 'unknown_project' }) + '\n'); s.end() }
  else welcome(s)
})
const ghost = startBridge({ CLAUDE_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: ghostSock })
await handshake(ghost)
const disowned = await call(ghost, 20, 'ask',
  { text: 'Delete the staging database?', options: [{ id: 'y', label: 'Yes' }] })
check('a question the hub refuses outright is reported as never asked', disowned.isError, disowned.text)
await until('a second connection the hub welcomes', () => ghostHub.got.filter(f => f.t === 'hello').length >= 2, 12000)
  .catch(() => {})
await Bun.sleep(1500)
check('a question the agent was told was never asked never reaches his phone later',
  !ghostHub.got.some(f => f.t === 'ask'), JSON.stringify(ghostHub.got.find(f => f.t === 'ask')))

// And a frame that WAS honestly queued, then stranded by a refusal nothing can mend, has its
// promise taken back where the agent will read it.
const strandedSock = join(dir, 'stranded.sock')
const stranded = startBridge({ CLAUDE_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: strandedSock })
await handshake(stranded)
const held = await call(stranded, 21, 'ask', { text: 'Overwrite prod?', options: [{ id: 'y', label: 'Yes' }] })
check('with the hub down the question is honestly reported as waiting', !held.isError && /waiting/.test(held.text), held.text)
const strandedHub = fakeHub(strandedSock, (_h, s) => {
  s.write(JSON.stringify({ v: 1, id: 'r', t: 'refused', reason: 'unknown_project' }) + '\n'); s.end()
})
await until('the promise being taken back', () => noticesTo(stranded).length >= 1, 15000).catch(() => {})
const takenBack = String(noticesTo(stranded)[0]?.params?.content ?? '')
check('a question left waiting for a link that will not come back is not left waiting in silence',
  /let go/.test(takenBack), takenBack)
check('and the agent is told to stop waiting for the answer',
  /stop waiting/.test(takenBack), takenBack)
check('the question really was let go, not delivered later',
  !strandedHub.got.some(f => f.t === 'ask'), JSON.stringify(strandedHub.got.map(f => f.t)))
stranded.child.kill()
strandedHub.stop()
ghost.child.kill()
ghostHub.stop()

// The queue's promise, kept. `Socket.write` in Bun returns how many bytes the kernel took and
// DROPS the rest, and the drain used to shift each frame off before knowing it had gone — so a
// backlog past the socket's send buffer was destroyed in silence, after every one of those frames
// had been reported as waiting in line and certain to go out.
const backlogSock = join(dir, 'backlog.sock')
const backlog = startBridge({ CLAUDE_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: backlogSock })
await handshake(backlog)
const body = 'z'.repeat(20_000)
let promised = 0
for (let i = 0; i < 64; i++) {
  const r = await call(backlog, 100 + i, 'reply', { text: `${String(i).padStart(2, '0')}:${body}` })
  if (!r.isError && /waiting/.test(r.text)) promised++
}
check('a full queue of messages is each reported as waiting, none as said', promised === 64, `${promised} of 64`)
const backlogHub = fakeHub(backlogSock, (_h, s) => welcome(s))
await until('the backlog to arrive', () => backlogHub.got.filter(f => f.t === 'say').length >= 64, 25000)
  .catch(() => {})
const arrived = backlogHub.got.filter(f => f.t === 'say')
check('and every one of them arrives when the link comes back', arrived.length === 64, `${arrived.length} of 64`)
check('whole, and in the order they were said',
  arrived.every((f, i) => f.text.startsWith(String(i).padStart(2, '0') + ':') && f.text.length === 20_003),
  arrived.map(f => f.text.slice(0, 3)).join(''))
backlog.child.kill()
backlogHub.stop()

// ── Part 4: lanes ──────────────────────────────────────────────────────────────────────────────
//
// kickoff runs several worktrees of one repo at once. A lane worktree holds NO `.kickoff/hub.token`
// — the secret is gitignored and never checked out into one — so the bridge stopped at the top of
// the working tree, found nothing, and every lane failed closed with "this project is not
// enrolled". Its project is the MAIN working tree of the same repository, which git names.

check('an ordinary session names no lane at all', !('lane' in hello),
  JSON.stringify(hello.lane))

// A real repository with a real linked worktree, because `--git-common-dir` is the fact this rests
// on and a fake of it would prove nothing.
const laneRepo = join(dir, 'lanerepo')
mkdirSync(join(laneRepo, '.kickoff'), { recursive: true })
writeFileSync(join(laneRepo, '.kickoff', 'hub.token'), 'b'.repeat(64))
const git = (...args: string[]) => {
  const r = Bun.spawnSync(['git', '-C', laneRepo, ...args], { stdout: 'ignore', stderr: 'ignore' })
  if (r.exitCode !== 0) { console.log(`git ${args.join(' ')} failed`); process.exit(1) }
}
git('init', '-q')
git('-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-q', '--allow-empty', '-m', 'x')
const laneDir = join(dir, 'lane-0902-201212-2783563')
git('worktree', 'add', '-q', laneDir, '-b', 'lane/x')
check('a lane worktree really has no secret of its own',
  !existsSync(join(laneDir, '.kickoff', 'hub.token')))

const laneSock = join(dir, 'lane.sock')
// A hub that echoes the lane it admitted, which is what the new one does. The echo is what tells
// the bridge its worktree got a conversation of its own rather than the project's.
const laneHub = fakeHub(laneSock, (h, s) =>
  s.write(JSON.stringify({ v: 1, id: 'h-w', t: 'welcome', project: 'repo', lane: h.lane,
    limits: { max_frame_bytes: 262144, max_text_chars: 3500, max_options: 8 } }) + '\n'))
const laneBridge = startBridge({ CLAUDE_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: laneSock })
await handshake(laneBridge)
await until('a lane to say hello', () => laneHub.got.some(f => f.t === 'hello'))
const laneHello = laneHub.got.find(f => f.t === 'hello')!
check('a session in a lane worktree finds the secret in the main working tree',
  laneHello.token === 'b'.repeat(64), laneHello.token?.slice(0, 8))
check("and it names itself by git's own name for the worktree",
  laneHello.lane === 'lane-0902-201212-2783563', JSON.stringify(laneHello.lane))
check('the repo it reports is the main tree, not the worktree',
  laneHello.repo === laneRepo, laneHello.repo)
laneBridge.child.kill()
laneHub.stop()

// A lane name the hub will not address is PERMANENT — the folder does not rename itself between
// attempts — so the agent has to be told to stop waiting. An unknown reason is treated as temporary
// on purpose, and a bad lane inheriting that would spin for ever saying nothing useful.
const badSock = join(dir, 'badlane.sock')
const badHub = fakeHub(badSock, (_h, s) =>
  s.write(JSON.stringify({ v: 1, id: 'h-r', t: 'refused', reason: 'bad_lane' }) + '\n'))
const badBridge = startBridge({ CLAUDE_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: badSock })
await handshake(badBridge)
await until('the hub to see the lane', () => badHub.got.some(f => f.t === 'hello'))
// Polled rather than slept on: the refusal is in flight, and what is under test is that once it
// lands the agent is told to STOP — never that it is queued behind a link that is coming back.
let badSaid = { text: '', isError: false }
for (let i = 0; i < 60 && !badSaid.isError; i++) {
  badSaid = await call(badBridge, 200 + i, 'reply', { text: 'anything' })
  if (!badSaid.isError) await Bun.sleep(50)
}
check('a lane the hub will not address is told for good rather than retried',
  badSaid.isError && /worktree/.test(badSaid.text) && !/waiting/i.test(badSaid.text), badSaid.text)
badBridge.child.kill()
badHub.stop()

// Two worktrees of one repo may have the SAME folder name — git dedupes only its own internal name,
// never the checkout path — and two worktrees that present one lane are one conversation: the
// second one's arrival evicts the first's claim and sweeps its still-open questions off his phone
// as "the session that asked this restarted". Neither tree restarted; they are different trees.
const twinRepo = join(dir, 'twinrepo')
mkdirSync(join(twinRepo, '.kickoff'), { recursive: true })
writeFileSync(join(twinRepo, '.kickoff', 'hub.token'), 'c'.repeat(64))
const twin = (...args: string[]) => {
  const r = Bun.spawnSync(['git', '-C', twinRepo, ...args], { stdout: 'ignore', stderr: 'ignore' })
  if (r.exitCode !== 0) { console.log(`git ${args.join(' ')} failed`); process.exit(1) }
}
twin('init', '-q')
twin('-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-q', '--allow-empty', '-m', 'x')
const twinA = join(dir, 'twin-a', 'wip')
const twinB = join(dir, 'twin-b', 'wip')
twin('worktree', 'add', '-q', twinA, '-b', 'wip-a')
twin('worktree', 'add', '-q', twinB, '-b', 'wip-b')

const twinSock = join(dir, 'twin.sock')
const twinHub = fakeHub(twinSock, (h, s) =>
  s.write(JSON.stringify({ v: 1, id: 'h-w', t: 'welcome', project: 'repo', lane: h.lane,
    limits: { max_frame_bytes: 262144, max_text_chars: 3500, max_options: 8 } }) + '\n'))
const twinNames: string[] = []
for (const [i, tree] of [twinA, twinB].entries()) {
  const b = startBridge({ CLAUDE_PROJECT_DIR: tree, KICKOFF_HUB_SOCKET: twinSock })
  await handshake(b)
  await until(`worktree ${i} to say hello`, () => twinHub.got.filter(f => f.t === 'hello').length > i)
  twinNames.push(twinHub.got.filter(f => f.t === 'hello')[i].lane)
  b.child.kill()
}
check('two worktrees checked out under one folder name are still two conversations',
  twinNames[0] !== twinNames[1], JSON.stringify(twinNames))
twinHub.stop()

// A channel plugin restarts only when its session does, so plugin-new against hub-old is the
// ORDINARY intermediate state of a rollout — and an old hub ignores an unknown field, so it admits
// a lane AS THE PROJECT ITSELF. Silently: the worktree takes the project's one claim, talks in the
// project's topic, and the project's own session is then refused. The echo is the only thing that
// can tell those two apart, so its absence has to be a refusal and not a shrug.
const oldSock = join(dir, 'oldhub.sock')
const oldHub = fakeHub(oldSock, (_h, s) => welcome(s))
const oldBridge = startBridge({ CLAUDE_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: oldSock })
await handshake(oldBridge)
await until('the old hub to admit the lane', () => oldHub.got.some(f => f.t === 'hello'))
let oldSaid = { text: '', isError: false }
for (let i = 0; i < 60 && !oldSaid.isError; i++) {
  oldSaid = await call(oldBridge, 400 + i, 'reply', { text: 'anything' })
  if (!oldSaid.isError) await Bun.sleep(50)
}
check('a worktree whose hub cannot give it a place of its own refuses rather than impersonating the project',
  oldSaid.isError && /restart/i.test(oldSaid.text), oldSaid.text)
oldBridge.child.kill()
oldHub.stop()

// The claim is per worktree now, so "another session for this project" names the wrong thing and
// its instruction — close that session — sends him to a session that is not the holder.
const heldSock = join(dir, 'held.sock')
const heldHub = fakeHub(heldSock, (_h, s) =>
  s.write(JSON.stringify({ v: 1, id: 'h-r', t: 'refused', reason: 'already_claimed' }) + '\n'))
const heldBridge = startBridge({ CLAUDE_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: heldSock })
await handshake(heldBridge)
await until('the hub to refuse the worktree', () => heldHub.got.some(f => f.t === 'hello'))
let heldSaid = { text: '', isError: false }
for (let i = 0; i < 60 && !/worktree/.test(heldSaid.text); i++) {
  heldSaid = await call(heldBridge, 600 + i, 'reply', { text: 'anything' })
  await Bun.sleep(50)
}
check('a worktree told its link is already held is told WHICH thing is holding it',
  /worktree/.test(heldSaid.text) && heldSaid.text.includes('lane-0902-201212-2783563'), heldSaid.text)
heldBridge.child.kill()
heldHub.stop()

rmSync(dir, { recursive: true, force: true })
console.log(`\n${failures === 0 ? 'all checks passed' : `${failures} FAILED`}`)
process.exit(failures === 0 ? 0 : 1)
