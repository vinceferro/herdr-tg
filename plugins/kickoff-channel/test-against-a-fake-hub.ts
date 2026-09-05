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
function startBridge(env: Record<string, string>, cwd = import.meta.dir) {
  const child = Bun.spawn(['bun', join(import.meta.dir, 'server.ts')], {
    cwd,
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

/**
 * Claude Code introducing itself, CAPTURED from the real client rather than imagined.
 *
 * `claude mcp list` was pointed at a server that wrote down the `initialize` it received: version
 * 2.1.250 of Claude Code sends exactly this. It matters because the bridge now reads the client's
 * name to decide whether it may promise the agent that an answer is coming, and a fixture invented
 * here would only prove this file agrees with itself.
 */
const CLAUDE_CODE = {
  capabilities: { roots: { listChanged: true }, elicitation: {} },
  clientInfo: { name: 'claude-code', title: 'Claude Code', version: '2.1.250' },
}

/** opencode 1.18.25 introducing itself, captured the same way from a real `opencode serve`. */
const OPENCODE = { capabilities: { roots: {} }, clientInfo: { name: 'opencode', version: '1.18.25' } }

async function handshake(b: ReturnType<typeof startBridge>, who = CLAUDE_CODE) {
  b.to({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {
    protocolVersion: '2025-11-25', ...who } })
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
// The instructions block teaches ONE test — "not …" is queued, "NOT" is never — and this was the
// only tool that did not obey it, so an agent applying the rule it was taught found no marker here
// and had to read the sentence closely to learn nothing had happened.
//
// RED, before the fix: 'Nothing came off his phone. The hub does not know this project. …'
check('and it fails in the same first word every other tool fails in',
  /^NOT /.test(offInVain.text), offInVain.text)
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
// The same prefix rule, on the tool that used to be the one exception to it.
//
// RED, before the fix: 'the buttons are still on his phone. Taking them off is waiting in line …'
const offQueued = await call(waiting, 15, 'ask_resolved', { ask_id: 'a1', how: 'withdrawn' })
check('a retirement queued while the hub is down begins in the same two words a message does',
  /^not /.test(offQueued.text) && !offQueued.isError, offQueued.text)
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

/**
 * A fake hub that records what reaches it, and answers `hello` however the test asks — and every
 * later frame however `then` asks, for a test about what a hub does AFTER the welcome.
 */
function fakeHub(path: string, answer: (hello: any, s: any) => void, then?: (frame: any, s: any) => void) {
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
          else then?.(f, s)
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

// A frame the kernel took whole is off the queue the moment it is written, and the hub answers for
// it only on THIS connection: an ack names a frame by an id the next connection's hub has never
// seen. So a connection that ends with frames flushed and unanswered ends with frames nobody will
// ever answer for — and the bridge used to keep them on its books, in silence, for ever. The agent
// had been told "said"; no correction ever came. Measured against the real hub: fourteen frames
// flushed whole into two refused connections, zero corrections, nothing on the phone.
//
// The hub below does to the last two frames exactly what the shipped hub did to everything a
// bridge said before its pong — reads them, closes, answers for nothing — and to the first two
// what it does now, which is say `no` before closing. The agent has to learn of all four, and in
// two different registers, because "he never got it" and "nobody knows" are not the same fact.
const thrownSock = join(dir, 'thrown.sock')
const thrown = startBridge({ CLAUDE_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: thrownSock })
await handshake(thrown)
await call(thrown, 200, 'reply', { text: 'first' })
await call(thrown, 201, 'reply', { text: 'second' })
await call(thrown, 202, 'ask', { text: 'third: proceed?', options: [{ id: 'y', label: 'Yes' }] })
await call(thrown, 203, 'reply', { text: 'fourth' })
let read = 0
const thrownHub = fakeHub(thrownSock, (_h, s) => welcome(s), (f, s) => {
  if (f.t !== 'say' && f.t !== 'ask') return
  read++
  if (read <= 2) s.write(JSON.stringify({ v: 1, id: `h-a${read}`, t: 'ack', ref: f.id, delivered: 'no', why: 'too-fast' }) + '\n')
  if (read === 4) s.end()
})
await until('the agent to be told what became of the frames the hub never answered for',
  () => noticesTo(thrown).some(n => /before the hub said/.test(String(n.params?.content ?? ''))), 8000)
  .catch(() => {})
const unanswered = noticesTo(thrown).map(n => String(n.params?.content ?? '')).find(t => /before the hub said/.test(t)) ?? ''
const neverGot = noticesTo(thrown).map(n => String(n.params?.content ?? '')).filter(t => /He never got/.test(t))
check('a_bridge_learns_which_flushed_frames_the_hub_threw_away', unanswered !== '',
  JSON.stringify(noticesTo(thrown).map(n => n.params?.content)))
check('it says they may or may not have reached him, never that they did or that they did not',
  /may have arrived and it may not/.test(unanswered) && !/never got/.test(unanswered), unanswered)
check('it names what each of them was', /a message for him/.test(unanswered) && /a question \(a\d+\)/.test(unanswered), unanswered)
check('and tells the agent not to ask that question again, because its buttons may be live',
  /do not ask it again/i.test(unanswered), unanswered)
check('the two the hub refused before closing are reported as refused, separately',
  neverGot.length === 2, `${neverGot.length}: ${JSON.stringify(neverGot)}`)
await until('the bridge to come back', () => thrownHub.got.filter(f => f.t === 'hello').length >= 2, 6000).catch(() => {})
await Bun.sleep(300)
check('and none of the four is sent again, because a second copy of a question is two live menus',
  thrownHub.got.filter(f => f.t === 'say' || f.t === 'ask').length === 4,
  JSON.stringify(thrownHub.got.map(f => f.t)))
thrown.child.kill()
thrownHub.stop()

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
// Two causes wear this one name. The relay has no wire field for "the hub is older than me", so it
// folds that onto `bad_lane` too — and a sentence naming only the other cause tells whoever reads
// it to delete and recreate a git worktree, losing whatever is uncommitted in it, while the one
// action that actually mends it goes unmentioned.
//
// RED, before the fix: '… until the worktree is remade under a plainer one.' — and nothing else.
check('and it names restarting the hub, not only remaking the worktree',
  /herdr-tg/.test(badSaid.text) && /remade/.test(badSaid.text), badSaid.text)
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

// ── Part 5: the same server, started by the other engine ──────────────────────────────────────
//
// opencode declares this file under `mcp` and sets the MCP child's cwd to the session's own
// directory. It sets no `CLAUDE_PROJECT_DIR`, so the only way it can name the project is the cwd it
// already set — and a BARE cwd fallback is wrong under Claude Code in both plugin layouts, which is
// the defect that made every message an agent believed it had sent go nowhere. So the cwd term is a
// flag something had to set, and these check that the flag is the only thing that unlocks it.
console.log('\nwhichever engine started it:')

const neutralSock = join(dir, 'neutral.sock')
const neutralHub = fakeHub(neutralSock, (_h, s) => welcome(s))

// A decoy repository, enrolled and complete, for the checks that turn on WHICH directory won.
const decoy = join(dir, 'decoy')
mkdirSync(join(decoy, '.kickoff'), { recursive: true })
writeFileSync(join(decoy, '.kickoff', 'hub.token'), 'd'.repeat(64))
Bun.spawnSync(['git', '-C', decoy, 'init', '-q'])

// Claude Code sets its own variable and nothing else, and a plugin manifest cannot be edited from
// here — so a session that names its project only that way has to keep attaching. It is the engine's
// variable, read only when nothing in this interface's own namespace has spoken.
const claudeWay = startBridge({ CLAUDE_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: neutralSock }, decoy)
await handshake(claudeWay)
await until('the claude-way hello', () => neutralHub.got.some(f => f.t === 'hello'))
const claudeHello = neutralHub.got.find(f => f.t === 'hello')!
check('a_claude_session_keeps_working_exactly_as_it_did_before',
  claudeHello.repo === repo && claudeHello.token === 'a'.repeat(64),
  `${claudeHello.repo} / ${claudeHello.token?.slice(0, 4)}`)

// The one variable a normal adopter sets, and the only one that is required.
const namedWay = startBridge({ KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: neutralSock })
await handshake(namedWay)
await until('the named-way hello', () => neutralHub.got.filter(f => f.t === 'hello').length >= 2)
check('a_harness_that_names_the_project_directory_is_believed',
  neutralHub.got.filter(f => f.t === 'hello')[1].repo === repo)

// The dot, which is the whole of what "my cwd is the project, and I vouch for it" now costs. cwd
// here is a SUBFOLDER of the repo, because that is what opencode does when the session is opened on
// one — and the upward search is what turns it into the project.
const sub = join(repo, 'crates', 'deep')
mkdirSync(sub, { recursive: true })
const cwdWay = startBridge(
  { KICKOFF_HUB_PROJECT_DIR: '.', KICKOFF_HUB_SOCKET: neutralSock },
  sub,
)
await handshake(cwdWay)
await until('the cwd-way hello', () => neutralHub.got.filter(f => f.t === 'hello').length >= 3)
check('a_tool_server_told_its_cwd_is_the_project_finds_the_secret_by_searching_upward_from_it',
  neutralHub.got.filter(f => f.t === 'hello')[2].repo === repo,
  neutralHub.got.filter(f => f.t === 'hello')[2].repo)

// And WITHOUT the flag it still refuses to guess, which is the whole safety argument for the flag.
const guessing = startBridge({ KICKOFF_HUB_SOCKET: neutralSock }, repo)
await handshake(guessing)
const guessed = await call(guessing, 700, 'reply', { text: 'anything' })
check('a_tool_server_that_was_not_vouched_for_still_refuses_to_guess_its_own_project',
  guessed.isError && /never said which project/.test(guessed.text), guessed.text)
await Bun.sleep(400)
check('and it never said hello to anything',
  neutralHub.got.filter(f => f.t === 'hello').length === 3,
  String(neutralHub.got.filter(f => f.t === 'hello').length))
guessing.child.kill()

// MEASURED ON THE MACHINE, not reasoned about: an opencode server started from inside a Claude
// Code session inherits `CLAUDE_PROJECT_DIR`, and its whole environment reaches the MCP child. The
// first term then wins and the child resolves ANOTHER repository's secret — silently, because it
// really does find one. opencode's config can only overlay a variable, never remove it, and a
// missing `{env:VAR}` there substitutes to the empty string, so "set to nothing" has to mean unset
// or there is no way to close this at all.
const blanked = startBridge(
  { CLAUDE_PROJECT_DIR: '', KICKOFF_HUB_PROJECT_DIR: '.', KICKOFF_HUB_SOCKET: neutralSock },
  repo,
)
await handshake(blanked)
await until('the blanked hello', () => neutralHub.got.filter(f => f.t === 'hello').length >= 4)
check('a_project_directory_the_config_blanked_out_is_treated_as_unset',
  neutralHub.got.filter(f => f.t === 'hello')[3].repo === repo,
  neutralHub.got.filter(f => f.t === 'hello')[3].repo)
blanked.child.kill()

// A lane worktree opened by opencode is still named by git, not by its folder.
const laneCwd = startBridge(
  { KICKOFF_HUB_PROJECT_DIR: '.', KICKOFF_HUB_SOCKET: neutralSock },
  laneDir,
)
await handshake(laneCwd)
await until('the lane-by-cwd hello', () => neutralHub.got.filter(f => f.t === 'hello').length >= 5)
const laneByCwd = neutralHub.got.filter(f => f.t === 'hello')[4]
check('a_tool_server_told_its_cwd_is_a_lane_worktree_names_the_lane_git_names',
  laneByCwd.lane === 'lane-0902-201212-2783563' && laneByCwd.repo === laneRepo,
  `${laneByCwd.lane} / ${laneByCwd.repo}`)
laneCwd.child.kill()

// The words. What a tool returns is what the agent goes on to repeat to the operator, so a wording
// that drifted per engine would be an agent saying his phone had buzzed in a sentence that is true
// on one engine and not on the other. The same calls in the same order, so even the minted ask id
// has to match.
async function whatItSays(b: ReturnType<typeof startBridge>) {
  b.to({ jsonrpc: '2.0', id: 900, method: 'tools/list' })
  await until('a tool list', () => b.out.some(l => l.id === 900))
  const tools = b.out.find(l => l.id === 900)!.result.tools
  const said = [
    (await call(b, 901, 'reply', { text: 'progress' })).text,
    (await call(b, 902, 'ask', { text: 'go on?', options: [{ id: 'y', label: 'Yes' }] })).text,
    (await call(b, 903, 'done', { text: 'finished' })).text,
    (await call(b, 904, 'ask_resolved', { ask_id: 'a3', how: 'withdrawn' })).text,
  ]
  return JSON.stringify({ tools, said })
}

const claudeWords = await whatItSays(claudeWay)
const opencodeWords = await whatItSays(cwdWay)
check('the_words_an_agent_reads_are_the_same_whichever_engine_it_is',
  claudeWords === opencodeWords,
  `${claudeWords.slice(0, 200)}\n  vs\n  ${opencodeWords.slice(0, 200)}`)
// Belt and braces: the sentences must be the SUCCESS ones, or two identically broken bridges would
// pass the check above.
check('and those words are the ones that say he was reached',
  /"said"/.test(claudeWords) && /asked \(a\d+\)/.test(claudeWords) &&
    /"sent"/.test(claudeWords) && /the buttons are coming off/.test(claudeWords),
  claudeWords.slice(-260))

// ── The one sentence that cannot be the same on both engines ──────────────────────────────────
//
// The operator's ANSWER comes back as `notifications/claude/channel`. Claude Code injects that into
// the agent's turn; opencode has no passthrough for an arbitrary MCP notification, so there it goes
// nowhere at all. Both clients were asked what they are — that is where the two handshakes above
// came from — and neither advertises a capability about channels, so the name is the only honest
// signal there is. "His answer will arrive" is therefore true on one engine and false on the other,
// and an agent repeating the false one to the operator is the incident this vocabulary exists for.
const onOpencode = startBridge({ KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: neutralSock })
await handshake(onOpencode, OPENCODE)
const askedThere = await call(onOpencode, 910, 'ask',
  { text: 'go on?', options: [{ id: 'y', label: 'Yes' }] })
const askedHere = await call(claudeWay, 911, 'ask',
  { text: 'go on?', options: [{ id: 'y', label: 'Yes' }] })
check('an_agent_is_never_promised_an_answer_on_an_engine_that_cannot_deliver_one',
  !/answer will arrive/.test(askedThere.text) && /do not wait for an answer/i.test(askedThere.text),
  askedThere.text)
check('and it is still told, in the same first words, whether his phone actually buzzed',
  /^asked \(a\d+\)/.test(askedThere.text), askedThere.text)
check('while a claude session is told his answer is coming, in the sentence it always was',
  askedHere.text === `asked (${askedHere.text.match(/\((a\d+)\)/)![1]}) — his answer will arrive as a channel message, do not wait here`,
  askedHere.text)

// His typed words, on the engine that cannot read them. They arrive as `message`, and the bridge
// used to write them into a channel notification nothing on opencode consumes — a line on stderr
// saying so, and nothing on the wire — so the hub went on believing they were read, and the
// operator went on looking at a line that had reached nobody. The wire has always had
// `ack{status: refused, reason}` for exactly this.
const typedSock = join(dir, 'typed.sock')
let typedConn: any = null
const typedHub = fakeHub(typedSock, (_h, s) => { welcome(s); typedConn = s })
const typedThere = startBridge({ KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: typedSock })
await handshake(typedThere, OPENCODE)
await until('the bridge to be welcomed', () => typedConn !== null)
await Bun.sleep(200)
typedConn.write(JSON.stringify({ v: 1, id: 'h-m1', t: 'message', msg_id: 'm1',
  text: 'try the staging one first', from: { chat_id: -1, user_id: 1 } }) + '\n')
await until('the refusal', () => typedHub.got.some(f => f.t === 'ack' && f.ref === 'h-m1'), 5000).catch(() => {})
const refusedThere = typedHub.got.find(f => f.t === 'ack' && f.ref === 'h-m1')
check('typed_words_on_an_engine_that_cannot_read_them_are_refused_on_the_wire_not_dropped',
  refusedThere?.status === 'refused' && /typed words/.test(String(refusedThere?.reason)) && /--opencode/.test(String(refusedThere?.reason)),
  JSON.stringify(refusedThere ?? null))
check('and they are not written into a channel nothing on this engine reads',
  !typedThere.out.some(l => l.method === 'notifications/claude/channel' && l.params?.content === 'try the staging one first'),
  JSON.stringify(typedThere.out.filter(l => l.method === 'notifications/claude/channel').map(l => l.params?.content)))
typedThere.child.kill()
typedHub.stop()

// The other three tools' SUCCESS sentences are a pair, not a copy, and calling them a copy is what
// hid a defect for a whole slice. "Said" is honest on Claude Code because the hub can still
// contradict it afterwards — `ack{delivered:'no'}` reaches the agent as a channel message and takes
// it back. Where nothing can carry that correction, the same word is the last thing the agent will
// ever hear on the subject, and it has to say so or the agent upgrades "it went out" to "he has
// seen it" and tells the operator his phone buzzed when it did not.
//
// So: the opencode sentence must OPEN with the claude one — the claim itself does not drift — and
// must then carry the marker the `instructions` block teaches the agent to look for.
//
// RED, before the fix: every one of these was byte for byte "said" / "sent" / "the buttons are
// coming off" on both engines, and the correction went to stderr.
const successes = async (b: ReturnType<typeof startBridge>, id: number) => [
  (await call(b, id, 'reply', { text: 'progress' })).text,
  (await call(b, id + 1, 'done', { text: 'finished' })).text,
  (await call(b, id + 2, 'ask_resolved', { ask_id: 'a3', how: 'withdrawn' })).text,
]
const thereWords = await successes(onOpencode, 920)
const hereWords = await successes(claudeWay, 930)
check('a_claude_session_is_told_he_was_reached_in_the_plainest_words_there_are',
  JSON.stringify(hereWords) === JSON.stringify(['said', 'sent', 'the buttons are coming off']),
  JSON.stringify(hereWords))
check('an_engine_that_can_never_take_a_success_back_says_so_in_the_same_breath',
  thereWords.every((t, i) => t.startsWith(hereWords[i]) && /nothing on this engine/.test(t)),
  JSON.stringify(thereWords))
// The marker has to be the one the instructions block hands the agent, or the teaching points at
// nothing. Read out of the running server's own handshake rather than restated here — and the
// teaching has to be there for a turn that never calls `ask`, which is the common shape.
const taught: string = onOpencode.out.find(l => l.id === 1)!.result.instructions ?? ''
check('and the marker is the one the instructions block taught it to look for',
  /nothing on this engine/.test(taught) && !/^.*`ask` says in its own result/.test(taught),
  JSON.stringify(taught.slice(0, 200)))
onOpencode.child.kill()

// ── Part 6: one namespace, and an address whoever dispatched us minted ────────────────────────
//
// Eleven variables across five prefixes said the same three things in three different ways, and
// not one of them could tell this bridge WHICH CONVERSATION it was — the address was read off git
// and nothing else could supply one. `docs/ATTACHING.md` is the interface that replaced them; these
// are the four properties an adopter writing a third adapter has to be able to rely on.
console.log('\nattached the way the interface says:')

const oneSock = join(dir, 'onenamespace.sock')
// It echoes the address back, which is what the real hub does and what a bridge that named one
// checks before it will go up.
const oneHub = fakeHub(oneSock, (h, s) =>
  s.write(JSON.stringify({ v: 1, id: 'h-w', t: 'welcome', project: 'repo', lane: h.lane,
    limits: { max_frame_bytes: 65536, per_minute: 20 } }) + '\n'))

// The directory handed over IS a linked worktree, and git's own name for it is
// `lane-0902-201212-2783563`. So if the address were still read off git — or read off git first and
// overridden second — this check reads that name back instead of the dispatcher's.
const dispatched = startBridge({
  KICKOFF_HUB_PROJECT_DIR: laneDir,
  KICKOFF_HUB_ADDRESS: 'CEO-steering',
  KICKOFF_HUB_SOCKET: oneSock,
})
await handshake(dispatched)
await until('the dispatched hello', () => oneHub.got.some(f => f.t === 'hello')).catch(() => {})
const dispatchedHello = oneHub.got.find(f => f.t === 'hello')
check('a_dispatcher_can_name_the_conversation_and_git_is_not_consulted',
  dispatchedHello?.lane === 'CEO-steering', JSON.stringify(dispatchedHello?.lane))
check('and it still proves itself with the secret in the main working tree',
  dispatchedHello?.token === 'b'.repeat(64), String(dispatchedHello?.token).slice(0, 8))
dispatched.child.kill()

// The container case, which is where git is not a fact and the derived default cannot exist: a
// directory that is inside no repository at all, told where its secret is and what it is called.
// Nothing about attaching may require git.
const nogit = join(dir, 'no-git-anywhere')
mkdirSync(join(nogit, '.kickoff'), { recursive: true })
writeFileSync(join(nogit, '.kickoff', 'hub.token'), 'e'.repeat(64), { mode: 0o600 })
const container = startBridge({
  KICKOFF_HUB_PROJECT_DIR: nogit,
  KICKOFF_HUB_TOKEN_FILE: join(nogit, '.kickoff', 'hub.token'),
  KICKOFF_HUB_ADDRESS: 'room-7',
  KICKOFF_HUB_SOCKET: oneSock,
})
await handshake(container)
await until('the container hello', () => oneHub.got.filter(f => f.t === 'hello').length >= 2)
  .catch(() => {})
const containerHello = oneHub.got.filter(f => f.t === 'hello')[1]
check('an_adapter_outside_a_repository_attaches_from_the_two_paths_it_was_handed',
  containerHello?.lane === 'room-7' && containerHello?.token === 'e'.repeat(64),
  JSON.stringify([containerHello?.lane, String(containerHello?.token).slice(0, 8)]))
container.child.kill()

// Told nothing at all, and started in a directory that would have been the right guess. The guess
// is refused anyway — it is wrong under Claude Code in both plugin layouts, which is the defect
// that made every message an agent believed it had sent go nowhere — and what it says instead has
// to be enough for whoever configured this to fix it without reading our source.
const untold = startBridge({ KICKOFF_HUB_SOCKET: oneSock }, repo)
await handshake(untold)
const untoldSaid = await call(untold, 800, 'reply', { text: 'anything' })
check('an_adapter_told_nothing_at_all_says_what_it_needed_rather_than_guessing',
  untoldSaid.isError && untoldSaid.text.includes('KICKOFF_HUB_PROJECT_DIR'), untoldSaid.text)
untold.child.kill()

// The hub refuses five shapes of address, and `bad_lane` is permanent — the same name is refused
// every time, so a bridge that learned it from a refusal frame would have spent a claim, a round
// trip and a reconnect to be told something it could have read off its own configuration. Each one
// below is a rule in `lane_is_addressable`, and the adapter has to name what is wrong with the name
// it was given rather than repeating the hub's enum at the agent.
const badAddresses: [string, string][] = [
  ['CEO/steering', 'a separator'],
  ['..', 'the name of a directory above'],
  ['x'.repeat(65), 'too long'],
  ['two\tnames', 'a control character'],
]
let refusedBeforeTheWire = 0
let sampleRefusal = ''
const helloesBefore = oneHub.got.filter(f => f.t === 'hello').length
for (const [i, [address]] of badAddresses.entries()) {
  const bad = startBridge({
    KICKOFF_HUB_PROJECT_DIR: repo,
    KICKOFF_HUB_ADDRESS: address,
    KICKOFF_HUB_SOCKET: oneSock,
  })
  await handshake(bad)
  const said = await call(bad, 810 + i, 'reply', { text: 'anything' })
  if (said.isError && said.text.includes(address.slice(0, 12))) refusedBeforeTheWire++
  if (!i) sampleRefusal = said.text
  bad.child.kill()
}
await Bun.sleep(300)
check('an_address_the_hub_would_refuse_is_refused_before_it_reaches_the_wire',
  refusedBeforeTheWire === badAddresses.length,
  `${refusedBeforeTheWire} of ${badAddresses.length}; first said: ${sampleRefusal}`)
check('and not one of them cost the hub a connection to say so',
  oneHub.got.filter(f => f.t === 'hello').length === helloesBefore,
  String(oneHub.got.filter(f => f.t === 'hello').length - helloesBefore))

// A setting that expanded to nothing is not a setting, and for a variable whose unset behaviour is
// a WORKING DEFAULT that has to be its own refusal rather than a fall-through — otherwise a
// container told exactly where its secret is, by a config whose variable failed to expand, quietly
// goes looking for one instead and attaches with whatever it finds. opencode substitutes a missing
// `{env:VAR}` with the empty string rather than failing, so this is the ordinary way it happens.
const helloesBeforeBlank = oneHub.got.filter(f => f.t === 'hello').length
const blankSetting = startBridge({
  KICKOFF_HUB_PROJECT_DIR: repo,
  KICKOFF_HUB_TOKEN_FILE: '',
  KICKOFF_HUB_SOCKET: oneSock,
})
await handshake(blankSetting)
const blankSaid = await call(blankSetting, 830, 'reply', { text: 'anything' })
check('a_setting_that_expanded_to_nothing_is_refused_rather_than_quietly_defaulted',
  blankSaid.isError && blankSaid.text.includes('KICKOFF_HUB_TOKEN_FILE'), blankSaid.text)
await Bun.sleep(300)
check('and it never went looking for a secret it was not pointed at',
  oneHub.got.filter(f => f.t === 'hello').length === helloesBeforeBlank,
  String(oneHub.got.filter(f => f.t === 'hello').length - helloesBeforeBlank))
blankSetting.child.kill()

// The one ordering that is a safety argument rather than a preference. `CLAUDE_PROJECT_DIR` is the
// engine's variable and it crosses engine boundaries: an opencode server started inside a Claude
// session inherits it, its MCP child inherits the server's whole environment, and the child then
// resolved ANOTHER repository's secret — silently, because it really did find one. A dispatcher's
// explicit word has to beat an engine's ambient one, so the decoy goes in the engine's variable.
const bothWays = startBridge({
  KICKOFF_HUB_PROJECT_DIR: repo,
  CLAUDE_PROJECT_DIR: decoy,
  KICKOFF_HUB_SOCKET: oneSock,
})
await handshake(bothWays)
await until('the both-ways hello',
  () => oneHub.got.filter(f => f.t === 'hello').length > helloesBefore).catch(() => {})
check('a_directory_the_dispatcher_names_outranks_the_one_an_engine_left_lying_around',
  oneHub.got.filter(f => f.t === 'hello')[helloesBefore]?.repo === repo,
  oneHub.got.filter(f => f.t === 'hello')[helloesBefore]?.repo)
bothWays.child.kill()

// ── Part 7: the ways a promiscuous environment reaches in, and what a session says instead ─────
//
// A namespace does not stop a variable crossing an engine boundary; it only gives the crossing one
// prefix instead of five. Every check here is a way the environment or the machine hands this
// bridge something nobody meant it to have, and what it must do about it.
console.log('\nwhat it does with what it was never meant to be handed:')

// An empty value is what opencode substitutes for a `{env:VAR}` that is not there, and for the
// project directory the fall-through is not a refusal at all — it lands on the ENGINE's own
// variable, which is the exact environment this ordering exists for. Silently, because it really
// does find a secret: the second incident, through the one variable the rule had exempted.
const helloesBeforeEmptyDir = oneHub.got.filter(f => f.t === 'hello').length
const emptyDir = startBridge({
  KICKOFF_HUB_PROJECT_DIR: '',
  CLAUDE_PROJECT_DIR: decoy,
  KICKOFF_HUB_SOCKET: oneSock,
})
await handshake(emptyDir)
const emptyDirSaid = await call(emptyDir, 840, 'reply', { text: 'anything' })
await Bun.sleep(300)
check('an_empty_project_directory_is_refused_rather_than_falling_back_to_an_engines_own',
  emptyDirSaid.isError && emptyDirSaid.text.includes('KICKOFF_HUB_PROJECT_DIR') &&
    oneHub.got.filter(f => f.t === 'hello').length === helloesBeforeEmptyDir,
  emptyDirSaid.text)
emptyDir.child.kill()

// The other half of the same problem. A config that starts a second engine INHERITS whatever the
// first was dispatched with, and an opencode-shaped config can only overlay a variable, never
// remove one — so without a spelling for "ignore this", a session speaks into a conversation
// nobody opened for it and proves itself with a repository it has never seen. The empty string
// cannot be that spelling, because empty is what a failed substitution produces.
const helloesBeforeUninherit = oneHub.got.filter(f => f.t === 'hello').length
const uninherited = startBridge({
  KICKOFF_HUB_PROJECT_DIR: laneDir,
  // Both left behind by an outer session: an address minted for a different conversation, and a
  // secret belonging to a different repository.
  KICKOFF_HUB_ADDRESS: '-',
  KICKOFF_HUB_TOKEN_FILE: '-',
  KICKOFF_HUB_SOCKET: oneSock,
})
await handshake(uninherited)
await until('the un-inherited hello',
  () => oneHub.got.filter(f => f.t === 'hello').length > helloesBeforeUninherit).catch(() => {})
const uninheritedHello = oneHub.got.filter(f => f.t === 'hello')[helloesBeforeUninherit]
check('a_variable_an_outer_session_left_behind_can_be_ignored_without_unsetting_it',
  uninheritedHello?.lane === 'lane-0902-201212-2783563' && uninheritedHello?.token === 'b'.repeat(64),
  JSON.stringify([uninheritedHello?.lane, String(uninheritedHello?.token).slice(0, 8)]))
uninherited.child.kill()

// A path that does not start with "/" is worked out against whichever folder this process happens
// to be sitting in — under a plugin manifest, the plugin folder, which is the one directory this
// whole interface exists to stop guessing from. Nothing listens at the answer, so the link reports
// itself as merely down and every message is "waiting in line" for ever.
let refusedRelative = 0
let relativeSaid = ''
for (const [i, name] of ['KICKOFF_HUB_SOCKET', 'KICKOFF_HUB_RELAY_SOCKET', 'KICKOFF_HUB_RELAY_DIR'].entries()) {
  const rel = startBridge({ KICKOFF_HUB_PROJECT_DIR: repo, [name]: 'relative.sock' })
  await handshake(rel)
  const said = await call(rel, 850 + i, 'reply', { text: 'anything' })
  if (said.isError && said.text.includes(name)) refusedRelative++
  if (!i) relativeSaid = said.text
  rel.child.kill()
}
check('a_socket_path_that_is_not_a_full_path_is_refused_rather_than_dialled_from_wherever_this_process_sits',
  refusedRelative === 3, `${refusedRelative} of 3; first said: ${relativeSaid}`)

// The folder a session was started in can be deleted while the session is still running, and for a
// product whose unit of work is a `git worktree` that is an ordinary Tuesday. Asking the machine
// for it on every start — before anything has looked at whether it was even wanted — turns that
// into a stdio server that exits before it can say a word: no refusal, no sentence for the agent,
// a stack trace on a stream nobody reads.
const doomed = join(dir, 'a-worktree-that-gets-removed')
mkdirSync(doomed, { recursive: true })
const probeFile = join(dir, 'gone-cwd-probe.ts')
writeFileSync(probeFile, [
  `import { rmSync } from 'fs'`,
  `process.chdir(${JSON.stringify(doomed)})`,
  `rmSync(${JSON.stringify(doomed)}, { recursive: true, force: true })`,
  `const { readConfig } = await import(${JSON.stringify(join(import.meta.dir, 'attach.ts'))})`,
  `const told = readConfig({ CLAUDE_PROJECT_DIR: ${JSON.stringify(repo)} })`,
  `const dotted = readConfig({ KICKOFF_HUB_PROJECT_DIR: '.' })`,
  `console.log(JSON.stringify({`,
  `  told: 'config' in told ? told.config.projectDir : told.problem.note,`,
  `  dotted: 'config' in dotted ? dotted.config.projectDir : dotted.problem.why,`,
  `}))`,
].join('\n'))
const goneCwd = Bun.spawnSync(['bun', probeFile], { env: { ...process.env }, stdout: 'pipe', stderr: 'pipe' })
const goneOut = new TextDecoder().decode(goneCwd.stdout).trim()
let gone: { told?: string; dotted?: string } = {}
try { gone = JSON.parse(goneOut) } catch { /* it died before it could say anything */ }
check('a_session_whose_folder_was_deleted_says_so_rather_than_dying_before_it_can_speak',
  gone.told === repo && typeof gone.dotted === 'string' && gone.dotted.includes('no longer there'),
  JSON.stringify(gone.told ? gone : { exitCode: goneCwd.exitCode, stderr: new TextDecoder().decode(goneCwd.stderr).slice(0, 200) }))

// Two settings that used to be one line are now two, and pasting the folder while missing the
// relay line is the ordinary way to arrive here. What the holder actually is, is the relay the
// operator is running on purpose — so every sentence about "another session" sends him hunting for
// something that does not exist, and none of them names the line that is missing.
const claimedSock = join(dir, 'claimed.sock')
const claimedHub = fakeHub(claimedSock, (_h, s) =>
  s.write(JSON.stringify({ v: 1, id: 'h-c', t: 'refused', reason: 'already_claimed' }) + '\n'))
const standIn = join(dir, 'a-relay-is-listening.sock')
// Something really listening, because a leftover socket FILE is not a relay and must not be
// diagnosed as one — a relay killed outright leaves its file behind.
const standInRelay = Bun.listen({ unix: standIn, socket: { open(s) { s.end() }, data() {}, close() {} } })
const dialledPast = startBridge({
  KICKOFF_HUB_PROJECT_DIR: repo,
  KICKOFF_HUB_SOCKET: claimedSock,
  KICKOFF_HUB_RELAY_SOCKET: standIn,
})
await handshake(dialledPast)
await Bun.sleep(600)
const dialledPastSaid = await call(dialledPast, 860, 'reply', { text: 'anything' })
check('a_session_dialling_past_a_running_relay_is_told_which_setting_joins_it',
  dialledPastSaid.text.includes('KICKOFF_HUB_RELAY') && !/another session/i.test(dialledPastSaid.text),
  dialledPastSaid.text)
dialledPast.child.kill()
standInRelay.stop(true)
claimedHub.stop()

// `bad_lane` when NO name was sent cannot be the hub: the hub only checks a name that is there.
// It is a relay carrying one conversation of a project, turning away a session that speaks for the
// project as a whole. Telling that session to remake a worktree names a thing that does not exist
// and prescribes an action that has already once cost somebody uncommitted work.
const foldedSock = join(dir, 'relay-holding-a-lane.sock')
const foldedRelay = fakeHub(foldedSock, (_h, s) =>
  s.write(JSON.stringify({ v: 1, id: 'h-b', t: 'refused', reason: 'bad_lane' }) + '\n'))
const noName = startBridge({ KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: foldedSock })
await handshake(noName)
await Bun.sleep(600)
const noNameSaid = await call(noName, 870, 'reply', { text: 'anything' })
check('a_session_that_named_no_conversation_is_never_told_to_remake_a_worktree',
  !/worktree/.test(noNameSaid.text) && !/unnamed/.test(noNameSaid.text) &&
    noNameSaid.text.includes('KICKOFF_HUB_ADDRESS'),
  noNameSaid.text)
noName.child.kill()
foldedRelay.stop()

// The one refusal an agent reads that has to carry a command a person can run. Its two sources are
// git's answer and a secret it just failed to find, so in a container — or any directory git will
// not talk about — both are empty and what the agent read, and then repeated to the operator, was
// the literal words "<the project folder>".
const nowhere = join(dir, 'no-git-and-no-secret')
mkdirSync(nowhere, { recursive: true })
const unenrolled = startBridge({ KICKOFF_HUB_PROJECT_DIR: nowhere, KICKOFF_HUB_SOCKET: oneSock })
await handshake(unenrolled)
const unenrolledSaid = await call(unenrolled, 880, 'reply', { text: 'anything' })
check('an_instruction_to_enrol_names_a_folder_even_where_git_cannot_answer',
  unenrolledSaid.text.includes(`herdr-tg enroll ${nowhere}`) && !unenrolledSaid.text.includes('<the project folder>'),
  unenrolledSaid.text)
unenrolled.child.kill()

oneHub.stop()

claudeWay.child.kill()
namedWay.child.kill()
cwdWay.child.kill()
neutralHub.stop()

rmSync(dir, { recursive: true, force: true })
console.log(`\n${failures === 0 ? 'all checks passed' : `${failures} FAILED`}`)
process.exit(failures === 0 ? 0 : 1)
