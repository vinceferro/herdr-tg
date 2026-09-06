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

import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, existsSync, writeFileSync } from 'fs'
import { join } from 'path'

import {
  CLAUDE_CODE, HERE, OPENCODE, call, channelMessages, check, claimingHub, failed, handshake,
  makeRepo, noticesTo, rawProducer, startAttach, startServer, until,
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
const outbox = join(dir, 'outbox')
mkdirSync(outbox, { recursive: true, mode: 0o700 })
const hub = claimingHub(hubSock, outbox)
const relay = startAttach(laneDir, { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: faninDir })
await until('the relay to say hello to the hub', () => hub.got.some(f => f.t === 'hello'))

// Nothing is attached yet. The operator's typed words used to be dropped here in silence — no
// producer to hand them to, no note, and no ack — so the hub went on believing they were read.
// The wire has always had `ack{status: refused, reason}` for exactly this; the door answers with
// it, and the hub's own half (the line in his topic) is `crates/herdr-tg/src/hub/tests.rs`.
hub.to({ v: 1, id: 'h-m0', t: 'message', msg_id: 'm0', text: 'anyone home?', from: { chat_id: -1, user_id: 1 } })
await until('the refusal', () => hub.got.some(f => f.t === 'ack' && f.ref === 'h-m0')).catch(() => {})
const nobody = hub.got.find(f => f.t === 'ack' && f.ref === 'h-m0')
check('typed_words_arriving_while_nothing_is_attached_are_refused_out_loud_rather_than_dropped',
  nobody?.status === 'refused' && /nothing.*attached|attached.*nothing/i.test(String(nobody?.reason)),
  JSON.stringify(nobody ?? null))

const viaRelay = { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: faninDir, KICKOFF_HUB_RELAY: '1' }

// Producer A is started the way CLAUDE CODE starts one: cwd is somewhere else entirely and the
// project is named only by the variable.
const A = startServer({ CLAUDE_PROJECT_DIR: laneDir, ...viaRelay })
await handshake(A, CLAUDE_CODE.capabilities, CLAUDE_CODE.clientInfo)
// Producer B is started the way OPENCODE starts one: no `CLAUDE_PROJECT_DIR` anywhere, cwd IS the
// session's directory, and a flag saying so. Same file, same wire — and the same claim in the same
// first word, with one difference the ENGINE decides: nothing there can take a success back later,
// so the sentence that reports one says it is the last word there will be.
const B = startServer({ KICKOFF_HUB_PROJECT_DIR: '.', ...viaRelay }, laneDir)
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

// A message that carries a FILE, at a door with three producers behind it — one of them older
// than files, which is what any upgrade window and the ordinary opencode layout both look like.
// The count in the one ack the hub hears decides whether it puts "that file did not reach the
// agent" in his topic, and the first answer to arrive is a race between processes: the older one
// wins it whenever it is a lookup and the newer one is a POST. Answered from the loser, the hub
// tells him a file was lost that an agent in this same lane is looking at.
{
  const socks = readdirSync(faninDir).filter(f => f.endsWith('.sock'))
  const older = rawProducer(join(faninDir, socks[0]), (f, send) => {
    // The instant it reads his words, and it has never heard of `files`.
    if (f.t === 'message') send({ v: 1, id: `o-${f.id}`, t: 'ack', ref: f.id, status: 'accepted' })
  })
  await older.ready
  older.send({ v: 1, id: 'o1', t: 'hello', project_id: 'p', token: 'a'.repeat(64),
    instance: 'older-than-files', repo, pid: process.pid, lane: LANE })
  await until('the older producer to be let in', () => older.got.some(f => f.t === 'welcome'))
  hub.to({ v: 1, id: 'h-f1', t: 'message', msg_id: 'mf1', text: 'the login page, as it is now',
    from: { chat_id: -1, user_id: 1 },
    files: [{ kind: 'photo', path: join(dir, 'shot.jpg'), mime: 'image/jpeg', bytes: 1183412 }] })
  await until('an answer about the file', () => hub.got.some(f => f.t === 'ack' && f.ref === 'h-f1'), 8000)
    .catch(() => {})
  await Bun.sleep(900)
  const folded = hub.got.filter(f => f.t === 'ack' && f.ref === 'h-f1')
  check('a_short_count_from_one_producer_never_answers_for_the_one_that_took_the_file',
    folded.length === 1 && folded[0].status === 'accepted' && folded[0].files === 1,
    JSON.stringify(folded))
  older.end()
  await Bun.sleep(200)
}

// The fold settles on the first answer that cannot be bettered, and the producers that had not
// answered yet still do — a refusal from a watcher landing after the tool server's accept is the
// ordinary shape whenever the box is busy. The door used to forget the fold the moment it settled,
// so the late answer found nothing to fold into and went up as it was: two acks for one message,
// from the one thing on this wire that exists to keep it at one.
{
  const socks = readdirSync(faninDir).filter(f => f.endsWith('.sock'))
  const late = rawProducer(join(faninDir, socks[0]), (f, send) => {
    if (f.t === 'message') setTimeout(() => send({ v: 1, id: `l-${f.id}`, t: 'ack', ref: f.id,
      status: 'refused', reason: 'nothing on this engine reads a channel message' }), 300)
  })
  await late.ready
  late.send({ v: 1, id: 'l1', t: 'hello', project_id: 'p', token: 'a'.repeat(64),
    instance: 'answers-late', repo, pid: process.pid, lane: LANE })
  await until('the late producer to be let in', () => late.got.some(f => f.t === 'welcome'))
  hub.to({ v: 1, id: 'h-m1', t: 'message', msg_id: 'mm1', text: 'and the tests?', from: { chat_id: -1, user_id: 1 } })
  await until('an answer about the words', () => hub.got.some(f => f.t === 'ack' && f.ref === 'h-m1'), 8000)
    .catch(() => {})
  await Bun.sleep(900)
  const answers = hub.got.filter(f => f.t === 'ack' && f.ref === 'h-m1')
  check('an_answer_that_lands_after_the_fold_has_settled_is_never_a_second_ack_for_the_same_words',
    answers.length === 1 && answers[0].status === 'accepted', JSON.stringify(answers))
  late.end()
  await Bun.sleep(200)
}

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

// The operator does not know there are two producers, and the hub has no way to address one. The
// door hands his words to every producer and each answers for itself: a Claude session's tool
// server hands them to its agent; an opencode session's cannot — nothing on that engine reads a
// channel message — and it used to write them into that void anyway, with a line on stderr and
// nothing on the wire, so the hub went on believing they were read.
const beforeMsgA = channelMessages(A).length
const beforeMsgB = channelMessages(B).length
hub.to({ v: 1, id: 'h-m', t: 'message', msg_id: 'm9', text: 'try it with --dry-run first',
  from: { chat_id: -1, user_id: 1 } })
await until('the typed words to reach the producer whose engine can read them',
  () => channelMessages(A).length > beforeMsgA)
check('the_operators_typed_words_reach_the_producer_whose_engine_can_read_them',
  channelMessages(A).at(-1)!.params.content === 'try it with --dry-run first')
await Bun.sleep(300)
check('and are not written into a channel the other engine cannot read',
  channelMessages(B).length === beforeMsgB,
  JSON.stringify(channelMessages(B).slice(beforeMsgB).map(l => l.params?.content)))

// Files cross the door with no change to it (`docs/ATTACHING.md` §14.2): the welcome's `outbox`
// reaches a producer as the hub wrote it — which is how the producer knows where to copy — a
// `say` carrying `file` reaches the hub with the field the producer wrote, and the hub's `no-file`
// ack comes back down to the producer that sent it and no other. Asserted against the real door
// rather than assumed from the relay's source, because "forwards everything" is exactly the kind
// of sentence a later edit makes false.
const shot = join(dir, 'shot.png')
writeFileSync(shot, Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47]), Buffer.alloc(500, 3)]))
hub.ack = null
const withFile = await call(A, 'reply', { text: 'the chart', file: shot })
await until('the say with a file at the hub', () => hub.got.some(f => f.t === 'say' && f.file), 3000).catch(() => {})
const fileAtHub = hub.got.find(f => f.t === 'say' && f.file)
check('a_file_a_producer_names_crosses_the_door_to_the_hub_with_its_bytes_in_the_outbox_the_hub_named',
  /^said, with the file shot\.png/.test(withFile.text)
    && fileAtHub !== undefined && fileAtHub.text === 'the chart' && fileAtHub.file.mime === 'image/png'
    && fileAtHub.file.filename === 'shot.png' && existsSync(join(outbox, fileAtHub.file.name)),
  `${withFile.text} / ${JSON.stringify(fileAtHub ?? 'no say with a file')}`)
const noticesA = noticesTo(A).length
const noticesB = noticesTo(B).length
if (fileAtHub) hub.to({ v: 1, id: 'h-nf', t: 'ack', ref: fileAtHub.id, delivered: 'yes', why: 'no-file' })
await until('the producer to hear the file did not go', () => noticesTo(A).length > noticesA, 5000).catch(() => {})
check('and_the_hubs_no_file_ack_reaches_the_producer_that_sent_it_and_no_other',
  noticesTo(A).length === noticesA + 1
    && /file with it did not come through/.test(String(noticesTo(A).at(-1)?.params?.content ?? ''))
    && noticesTo(B).length === noticesB,
  JSON.stringify(noticesTo(A).slice(noticesA).map(n => n.params?.content)))
hub.ack = 'yes'

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
const relayF = startAttach(laneDir, { KICKOFF_HUB_SOCKET: flooded, KICKOFF_HUB_RELAY_DIR: floodDir })
await until('the flood relay to reach its hub', () => floodHub.got.some(f => f.t === 'hello'))
const viaFlood = { KICKOFF_HUB_SOCKET: flooded, KICKOFF_HUB_RELAY_DIR: floodDir, KICKOFF_HUB_RELAY: '1' }
const loud = startServer({ CLAUDE_PROJECT_DIR: laneDir, ...viaFlood })
const quiet = startServer({ KICKOFF_HUB_PROJECT_DIR: '.', ...viaFlood }, laneDir)
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

// ── Part 2c: the two voices this slice exists for, now inside ONE process ─────────────────────
//
// The tool server carries what the agent CHOSE to say; the event watcher carries the prompts it did
// not choose. Those are the two things that could not both hold the claim, and this is the pair the
// operator asked for. There is no longer a separate bridge PROCESS: attach is started with
// `--opencode`, and the watcher is an in-process producer at attach's own door — the Part 2c shape
// with the process boundary removed. If the two voices still keep one claim and their ids apart, the
// collapse changed nothing on the wire.
console.log('\nthe two voices, through one slot, inside one process:')

// attach speaks for a PROJECT and names no address, so its door is the project's own.
const projSock = join(dir, 'proj.sock')
const projDir = join(dir, 'proj-fanin')
const projHub = claimingHub(projSock)

// A fake opencode that publishes ONE real event. The payload is the shape captured off a real
// `/event` stream — `properties`, not `data` — because a fixture invented here would only prove
// this file agrees with itself, which is the exact way that field was got wrong once already.
let pushEvent: ((o: unknown) => void) | null = null
/** What `GET /session` lists — `{}`, an answer the watcher cannot read, until a test lists one. */
let ocSessions: Record<string, unknown>[] | null = null
/** Every prompt the fake opencode was handed, as the session it went to and the words. */
const ocPrompts: { path: string; text: string }[] = []
const oc = Bun.serve({
  port: 0,
  async fetch(req) {
    const u = new URL(req.url)
    if (u.pathname === '/event') {
      return new Response(new ReadableStream({
        start(c) {
          const enc = new TextEncoder()
          pushEvent = (o: unknown) => c.enqueue(enc.encode(`data: ${JSON.stringify(o)}\n\n`))
        },
      }), { headers: { 'content-type': 'text/event-stream' } })
    }
    if (u.pathname === '/session' && req.method === 'GET') {
      return ocSessions ? Response.json(ocSessions) : new Response('{}', { status: 200 })
    }
    if (u.pathname.endsWith('/prompt_async') && req.method === 'POST') {
      const body = await req.json()
      ocPrompts.push({ path: u.pathname, text: String(body?.parts?.[0]?.text) })
      // The captured answer: 204, no body.
      return new Response(null, { status: 204 })
    }
    return new Response('{}', { status: 200 })
  },
})

// ONE attach: it holds the claim, opens the door, AND watches the fake opencode in-process.
const attachP = startAttach(repo, { KICKOFF_HUB_SOCKET: projSock, KICKOFF_HUB_RELAY_DIR: projDir },
  false, ['--opencode', `http://127.0.0.1:${oc.port}`])
await until('attach to reach the hub', () => projHub.got.some(f => f.t === 'hello'))
const projRelaySock = join(projDir, readdirSync(projDir).find(f => f.endsWith('.sock'))!)

// The tool server finds attach's door by the same git derivation attach used — same RELAY_DIR, same
// repo, no address — so it meets the door with no socket named, exactly as the operator's paste
// block does on his own box.
const voice = startServer({ KICKOFF_HUB_PROJECT_DIR: '.',
  KICKOFF_HUB_RELAY_DIR: projDir, KICKOFF_HUB_RELAY: '1' }, repo)
await handshake(voice, OPENCODE.capabilities, OPENCODE.clientInfo)

const chose = await call(voice, 'reply', { text: 'what the agent chose to say' })
check('the voice the agent chooses reaches the operator',
  chose.text.startsWith('said') && !chose.isError, chose.text)

await until('the watcher to be watching', () => pushEvent !== null, 15000)
pushEvent!({
  id: 'evt_1',
  type: 'permission.v2.asked',
  properties: { id: 'per_1', sessionID: 'ses_1', action: 'run a command', resources: ['rm -rf build'] },
})
await until('the prompt the agent did not choose',
  () => projHub.got.some(f => f.t === 'ask' && /rm -rf build/.test(String(f.text))), 15000).catch(() => {})
// The two producers mint ids from their own counters — the tool server's and the watcher's — and
// both would have collided at the hub without the door rewriting them.
const ids = projHub.got.filter(f => ['say', 'ask'].includes(f.t)).map(f => f.id)
check('the_event_voice_and_the_chosen_voice_share_one_claim_inside_one_process',
  projHub.got.filter(f => f.t === 'hello').length === 1 && projHub.refusals.length === 0 &&
    projHub.got.some(f => f.t === 'say' && f.text === 'what the agent chose to say') &&
    projHub.got.some(f => f.t === 'ask' && /rm -rf build/.test(String(f.text))) &&
    new Set(ids).size === ids.length,
  `${projHub.got.filter(f => f.t === 'hello').length} hellos, ${projHub.refusals.length} refusals, ids ${JSON.stringify(ids)}`)

// His typed words, at a door with TWO voices behind it, get ONE answer at the hub — the hub keeps
// the first ack it hears for a message and no other. The tool server on opencode refuses them
// (nothing on that engine reads a channel message); the watcher carries them, or refuses with the
// reason that matters. Forwarded as they came, the tool server's "this engine cannot" arrived first
// every time, being no more than a lookup, and the operator read the wrong reason on the days the
// watcher could have said why — and "accepted" was never the first when both spoke.
const typedAt = (id: string, text: string) =>
  projHub.to({ v: 1, id, t: 'message', msg_id: `m-${id}`, text, from: { chat_id: -1, user_id: 1 } })
typedAt('h-t1', 'anyone there?')
await until('an answer', () => projHub.got.some(f => f.t === 'ack' && f.ref === 'h-t1'), 8000).catch(() => {})
await Bun.sleep(600)
const answers = projHub.got.filter(f => f.t === 'ack' && f.ref === 'h-t1')
check('typed_words_at_a_door_with_two_voices_get_one_answer_and_it_is_the_carriers',
  answers.length === 1 && answers[0].status === 'refused' &&
    /worker's server|session/.test(String(answers[0].reason)) && !/engine/.test(String(answers[0].reason)),
  JSON.stringify(answers))
ocSessions = [{ id: 'ses_000000000000000000theOne', directory: repo, time: { created: 1, updated: 2 } }]
typedAt('h-t2', 'try the staging one first')
await until('the words to be taken', () => projHub.got.some(f => f.t === 'ack' && f.ref === 'h-t2'), 8000).catch(() => {})
await Bun.sleep(600)
const takenAt = projHub.got.filter(f => f.t === 'ack' && f.ref === 'h-t2')
check("and when the carrier takes them the hub hears accepted, once, never the other voice's refusal",
  takenAt.length === 1 && takenAt[0].status === 'accepted' && ocPrompts.some(p => p.text === 'try the staging one first'),
  `${JSON.stringify(takenAt)} prompts=${JSON.stringify(ocPrompts)}`)

// THE STRANGER'S TEST, and it is the one that matters most. `docs/examples/attach-from-the-document.ts`
// was written from `docs/ATTACHING.md` alone and imports nothing from this repository — not the
// wire, not the configuration reader, not a type. An interface is only abstract if somebody who has
// never read our TypeScript can attach from the document, so what is under test here is the
// DOCUMENT. Its peer is the real relay and, behind that, the whole real path to the hub.
const strangerSource = readFileSync(join(HERE, '..', '..', 'docs', 'examples', 'attach-from-the-document.ts'), 'utf8')
check('the stranger really did write it without reading our code',
  !/from '\.\.?\//.test(strangerSource) && !/hub-link|attach\.ts|where\.ts/.test(strangerSource),
  JSON.stringify(strangerSource.match(/^import .*$/gm)))

const strangerSaid: string[] = []
const stranger = Bun.spawn(['bun', join(HERE, '..', '..', 'docs', 'examples', 'attach-from-the-document.ts')], {
  // Started somewhere that is not the project, and told nothing but the three things §2 and §9 name.
  cwd: HERE,
  env: { ...process.env, KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_RELAY: '1',
    KICKOFF_HUB_RELAY_SOCKET: projRelaySock },
  stdout: 'pipe', stderr: 'inherit',
})
;(async () => {
  const dec = new TextDecoder()
  let acc = ''
  for await (const chunk of stranger.stdout as any) {
    acc += dec.decode(chunk)
    for (;;) {
      const nl = acc.indexOf('\n')
      if (nl < 0) break
      const l = acc.slice(0, nl); acc = acc.slice(nl + 1)
      strangerSaid.push(l)
      console.log(`    [stranger] ${l}`)
    }
  }
})()
await until('the stranger to be welcomed', () => strangerSaid.some(l => l.startsWith('WELCOME')), 15000)
  .catch(() => {})
check('an_adapter_written_from_the_document_alone_attaches_and_is_heard',
  strangerSaid.some(l => l.startsWith('WELCOME')) &&
    projHub.got.some(f => f.t === 'say' && f.text === 'attached from the document alone'),
  JSON.stringify(strangerSaid))
await until('the stranger to be acked', () => strangerSaid.some(l => l.startsWith('ACK')), 8000).catch(() => {})
check('and it is told what became of what it said, in the three values the document names',
  strangerSaid.some(l => l === 'ACK reached'), JSON.stringify(strangerSaid))
check('and it still cost the hub exactly one claim, with three adapters behind it',
  projHub.got.filter(f => f.t === 'hello').length === 1 && projHub.refusals.length === 0,
  `${projHub.got.filter(f => f.t === 'hello').length} hellos, ${projHub.refusals.length} refusals`)
stranger.kill()

attachP.kill(); voice.child.kill(); oc.stop(true); projHub.stop()
await Bun.sleep(200)

// ── Part 3: who the door will not speak for ───────────────────────────────────────────────────
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

// Two attaches for one address hold two claims and race, which is the whole thing this prevents.
const second_attach = startAttach(laneDir, { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: faninDir })
const rc = await second_attach.exited
check('a_second_attach_for_one_conversation_refuses_to_start_rather_than_racing_the_first',
  rc === 2, `exit ${rc}`)
check('and the hub still sees exactly one connection for the lane',
  hub.got.filter(f => f.t === 'hello').length === 1,
  String(hub.got.filter(f => f.t === 'hello').length))

// The hub takes a frame and the connection ends before it answers for it — a hub restart with
// something in flight, or the hub before this slice destroying what a bridge said before its pong.
// An ack names a per-connection id, so no answer is ever coming on the next connection; and the
// relay is the only thing that knows WHOSE frame it was. Its own link used to keep the frame on the
// books in silence, and the producer's agent went on believing "said".
hub.ack = null
const taken = await call(A, 'reply', { text: 'taken and never answered for' })
check('a frame the hub took is reported as said, because it was', taken.text === 'said', taken.text)
await until('the hub to have it', () => hub.got.some(f => f.t === 'say' && f.text === 'taken and never answered for'))
const hellosBefore = hub.got.filter(f => f.t === 'hello').length
hub.drop()
await until('the producer to be told that nobody knows what became of it',
  () => noticesTo(A).some(n => /could not confirm/.test(String(n.params?.content ?? ''))), 8000).catch(() => {})
const unknown = noticesTo(A).map(n => String(n.params?.content ?? '')).find(t => /could not confirm/.test(t)) ?? ''
check('a_producer_learns_which_of_its_frames_the_hub_took_and_never_answered_for', unknown !== '',
  JSON.stringify(noticesTo(A).map(n => n.params?.content)))
check('and it is told the fate is unknown, never that he did not get it',
  /may have arrived and it may not/.test(unknown) && !/never got/.test(unknown), unknown)
hub.ack = 'yes'
await until('the relay to come back', () => hub.got.filter(f => f.t === 'hello').length > hellosBefore, 8000).catch(() => {})
await Bun.sleep(300)
check('and the frame is not sent again, because it may already be on his phone',
  hub.got.filter(f => f.t === 'say' && f.text === 'taken and never answered for').length === 1,
  String(hub.got.filter(f => f.t === 'say' && f.text === 'taken and never answered for').length))

// The door's OWN line is not the hub's. When the hub connection ends, the frames still waiting in
// it never went anywhere — and the door then ends its producers' sockets, so each producer's own
// close-time report counted everything it had handed over and not heard back on, those included:
// its agent was told they had gone out with their fate unknown, and not to expect them again, and
// the door then sent them on the next connection. Each one is answered `no` BEFORE the socket
// ends, so "never got it" is the truth and nothing the agent was told to forget is sent later.
//
// The twelve are queued in the PRODUCER while the hub is away — "not said yet", certain to go out
// — so they are all inside it before the hub comes back. The hub then stops reading inside the
// first chunk they arrive in (its one thread sleeps, which is what a hub that wedged looks like
// from the door) so the door's writes fill the kernel and the rest sit in its own line; then the
// hub closes on it. Twelve frames of sixty thousand bytes is more than two socket buffers hold.
hub.drop()
await until('the hub connection to be gone', () => hub.live === 0)
await Bun.sleep(400)
const noticesBefore = noticesTo(A).length
const hellosNow = hub.got.filter(f => f.t === 'hello').length
const queued: number[] = []
for (let i = 0; i < 12; i++) {
  const id = 300 + i
  queued.push(id)
  A.to({ jsonrpc: '2.0', id, method: 'tools/call',
    params: { name: 'reply', arguments: { text: `held at the door ${i} ${'x'.repeat(60_000)}` } } })
}
await until('every reply to be answered', () => queued.every(id => A.out.some(l => l.id === id)), 15000)
const told = queued.map(id => String(A.out.find(l => l.id === id)!.result?.content?.[0]?.text ?? ''))
check('with the hub away the twelve are honestly reported as waiting', told.every(t => /^not said yet/.test(t)), JSON.stringify(told.map(t => t.slice(0, 40))))
hub.freezeThenDrop(1500)
await until('the relay to come back', () => hub.got.filter(f => f.t === 'hello').length > hellosNow, 10000).catch(() => {})
await until('every frame to be answered for', () => {
  const n = noticesTo(A).slice(noticesBefore).map(x => String(x.params?.content ?? ''))
  return n.filter(t => /never got/.test(t)).length + n.filter(t => /could not confirm/.test(t)).length >= 12
}, 10000).catch(() => {})
await until('the relay to come back once more', () => hub.got.filter(f => f.t === 'hello').length > hellosNow + 1, 10000).catch(() => {})
await Bun.sleep(1500)
const notices = noticesTo(A).slice(noticesBefore).map(x => String(x.params?.content ?? ''))
const neverGot = notices.filter(t => /never got/.test(t)).length
const unconfirmed = notices.filter(t => /could not confirm/.test(t)).length
const closeTime = notices.filter(t => /connection ended before the hub said/.test(t))
check('a_frame_still_waiting_at_the_door_when_the_hub_goes_is_answered_no_before_the_producer_is_ended',
  neverGot >= 1 && neverGot + unconfirmed === 12 && closeTime.length === 0,
  `never got ${neverGot}, unconfirmed ${unconfirmed}, close-time reports ${closeTime.length} ${JSON.stringify(closeTime.map(t => t.slice(0, 140)))}`)
check('and nothing the agent was told to forget is sent on the next connection',
  hub.got.filter(f => f.t === 'say' && /^held at the door/.test(String(f.text))).length === 0,
  String(hub.got.filter(f => f.t === 'say' && /^held at the door/.test(String(f.text))).length))

A.child.kill()
relay.kill()
hub.stop()
await Bun.sleep(300)

// ── Part 4: the relay is not there ────────────────────────────────────────────────────────────
console.log('\nwith no relay at all:')

const emptyDir = join(dir, 'no-relay')
mkdirSync(emptyDir, { recursive: true })
const orphan = startServer(
  { CLAUDE_PROJECT_DIR: laneDir, KICKOFF_HUB_RELAY_DIR: emptyDir, KICKOFF_HUB_RELAY: '1',
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
