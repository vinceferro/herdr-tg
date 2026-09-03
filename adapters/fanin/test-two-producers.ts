#!/usr/bin/env bun
/**
 * The relay, against REAL producers and a fake hub.
 *
 *     bun test-two-producers.ts
 *
 * This is the DESIGN: one slot at the hub, two producers behind it, and the ids and taps kept
 * apart. The failures four reviewers found in it are next door, in `test-what-breaks-it.ts`.
 *
 * The producers are the SHIPPING `server.ts`, started exactly as an engine starts it, never a stub
 * that speaks what this file imagines the wire to be. A fixture invented here would only prove that
 * this file agrees with itself, which is how the opencode bridge came to read its event payload out
 * of the wrong field and pass every test it had.
 *
 * The fake hub enforces ONE live claim per (repo, lane), because that rule is the entire reason the
 * relay exists. Part 1 proves the race it prevents is real before Part 2 proves it prevents it.
 * The rig itself — the fake hub, the real producers — is in `test-harness.ts`, shared with the
 * other suite so the two can never come to disagree about what the wire is.
 */

import { mkdtempSync, mkdirSync, readdirSync, rmSync, existsSync } from 'fs'
import { join } from 'path'

import {
  CLAUDE_CODE, HERE, OPENCODE, call, channelMessages, check, claimingHub, failed, handshake,
  makeRepo, noticesTo, rawProducer, startFanin, startServer, until,
} from './test-harness.ts'

const dir = mkdtempSync('/tmp/fi-')
const LANE = 'lane-0903-104500-1234'
const { repo, laneDir } = makeRepo(dir, LANE)

// ── Part 1: the race the relay exists to prevent is real ──────────────────────────────────────
console.log('\nwithout a relay:')

const raceSock = join(dir, 'race.sock')
const raceHub = claimingHub(raceSock)
const first = startServer({ CLAUDE_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: raceSock })
await handshake(first)
await until('the first hello', () => raceHub.got.some(f => f.t === 'hello'))
const second = startServer({ CLAUDE_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: raceSock })
await handshake(second)
await until('the second hello', () => raceHub.got.filter(f => f.t === 'hello').length >= 2)
await until('a refusal', () => raceHub.refusals.length >= 1)
check('two_producers_for_one_lane_dialling_the_hub_directly_race_for_one_claim',
  raceHub.refusals[0] === 'already_claimed', JSON.stringify(raceHub.refusals))
first.child.kill(); second.child.kill()
raceHub.stop()
await Bun.sleep(200)

// ── Part 2: one slot, served by two producers ─────────────────────────────────────────────────
console.log('\nwith a relay in front of it:')

const hubSock = join(dir, 'hub.sock')
const faninDir = join(dir, 'fanin')
const hub = claimingHub(hubSock)
const relay = startFanin(laneDir, { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_FANIN_DIR: faninDir })
await until('the relay to say hello to the hub', () => hub.got.some(f => f.t === 'hello'))

const viaRelay = { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_FANIN_DIR: faninDir, KICKOFF_CHANNEL_VIA_FANIN: '1' }

// Producer A is started the way CLAUDE CODE starts one: cwd is somewhere else entirely and the
// project is named only by the variable.
const A = startServer({ CLAUDE_PROJECT_DIR: laneDir, ...viaRelay })
await handshake(A, CLAUDE_CODE.capabilities, CLAUDE_CODE.clientInfo)
// Producer B is started the way OPENCODE starts one: no `CLAUDE_PROJECT_DIR` anywhere, cwd IS the
// session's directory, and a flag saying so. Same file, same wire — and the same claim in the same
// first word, with one difference the ENGINE decides: nothing there can take a success back later,
// so the sentence that reports one says it is the last word there will be.
const B = startServer({ KICKOFF_CHANNEL_CWD_IS_PROJECT: '1', ...viaRelay }, laneDir)
await handshake(B, OPENCODE.capabilities, OPENCODE.clientInfo)

const saidA = await call(A, 'reply', { text: 'A is halfway through' })
const saidB = await call(B, 'reply', { text: 'B is halfway through' })
check('an_opencode_agent_that_calls_reply_reaches_the_operator',
  saidB.text.startsWith('said') && !saidB.isError, saidB.text)
check('and is told in the same breath that nothing will ever take that back',
  /nothing on this engine/.test(saidB.text), saidB.text)
check('and the claude-started producer beside it is reached too', saidA.text === 'said', saidA.text)
await until('both messages at the hub', () => hub.got.filter(f => f.t === 'say').length >= 2)
check('the words the agent wrote reach the hub verbatim',
  hub.got.some(f => f.t === 'say' && f.text === 'B is halfway through'),
  JSON.stringify(hub.got.filter(f => f.t === 'say').map(f => f.text)))
check('two_producers_for_one_lane_share_one_hub_connection_rather_than_racing_for_it',
  hub.got.filter(f => f.t === 'hello').length === 1 && hub.refusals.length === 0,
  `${hub.got.filter(f => f.t === 'hello').length} hellos, ${hub.refusals.length} refusals`)
check('and the hub was told the worktree, once, by the relay itself',
  hub.got.find(f => f.t === 'hello')!.lane === LANE)

// Both producers are the same build with the same counter, so their first question is the same
// string on both. The hub resolves a tap BY ask id against a written record, so two questions
// sharing one id is a tap delivered to whichever agent the map happened to hold.
const askA = await call(A, 'ask', { text: "A's question", options: [{ id: 'y', label: 'Yes' }] })
const askB = await call(B, 'ask', { text: "B's question", options: [{ id: 'y', label: 'Yes' }] })
const mine = (t: string) => t.match(/\((a\d+)\)/)![1]
check('both producers really did mint the same question id', mine(askA.text) === mine(askB.text),
  `${askA.text} / ${askB.text}`)
await until('both questions at the hub', () => hub.got.filter(f => f.t === 'ask').length >= 2)
const asks = hub.got.filter(f => f.t === 'ask')
const atHubA = asks.find(f => f.text === "A's question")!
const atHubB = asks.find(f => f.text === "B's question")!
check('two_producers_that_both_mint_a1_get_their_own_answers_back',
  atHubA.ask_id !== atHubB.ask_id, `${atHubA.ask_id} / ${atHubB.ask_id}`)

const beforeB = channelMessages(B).length
hub.to({ v: 1, id: 'h-c', t: 'choice', msg_id: 'm1', ask_id: atHubA.ask_id, option_id: 'y' })
await until('the tap to reach A', () => channelMessages(A).some(l => l.params?.meta?.ask_id))
  .catch(() => {})
const tapped = channelMessages(A).find(l => l.params?.meta?.ask_id)
check('a tap comes back named with the id the producer minted, not the relay',
  tapped?.params?.meta?.ask_id === mine(askA.text), JSON.stringify(tapped?.params?.meta ?? null))
await Bun.sleep(400)
check('a_tap_on_a_question_one_producer_asked_is_never_delivered_to_the_other',
  channelMessages(B).length === beforeB,
  JSON.stringify(channelMessages(B).slice(beforeB).map(l => l.params?.content)))

// An `ack` names an envelope id, and the relay minted its own — so a ref passed straight through is
// an id the producer has no record of, and the notice its agent needs is never written at all.
hub.ack = 'no'
const doomed = await call(A, 'ask', { text: 'Restart the database?', options: [{ id: 'y', label: 'Yes' }] })
await until('the notice that he was not reached', () => noticesTo(A).length >= 1).catch(() => {})
const notice = String(noticesTo(A)[0]?.params?.content ?? '')
check('an_ack_names_the_frame_the_producer_sent_not_the_one_the_relay_sent',
  /never got/.test(notice) && notice.includes(mine(doomed.text)), notice)
hub.ack = 'yes'

// The operator does not know there are two producers, and the hub has no way to address one.
const beforeMsgA = channelMessages(A).length
const beforeMsgB = channelMessages(B).length
hub.to({ v: 1, id: 'h-m', t: 'message', msg_id: 'm9', text: 'try it with --dry-run first',
  from: { chat_id: -1, user_id: 1 } })
await until('the typed words to reach both',
  () => channelMessages(A).length > beforeMsgA && channelMessages(B).length > beforeMsgB)
check('the_operators_typed_words_reach_every_producer',
  channelMessages(A).at(-1)!.params.content === 'try it with --dry-run first' &&
    channelMessages(B).at(-1)!.params.content === 'try it with --dry-run first')

// Liveness is what keeps the claim. A producer that has wedged must not be able to cost the lane
// its place on the phone, so the ping is answered by the relay itself and never handed on.
const pongsBefore = hub.got.filter(f => f.t === 'pong').length
hub.to({ v: 1, id: 'h-ping', t: 'ping' })
await until('a pong', () => hub.got.filter(f => f.t === 'pong').length > pongsBefore)
check('a_liveness_probe_is_answered_even_while_every_producer_is_silent',
  hub.got.filter(f => f.t === 'pong').at(-1)!.ref === 'h-ping')

// One producer restarting is not the lane leaving. If a `bye` were passed through, the whole
// conversation would go quiet because one of two voices refreshed its context.
B.child.kill()
await until('the relay to notice', () => true)
await Bun.sleep(600)
const afterBye = await call(A, 'reply', { text: 'A is still here' })
check('a_producer_saying_goodbye_does_not_take_the_lane_off_the_phone',
  afterBye.text === 'said' && hub.got.filter(f => f.t === 'hello').length === 1, afterBye.text)
check('and the relay never passed the goodbye on',
  !hub.got.some(f => f.t === 'bye'), JSON.stringify(hub.got.filter(f => f.t === 'bye')))

// One producer's backlog must never cost another producer its question. A refused question is an
// agent told to stop waiting for an answer it needed.
//
// Through an OUTAGE the relay does not hold that backlog at all: it ends its producers' connections
// the moment its own link drops, so each of them keeps its own line and says "not said yet" in its
// own words — and a producer that overruns is told so in the tool result, which is the one place
// its agent is certain to read. The relay's share of its own queue governs the other case, a hub
// that is up and not keeping up, and three producers sharing it are next door in
// `test-what-breaks-it.ts`.
const flooded = join(dir, 'flood.sock')
const floodDir = join(dir, 'flood-fanin')
let floodHub = claimingHub(flooded)
const relayF = startFanin(laneDir, { KICKOFF_HUB_SOCKET: flooded, KICKOFF_FANIN_DIR: floodDir })
await until('the flood relay to reach its hub', () => floodHub.got.some(f => f.t === 'hello'))
const viaFlood = { KICKOFF_HUB_SOCKET: flooded, KICKOFF_FANIN_DIR: floodDir, KICKOFF_CHANNEL_VIA_FANIN: '1' }
const loud = startServer({ CLAUDE_PROJECT_DIR: laneDir, ...viaFlood })
const quiet = startServer({ KICKOFF_CHANNEL_CWD_IS_PROJECT: '1', ...viaFlood }, laneDir)
await handshake(loud)
await handshake(quiet)
// Both must be admitted BEFORE the hub goes, or the relay simply holds them un-greeted and nothing
// of theirs ever reaches its queue.
await until('both to be heard once', () => floodHub.got.filter(f => f.t === 'say').length >= 2,
  10000).catch(() => {})
await call(loud, 'reply', { text: 'loud is here' })
await call(quiet, 'reply', { text: 'quiet is here' })
await until('both greeted', () => floodHub.got.filter(f => f.t === 'say').length >= 2)

floodHub.stop()
rmSync(flooded, { force: true })
await Bun.sleep(800)

const flood = []
for (let i = 0; i < 80; i++) flood.push(await call(loud, 'reply', { text: `flood ${i}` }))
const shed = flood.filter(r => r.isError)
const asked = await call(quiet, 'ask',
  { text: "the quiet one's question", options: [{ id: 'y', label: 'Yes' }] })
check('a_chatty_producer_cannot_fill_the_queue_the_other_producers_question_needs',
  !asked.isError, asked.text)
check('and the quiet producer is never told to stop waiting for an answer',
  !/NOT asked/.test(asked.text) && noticesTo(quiet).length === 0,
  `${asked.text} | ${JSON.stringify(noticesTo(quiet).map(l => l.params?.content))}`)

floodHub = claimingHub(flooded)
await until("the quiet one's question to arrive once the hub is back",
  () => floodHub.got.some(f => f.t === 'ask' && f.text === "the quiet one's question"), 20000)
  .catch(() => {})
check('and it really does reach the hub when the link comes back',
  floodHub.got.some(f => f.t === 'ask' && f.text === "the quiet one's question"),
  JSON.stringify(floodHub.got.map(f => f.t)))
check('while the flood was bounded by the flooding producers own line, not the quiet ones',
  floodHub.got.filter(f => f.t === 'say' && /^flood /.test(f.text)).length <= 64 && shed.length > 0,
  `${floodHub.got.filter(f => f.t === 'say' && /^flood /.test(f.text)).length} through, ${shed.length} shed`)
check('and the flooding producer is told which of its own messages were shed, where it must read it',
  shed.every(r => /^NOT said/.test(r.text)), JSON.stringify(shed.slice(0, 1).map(r => r.text)))
loud.child.kill(); quiet.child.kill(); relayF.kill(); floodHub.stop()
await Bun.sleep(200)

// ── Part 2c: the two voices this slice exists for ─────────────────────────────────────────────
//
// The tool server carries what the agent CHOSE to say; the event bridge carries the prompts it did
// not choose. Those are the two things that could not both hold the claim, and this is the pair the
// operator asked for. The bridge below is the SHIPPING `bridge.ts` with not one line changed — only
// `KICKOFF_HUB_SOCKET` pointed at the relay instead of the hub.
console.log('\nthe two voices, through one slot:')

// The bridge speaks for a PROJECT and names no lane, so its relay is the project's own.
const projSock = join(dir, 'proj.sock')
const projDir = join(dir, 'proj-fanin')
const projHub = claimingHub(projSock)
const relayP = startFanin(repo, { KICKOFF_HUB_SOCKET: projSock, KICKOFF_FANIN_DIR: projDir })
await until('the project relay to reach the hub', () => projHub.got.some(f => f.t === 'hello'))
const projRelaySock = join(projDir, readdirSync(projDir).find(f => f.endsWith('.sock'))!)

// A fake opencode that publishes ONE real event. The payload is the shape captured off a real
// `/event` stream — `properties`, not `data` — because a fixture invented here would only prove
// this file agrees with itself, which is the exact way that field was got wrong once already.
let pushEvent: ((o: unknown) => void) | null = null
const oc = Bun.serve({
  port: 0,
  fetch(req) {
    if (new URL(req.url).pathname !== '/event') return new Response('{}', { status: 200 })
    return new Response(new ReadableStream({
      start(c) {
        const enc = new TextEncoder()
        pushEvent = (o: unknown) => c.enqueue(enc.encode(`data: ${JSON.stringify(o)}\n\n`))
      },
    }), { headers: { 'content-type': 'text/event-stream' } })
  },
})

const eventBridge = Bun.spawn(['bun', join(HERE, '..', 'opencode-bridge', 'bridge.ts')], {
  cwd: repo,
  env: { ...process.env, OPENCODE_BRIDGE_REPO: repo, OPENCODE_URL: `http://127.0.0.1:${oc.port}`,
    KICKOFF_HUB_SOCKET: projRelaySock },
  stdout: 'inherit', stderr: 'inherit',
})
const voice = startServer({ KICKOFF_CHANNEL_CWD_IS_PROJECT: '1',
  KICKOFF_FANIN_DIR: projDir, KICKOFF_CHANNEL_VIA_FANIN: '1' }, repo)
await handshake(voice, OPENCODE.capabilities, OPENCODE.clientInfo)

const chose = await call(voice, 'reply', { text: 'what the agent chose to say' })
check('the voice the agent chooses reaches the operator',
  chose.text.startsWith('said') && !chose.isError, chose.text)

await until('the bridge to be watching', () => pushEvent !== null, 15000)
pushEvent!({
  id: 'evt_1',
  type: 'permission.v2.asked',
  properties: { id: 'per_1', sessionID: 'ses_1', action: 'run a command', resources: ['rm -rf build'] },
})
await until('the prompt the agent did not choose',
  () => projHub.got.some(f => f.t === 'ask' && /rm -rf build/.test(String(f.text))), 15000).catch(() => {})
check('the_real_event_bridge_attaches_to_the_relay_without_a_line_of_its_own_changing',
  projHub.got.some(f => f.t === 'ask' && /rm -rf build/.test(String(f.text))),
  JSON.stringify(projHub.got.map(f => f.t)))
check('and both voices came through ONE claim at the hub',
  projHub.got.filter(f => f.t === 'hello').length === 1 && projHub.refusals.length === 0 &&
    projHub.got.some(f => f.t === 'say' && f.text === 'what the agent chose to say'),
  `${projHub.got.filter(f => f.t === 'hello').length} hellos, ${projHub.refusals.length} refusals`)
// The two producers mint ids from their own counters — `b*` and `f*` — and both would have been
// `f1`/`b1` at the hub without rewriting.
const ids = projHub.got.filter(f => ['say', 'ask'].includes(f.t)).map(f => f.id)
check('and every frame the hub saw carried an id of its own',
  new Set(ids).size === ids.length, JSON.stringify(ids))

eventBridge.kill(); voice.child.kill(); relayP.kill(); oc.stop(true); projHub.stop()
await Bun.sleep(200)

// ── Part 3: who the relay will not speak for ──────────────────────────────────────────────────
console.log('\nwho it turns away:')

// Found by looking, not by re-deriving: exactly one socket for exactly one address is itself the
// property under test, and a test that recomputed the hash would agree with the code by
// construction rather than checking it.
const socks = readdirSync(faninDir).filter(f => f.endsWith('.sock'))
check('one relay makes exactly one address', socks.length === 1, JSON.stringify(socks))
const relaySock = join(faninDir, socks[0])

const wrongLane = rawProducer(relaySock)
await wrongLane.ready
wrongLane.send({ v: 1, id: 'x1', t: 'hello', project_id: 'p', token: 'a'.repeat(64),
  instance: 'i', repo, pid: process.pid, lane: 'some-other-worktree' })
await until('a refusal for the wrong lane', () => wrongLane.got.length >= 1)
check('a_producer_that_names_a_lane_the_relay_does_not_hold_is_refused_rather_than_relayed',
  wrongLane.got[0].t === 'refused' && wrongLane.got[0].reason === 'bad_lane',
  JSON.stringify(wrongLane.got[0]))

const wrongSecret = rawProducer(relaySock)
await wrongSecret.ready
wrongSecret.send({ v: 1, id: 'x1', t: 'hello', project_id: 'p', token: 'z'.repeat(64),
  instance: 'i', repo, pid: process.pid, lane: LANE })
await until('a refusal for the wrong secret', () => wrongSecret.got.length >= 1)
check('a_producer_presenting_a_secret_the_relay_does_not_hold_is_turned_away',
  wrongSecret.got[0].t === 'refused' && wrongSecret.got[0].reason === 'bad_token',
  JSON.stringify(wrongSecret.got[0]))

const mute = rawProducer(relaySock)
await mute.ready
mute.send({ v: 1, id: 'x1', t: 'say', text: 'no hello first' })
await Bun.sleep(400)
check('a producer that never said hello is not relayed',
  !hub.got.some(f => f.t === 'say' && f.text === 'no hello first'))

// Two relays for one address hold two claims and race, which is the whole thing this prevents.
const second_relay = startFanin(laneDir, { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_FANIN_DIR: faninDir })
const rc = await second_relay.exited
check('a_second_relay_for_one_conversation_refuses_to_start_rather_than_racing_the_first',
  rc === 2, `exit ${rc}`)
check('and the hub still sees exactly one connection for the lane',
  hub.got.filter(f => f.t === 'hello').length === 1,
  String(hub.got.filter(f => f.t === 'hello').length))

A.child.kill()
relay.kill()
hub.stop()
await Bun.sleep(300)

// ── Part 4: the relay is not there ────────────────────────────────────────────────────────────
console.log('\nwith no relay at all:')

const emptyDir = join(dir, 'no-relay')
mkdirSync(emptyDir, { recursive: true })
const orphan = startServer(
  { CLAUDE_PROJECT_DIR: laneDir, KICKOFF_FANIN_DIR: emptyDir, KICKOFF_CHANNEL_VIA_FANIN: '1',
    KICKOFF_HUB_SOCKET: hubSock },
  HERE,
)
await handshake(orphan)
const unheard = await call(orphan, 'reply', { text: 'is anyone there' })
check('a_producer_whose_fan_in_is_gone_says_so_rather_than_reporting_success',
  unheard.text !== 'said' && /waiting/i.test(unheard.text), unheard.text)
check('and it names the relay rather than blaming the hub, which is running',
  /relay/i.test(unheard.text), unheard.text)
check('and it does not tell the agent to give up, because a relay can come back',
  !unheard.isError, unheard.text)
check('the relay socket really was absent', !existsSync(join(emptyDir, 'nothing.sock')))
orphan.child.kill()

rmSync(dir, { recursive: true, force: true })
const n = failed()
console.log(`\n${n === 0 ? 'all checks passed' : `${n} FAILED`}`)
process.exit(n === 0 ? 0 : 1)
