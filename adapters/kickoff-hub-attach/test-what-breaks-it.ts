#!/usr/bin/env bun
/**
 * attach's door under the failures four reviewers found in the relay it used to be.
 *
 *     bun test-what-breaks-it.ts
 *
 * `test-two-producers.ts` proves the design: one slot at the hub, two producers behind it, ids and
 * taps kept apart. Every property here is one that suite could not see, because it only ever ran
 * two producers, never lost one, never lost the hub under them, and never asked more than a handful
 * of questions. `startAttach` starts `main.ts` with no `--run` and no `--opencode`, which is the
 * whole of what the relay was.
 *
 * Each is written as the sentence that has to stay true. They were watched failing first, against
 * the relay as it stood, and what each one printed then is recorded beside it.
 */

import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'fs'
import { join } from 'path'

import {
  CLAUDE_CODE, call, check, claimingHub, failed, handshake, makeRepo, rawProducer, startAttach,
  startServer, until,
} from './test-harness.ts'

const dir = mkdtempSync('/tmp/fb-')
const LANE = 'lane-0903-201500-9999'
const { repo, laneDir } = makeRepo(dir, LANE)

/** One relay, one fake hub, one address — a scenario that cannot disturb the next one. */
function scenario(name: string, projectDir: string, opts: { hub?: boolean; env?: Record<string, string>; lease?: number } = {}) {
  const hubSock = join(dir, `${name}.sock`)
  const faninDir = join(dir, `${name}-fanin`)
  const hub = opts.hub === false ? null : claimingHub(hubSock, undefined, opts.lease)
  const relay = startAttach(projectDir, {
    KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: faninDir, ...(opts.env ?? {}),
  }, true)
  const sockOf = async () => {
    await until('the relay to open its door', () => {
      try { return readdirSync(faninDir).some(f => f.endsWith('.sock')) } catch { return false }
    }, 15000)
    return join(faninDir, readdirSync(faninDir).find(f => f.endsWith('.sock'))!)
  }
  return { hubSock, faninDir, hub, relay, sockOf,
    said: (re: RegExp) => relay.said.some(l => re.test(l)) }
}

const hello = (extra: Record<string, unknown>) => ({
  v: 1, id: 'h1', t: 'hello', project_id: 'p', token: 'a'.repeat(64), repo,
  pid: process.pid, ...extra,
})

// ── A. the hub goes away under a producer ─────────────────────────────────────────────────────
//
// The relay stands in for the hub, so a producer's own link is up whenever the RELAY is up — and
// the relay was letting it stay that way with nothing behind it. Every tool then returned the
// sentence that means "he was reached", which is the incident this whole vocabulary was written
// for, moved one process further out.
//
// RED, before the fix:
//   FAIL a_producer_is_never_told_he_was_reached_while_the_relays_own_link_is_down  said
//   FAIL and a producer that arrives while the hub is gone is not welcomed into the same lie  said
console.log('\nwhen the hub goes away under the relay:')

{
  const s = scenario('drop', laneDir)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const via = { KICKOFF_HUB_SOCKET: s.hubSock, KICKOFF_HUB_RELAY_DIR: s.faninDir, KICKOFF_HUB_RELAY: '1' }
  const A = startServer({ CLAUDE_PROJECT_DIR: laneDir, ...via })
  await handshake(A, CLAUDE_CODE.capabilities, CLAUDE_CODE.clientInfo)
  const before = await call(A, 'reply', { text: 'while everything is up' })
  check('while the hub is there the producer is told he was reached', before.text === 'said', before.text)

  s.hub!.stop()
  rmSync(s.hubSock, { force: true })
  await until('the relay to notice the hub is gone', () => s.said(/nothing is listening|link/i), 20000)
  await Bun.sleep(1500)

  const after = await call(A, 'reply', { text: 'while the hub is gone' })
  check('a_producer_is_never_told_he_was_reached_while_the_relays_own_link_is_down',
    after.text !== 'said', after.text)
  check('and it is told to wait rather than to give up, because the hub can come back',
    !after.isError && /waiting/i.test(after.text), after.text)
  // Behind the relay the local socket goes on answering through the whole outage, so the sentence
  // that names a missing process can never be reached and this is the only one an outage produces.
  // Saying the link "is being rebuilt" promises a repair a stopped hub will not perform: the agent
  // waits, the queue fills to 64 and starts dropping, and the one fact that would send somebody to
  // restart it sits on the relay's own stderr, which nobody reads.
  //
  // RED, before the fix: 'The link to his phone dropped and is being rebuilt.'
  check('a_hub_that_is_simply_not_running_is_named_rather_than_promised_to_come_back',
    /kickoff-channel/.test(after.text) && !/being rebuilt/.test(after.text), after.text)

  // A producer that arrives DURING the outage was greeted out of a `welcome` from a connection that
  // had been dead for minutes.
  const B = startServer({ KICKOFF_HUB_PROJECT_DIR: '.', ...via }, laneDir)
  await handshake(B, CLAUDE_CODE.capabilities, CLAUDE_CODE.clientInfo)
  await Bun.sleep(1200)
  const late = await call(B, 'done', { text: 'the lane is finished' })
  check('and a producer that arrives while the hub is gone is not welcomed into the same lie',
    late.text !== 'sent', late.text)

  // What was queued while the link was down must still go out, or the honest sentence would be a
  // lie in the other direction.
  const hub2 = claimingHub(s.hubSock)
  await until('the queued message to arrive once the hub is back',
    () => hub2.got.some(f => f.t === 'say' && f.text === 'while the hub is gone'), 40000).catch(() => {})
  check('and what it was told was waiting in line really does go out when the hub comes back',
    hub2.got.some(f => f.t === 'say' && f.text === 'while the hub is gone'),
    JSON.stringify(hub2.got.map(f => f.t)))
  const back = await call(A, 'reply', { text: 'and now he is reachable again' })
  check('and once the hub is back the producer is told he was reached again', back.text === 'said', back.text)

  A.child.kill(); B.child.kill(); s.relay.kill(); hub2.stop()
  await Bun.sleep(300)
}

// ── B. a producer goes away with a question open ──────────────────────────────────────────────
//
// The hub records an ask against the pid and instance of the CLAIM, which under a relay are always
// the relay's — so neither of the hub's two sweeps can ever see a producer die. The keyboard stays
// live on the operator's phone, he taps it, and the tap is written into a closed socket.
//
// RED, before the fix:
//   FAIL a_tap_for_a_producer_that_is_gone_is_never_dropped_in_silence  (no note at all)
//   FAIL a_question_whose_asker_never_came_back_has_its_buttons_taken_off  []
//   FAIL a_producer_that_reconnects_still_gets_the_tap_it_is_waiting_on  []
console.log('\nwhen a producer goes away with a question open:')

{
  const s = scenario('gone', repo, { env: { KICKOFF_HUB_RELAY_GRACE_MS: '2000' } })
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()

  const P = rawProducer(sock)
  await P.ready
  P.send(hello({ instance: 'producer-one' }))
  // Two questions: one the operator taps after its agent has gone, and one he never gets to.
  P.send({ v: 1, id: 'p-ask', t: 'ask', ask_id: 'a1', text: 'proceed with the deploy?',
    options: [{ option_id: 'y', label: 'Yes' }] })
  P.send({ v: 1, id: 'p-ask2', t: 'ask', ask_id: 'a2', text: 'and roll the database forward?',
    options: [{ option_id: 'y', label: 'Yes' }] })
  await until("both questions at the hub", () => s.hub!.got.filter(f => f.t === 'ask').length >= 2, 15000)
  const atHub = s.hub!.got.find(f => f.t === 'ask')!.ask_id as string
  const untapped = s.hub!.got.filter(f => f.t === 'ask')[1].ask_id as string

  P.end()
  await until('the relay to notice it went', () => s.said(/went away/), 10000)
  s.hub!.to({ v: 1, id: 'h-c', t: 'choice', msg_id: 'm1', ask_id: atHub, option_id: 'y' })
  await Bun.sleep(800)
  check('a_tap_for_a_producer_that_is_gone_is_never_dropped_in_silence',
    s.said(/tap/i), JSON.stringify(s.relay.said.slice(-3)))

  // The operator is looking at buttons nobody can answer any more. Nothing else will ever take them
  // off: the hub's sweeps see only the relay, and the relay is fine.
  await until('the withdrawal', () => s.hub!.got.some(f => f.t === 'ask_resolved'), 12000).catch(() => {})
  const withdrawn = s.hub!.got.find(f => f.t === 'ask_resolved')
  check('a_question_whose_asker_never_came_back_has_its_buttons_taken_off',
    withdrawn?.ask_id === untapped && withdrawn?.how === 'withdrawn',
    JSON.stringify(s.hub!.got.filter(f => f.t === 'ask_resolved')))

  // A producer that merely restarted its socket is NOT gone, and the tap it is waiting on must
  // still find it. Its `instance` is what says so: a process keeps one for its whole life.
  const Q = rawProducer(sock)
  await Q.ready
  Q.send(hello({ instance: 'producer-two' }))
  Q.send({ v: 1, id: 'q-ask', t: 'ask', ask_id: 'a1', text: 'and this one?',
    options: [{ option_id: 'y', label: 'Yes' }] })
  // Found by its own words, never by position: this producer's `a1` and the last one's `a1` are the
  // same string, which is the whole reason the relay namespaces them.
  await until("the second producer's question at the hub",
    () => s.hub!.got.some(f => f.t === 'ask' && f.text === 'and this one?'), 15000)
  const qAtHub = s.hub!.got.find(f => f.t === 'ask' && f.text === 'and this one?')!.ask_id as string
  Q.end()
  await Bun.sleep(200)
  const Q2 = rawProducer(sock)
  await Q2.ready
  Q2.send(hello({ instance: 'producer-two' }))
  await Bun.sleep(400)
  s.hub!.to({ v: 1, id: 'h-c2', t: 'choice', msg_id: 'm2', ask_id: qAtHub, option_id: 'y' })
  await until('the tap to reach the producer that came back',
    () => Q2.got.some(f => f.t === 'choice'), 8000).catch(() => {})
  check('a_producer_that_reconnects_still_gets_the_tap_it_is_waiting_on',
    Q2.got.some(f => f.t === 'choice' && f.ask_id === 'a1'), JSON.stringify(Q2.got))

  Q2.end(); s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

// ── C. more questions than the routing map would hold ─────────────────────────────────────────
//
// A question the hub DID deliver kept its routing entry for ever, and the map was capped at 256 by
// INSERTION ORDER — so the 257th question silently un-routed the 1st while its buttons were still
// live on his phone. The hub keeps a keyboard tappable for 48 hours; a permission prompt per
// command reaches 257 in an afternoon.
//
// RED, before the fix:
//   FAIL a_tap_on_the_oldest_of_many_still_open_questions_is_still_routed_back  []
console.log('\nwith more open questions than the map used to hold:')

{
  const s = scenario('many', repo)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()
  const P = rawProducer(sock)
  await P.ready
  P.send(hello({ instance: 'the-chatty-one' }))
  for (let i = 1; i <= 260; i++) {
    P.send({ v: 1, id: `m${i}`, t: 'ask', ask_id: `a${i}`, text: `question ${i}`,
      options: [{ option_id: 'y', label: 'Yes' }] })
  }
  await until('all of them at the hub',
    () => s.hub!.got.filter(f => f.t === 'ask').length >= 260, 30000)
  const first = s.hub!.got.find(f => f.t === 'ask')!.ask_id as string
  s.hub!.to({ v: 1, id: 'h-c', t: 'choice', msg_id: 'm1', ask_id: first, option_id: 'y' })
  await until('the tap on the oldest question', () => P.got.some(f => f.t === 'choice'), 8000)
    .catch(() => {})
  check('a_tap_on_the_oldest_of_many_still_open_questions_is_still_routed_back',
    P.got.some(f => f.t === 'choice' && f.ask_id === 'a1'),
    JSON.stringify(P.got.filter(f => f.t === 'choice')))

  P.end(); s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

// ── D. a third producer ───────────────────────────────────────────────────────────────────────
//
// The share of the queue was a constant tuned for exactly two producers, and opencode gives one
// address three without being asked: it starts one MCP child PER DIRECTORY, so a session in the
// repo root, a session in a subfolder and the event bridge are three voices on one relay. Two of
// them filled the line and the third's QUESTION was refused for a backlog that was not its own.
//
// RED, before the fix:
//   FAIL a_third_producer_is_not_refused_for_a_backlog_that_is_not_its_own  ack delivered no
console.log('\nwith three producers and no hub:')

{
  const s = scenario('three', repo, { hub: false })
  const sock = await s.sockOf()
  const P: ReturnType<typeof rawProducer>[] = []
  for (let i = 0; i < 3; i++) {
    const p = rawProducer(sock)
    await p.ready
    p.send(hello({ instance: `producer-${i}` }))
    P.push(p)
  }
  await until('all three attached', () => s.said(/attached \(3 now\)/), 10000)
  for (let i = 0; i < 30; i++) P[0].send({ v: 1, id: `x${i}`, t: 'say', text: `first ${i}`, hint: 'prose' })
  for (let i = 0; i < 30; i++) P[1].send({ v: 1, id: `y${i}`, t: 'say', text: `second ${i}`, hint: 'prose' })
  await Bun.sleep(500)
  for (let i = 0; i < 10; i++) P[2].send({ v: 1, id: `z${i}`, t: 'say', text: `third ${i}`, hint: 'prose' })
  P[2].send({ v: 1, id: 'z-ask', t: 'ask', ask_id: 'a1', text: 'the third one is blocked',
    options: [{ option_id: 'y', label: 'Yes' }] })
  await Bun.sleep(800)
  const refused = P[2].got.filter(f => f.t === 'ack' && f.delivered === 'no').map(f => f.ref)
  check('a_third_producer_is_not_refused_for_a_backlog_that_is_not_its_own',
    !refused.includes('z-ask'), JSON.stringify(refused))

  // And the line really is shared out rather than handed to whoever spoke first.
  const hub = claimingHub(s.hubSock)
  await until("the third producer's question to reach the hub once the link is up",
    () => hub.got.some(f => f.t === 'ask' && f.text === 'the third one is blocked'), 90000).catch(() => {})
  check('and its question really does reach the hub when the link comes up',
    hub.got.some(f => f.t === 'ask' && f.text === 'the third one is blocked'),
    JSON.stringify(hub.got.filter(f => f.t === 'ask').map(f => f.text)))

  for (const p of P) p.end()
  s.relay.kill(); hub.stop()
  await Bun.sleep(300)
}

// ── E. the relay itself restarts ──────────────────────────────────────────────────────────────
//
// Producers outlive this process: an opencode server holds its MCP children across a relay restart,
// and each of them may be waiting on a question. A new `instance` tells the hub every one of those
// is void — it retires them on arrival and refuses a tap on any that survive — with nothing to tell
// the agents. The direct-to-hub path does not have this: a channel plugin mints one instance for
// the life of its session, so restarting `herdr-tg` under it leaves its questions answerable.
//
// RED, before the fix:
//   FAIL a_relay_that_restarts_comes_back_as_the_same_voice_the_hub_has_questions_open_under
//        two instances: 424242-1788... / 424999-1788...
//   FAIL and a producer waiting since before the restart still gets its answer  []
console.log('\nwhen the relay itself restarts:')

{
  const s = scenario('again', repo, { env: { KICKOFF_HUB_RELAY_GRACE_MS: '30000' } })
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()
  const P = rawProducer(sock)
  await P.ready
  P.send(hello({ instance: 'the-agent-that-waits' }))
  P.send({ v: 1, id: 'p-ask', t: 'ask', ask_id: 'a7', text: 'wait for me',
    options: [{ option_id: 'y', label: 'Yes' }] })
  await until('the question at the hub', () => s.hub!.got.some(f => f.t === 'ask'), 15000)
  const atHub = s.hub!.got.find(f => f.t === 'ask')!.ask_id as string

  s.relay.kill()
  await s.relay.exited
  P.end()
  await Bun.sleep(300)

  const again = startAttach(repo, {
    KICKOFF_HUB_SOCKET: s.hubSock, KICKOFF_HUB_RELAY_DIR: s.faninDir, KICKOFF_HUB_RELAY_GRACE_MS: '30000',
  }, true)
  await until('the relay to say hello again',
    () => s.hub!.got.filter(f => f.t === 'hello').length >= 2, 20000)
  const hellos = s.hub!.got.filter(f => f.t === 'hello').map(f => f.instance)
  check('a_relay_that_restarts_comes_back_as_the_same_voice_the_hub_has_questions_open_under',
    hellos[0] === hellos[1], JSON.stringify(hellos))

  // And the producer that was waiting all along still gets its answer, under the id IT minted.
  const P2 = rawProducer(sock)
  await P2.ready
  P2.send(hello({ instance: 'the-agent-that-waits' }))
  await Bun.sleep(400)
  s.hub!.to({ v: 1, id: 'h-c', t: 'choice', msg_id: 'm1', ask_id: atHub, option_id: 'y' })
  await until('the tap to reach it', () => P2.got.some(f => f.t === 'choice'), 8000).catch(() => {})
  check('and a producer waiting since before the restart still gets its answer',
    P2.got.some(f => f.t === 'choice' && f.ask_id === 'a7'), JSON.stringify(P2.got))

  P2.end(); again.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

// ── E2. and it stops carrying yesterday's name once nothing is open under it ──────────────────
//
// The name above is kept across a restart because the hub has questions open under it. That reason
// runs out the moment there are none: a door that reads back a ledger with nothing waiting is a
// door whose old name means nothing to anybody, and keeping it anyway is how a box that ran the
// adapter before 8 September goes on saying a pid to the hub for ever — the ledger file outlives
// every restart, so the one change that took the pid off this wire would never reach a deployed
// box at all.
//
// RED, before the fix:
//   FAIL a_door_whose_remembered_name_has_no_questions_under_it_comes_back_with_a_new_one
//        {"remembered":"4242-1757000000000","said":"4242-1757000000000"}
console.log('\nwhen the door restarts with nothing open under its old name:')

{
  const s = scenario('quiet', repo)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()
  const state = `${sock}.state`
  // Ask nothing, but make the door write its file: a producer arriving is enough to give it a
  // number to remember, and that is the file a restart reads back.
  const P = rawProducer(sock)
  await P.ready
  P.send(hello({ instance: 'a-producer-with-no-questions' }))
  P.send({ v: 1, id: 'p-say', t: 'say', text: 'nothing to decide' })
  await until('the words at the hub', () => s.hub!.got.some(f => f.t === 'say'), 15000)
  await until('the door to write down what a restart would need', () => existsSync(state), 15000)
  P.end()
  s.relay.kill()
  await s.relay.exited
  await Bun.sleep(300)

  // What a box that ran the pre-change adapter has on disk: a name minted from a pid, and no
  // question open under it.
  const past = JSON.parse(readFileSync(state, 'utf8'))
  const remembered = '4242-1757000000000'
  writeFileSync(state, JSON.stringify({ ...past, instance: remembered, asks: [] }), { mode: 0o600 })

  const again = startAttach(repo, { KICKOFF_HUB_SOCKET: s.hubSock, KICKOFF_HUB_RELAY_DIR: s.faninDir }, true)
  await until('the relay to say hello again',
    () => s.hub!.got.filter(f => f.t === 'hello').length >= 2, 20000)
  const said = String(s.hub!.got.filter(f => f.t === 'hello')[1].instance)
  check('a_door_whose_remembered_name_has_no_questions_under_it_comes_back_with_a_new_one',
    said !== remembered && /^[0-9a-f]{16}-[0-9]{13,}$/.test(said),
    JSON.stringify({ remembered, said }))

  again.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

// ── F. a claim held for good ──────────────────────────────────────────────────────────────────
//
// The `already_claimed` run-counter moved here from the bridge as "three refusals in a row". Three
// dials with the link's backoff is a claim held for THREE SECONDS — shorter than the ten seconds a
// predecessor attach gets to stop, so an ordinary restart tripped it. And when it tripped, the
// producers were ended FIRST and their queued frames let go second, so the "no" had no socket to
// travel on and the agent kept reading "waiting in line" for a frame nothing would ever carry.
//
// So: a run is stuck when it has lasted longer than a predecessor's stop, and the moment it is, a
// producer still here hears `ack no` for what it queued BEFORE its socket ends.
//
// RED, before the fix:
//   FAIL a_claim_held_for_good_is_given_longer_than_a_predecessor_gets_to_stop  stuck after 3.0s
//   FAIL and_the_producer_hears_no_for_its_queued_frame_before_its_socket_ends  producer saw []
console.log('\nwhen another connection holds the claim and never lets go:')

{
  const hubSock = join(dir, 'squat.sock')
  const t0 = Date.now()
  const refusedAt: number[] = []
  // Every hello refused: the squatter never leaves.
  const squatter = Bun.listen({
    unix: hubSock,
    socket: {
      open() {},
      data(s: any, chunk: any) {
        for (const line of chunk.toString().split('\n')) {
          if (!line.trim()) continue
          if (JSON.parse(line).t !== 'hello') continue
          refusedAt.push(Date.now() - t0)
          s.write(JSON.stringify({ v: 1, id: 'sq', t: 'refused', reason: 'already_claimed' }) + '\n')
          s.end()
        }
      },
      close() {}, error() {},
    },
  })
  const door = join(dir, 'squat-door.sock')
  const relay = startAttach(repo, { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_SOCKET: door }, true)
  await until('the door', () => existsSync(door), 15000)

  // A producer that comes straight back every time the door ends it, as a real one does with
  // backoff — the same instance, so the door knows it is the same voice. It queues ONE frame, once.
  const heard: { at: number; f: Record<string, any> }[] = []
  let queued = false
  const redial = async (): Promise<void> => {
    const p = rawProducer(door)
    await p.ready.catch(() => {})
    if (!p.connected) { await Bun.sleep(100); return redial() }
    p.send(hello({ instance: 'the-one-that-waits' }))
    if (!queued) { queued = true; p.send({ v: 1, id: 'p-say', t: 'say', text: 'queued while the claim is held' }) }
    while (p.connected) await Bun.sleep(25)
    for (const f of p.got) heard.push({ at: Date.now() - t0, f })
    if (!stopped) { await Bun.sleep(50); return redial() }
  }
  let stopped = false
  void redial()

  await until('the door to give up on the claim', () => relay.said.some(l => /is not letting go/.test(l)), 45000).catch(() => {})
  const stuckAt = Date.now() - t0
  await Bun.sleep(500)
  stopped = true
  const ackNo = heard.find(h => h.f.t === 'ack' && h.f.delivered === 'no' && h.f.ref === 'p-say')
  const lastRefused = heard.filter(h => h.f.t === 'refused').at(-1)
  check('a_claim_held_for_good_is_given_longer_than_a_predecessor_gets_to_stop',
    relay.said.some(l => /is not letting go/.test(l)) && stuckAt >= 30_000 && refusedAt.length >= 5,
    `stuck after ${(stuckAt / 1000).toFixed(1)}s; refusals at ${refusedAt.map(ms => (ms / 1000).toFixed(1) + 's').join(' ')}`)
  check('and_the_producer_hears_no_for_its_queued_frame_before_its_socket_ends',
    ackNo !== undefined && !relay.said.some(l => /nothing could be told/.test(l)),
    `producer saw ${JSON.stringify(heard.map(h => `${h.f.t}${h.f.reason ? `(${h.f.reason})` : ''}${h.f.delivered ? `(${h.f.delivered} ref=${h.f.ref})` : ''}`))}; ${relay.said.filter(l => /let go|told/.test(l)).join(' | ')}`)
  void lastRefused
  relay.kill(); squatter.stop(true)
  await Bun.sleep(300)
}

// ── H. the project is switched off under a live door ──────────────────────────────────────────
//
// Since 5 September a `refused` can arrive on a LIVE connection: `not_enabled`, then a close, is
// what the hub sends a connected bridge when its project is switched off at a terminal. The door's
// refusal branch was written when a refusal only ever answered a `hello`, and said so — "no greeted
// producer is ended by this" — so it marked the link down FIRST, which ends every greeted producer,
// and wrote the reason afterwards into sockets that had already said goodbye. A tool server behind
// the door then saw only a dropped link, and its agent read "the hub went away" for a project the
// operator had deliberately turned off.
//
// RED, before the fix:
//   FAIL a_producer_greeted_by_the_door_is_told_its_project_is_off_before_its_socket_ends producer saw ["welcome"]
//   FAIL and_the_frame_the_hub_never_answered_is_not_left_hanging producer saw ["welcome"]
console.log('\nwhen the project is switched off under a live door:')

{
  const s = scenario('off', repo)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()
  const P = rawProducer(sock)
  await P.ready
  P.send(hello({ instance: 'the-one-switched-off' }))
  await until('the producer to be greeted', () => P.got.some(f => f.t === 'welcome'), 10000)
  // One frame the hub took and never answered, so the door is holding something for this producer
  // at the moment the switch is thrown — the ordinary state, not an empty one.
  s.hub!.ack = null
  P.send({ v: 1, id: 'p-say', t: 'say', text: 'still here?' })
  await until('the frame at the hub', () => s.hub!.got.some(f => f.t === 'say'), 10000)

  // What `hub.rs` does on the kick: the reason, then the close.
  s.hub!.to({ v: 1, id: 'h-off', t: 'refused', reason: 'not_enabled' })
  await Bun.sleep(150)
  s.hub!.drop()

  await until('the door to end the producer', () => !P.connected, 10000).catch(() => {})
  const heard = P.got.map(f => `${f.t}${f.reason ? `(${f.reason})` : ''}${f.delivered ? `(${f.delivered} ref=${f.ref})` : ''}`)
  check('a_producer_greeted_by_the_door_is_told_its_project_is_off_before_its_socket_ends',
    !P.connected && P.got.some(f => f.t === 'refused' && f.reason === 'not_enabled'),
    `producer saw ${JSON.stringify(heard)}`)
  check('and_the_frame_the_hub_never_answered_is_not_left_hanging',
    P.got.some(f => f.t === 'ack' && f.ref === 'p-say'),
    `producer saw ${JSON.stringify(heard)}`)

  s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

// ── I. the lease, and a tap nobody answered for ───────────────────────────────────────────────
//
// Two things arrived on the wire with the confirmed tap, and both of them are the DOOR's to get
// right, because the door is the only thing here the hub has ever spoken to.
//
// The lease is the dangerous one. It is minted per run of an address and it fences: a number the
// hub reads as higher than the one it handed out raises its floor for that address, and the
// project's real run is then refused for ever with nothing on the operator's phone. The door holds
// the connection, so the door's lease is the only true one — and a producer behind it, written by
// a stranger to the document, may stamp its own on everything it says. It must not travel.
//
// The tap is the other half. The hub now waits for a bridge that promised to confirm one, and
// edits his receipt with what it hears. The door promises for all of them, so every tap it hands
// on must come back with an answer or, where nobody can honestly give one, with nothing at all —
// which is the hub's own "not confirmed" line, and true.
//
// The lease is stripped in two places — at the door, where a producer's frame arrives, and at the
// writer, which deletes any `generation` in a payload before stamping the connection's own. Against
// a hub that GRANTS one they hide each other: the writer's stamp overwrites whatever the payload
// carried, so either can be deleted and the check below still passes. The check that can see them
// is the one against a hub that grants none — every hub built before the lease, which is the
// compatibility case the document promises — and it is the second block here.
//
// RED, before the fix:
//   FAIL the_door_promises_the_hub_it_will_answer_for_every_tap hello said undefined
//   FAIL a_tap_a_producer_took_is_answered_to_the_hub_as_taken no ack for the tap
//   FAIL a_tap_a_producer_refused_is_answered_with_its_own_reason no ack for the tap
//   FAIL a_tap_for_a_question_no_producer_holds_is_answered_rather_than_dropped no ack for the tap
//   FAIL a_producer_that_dies_holding_a_tap_has_the_door_answer_for_it no ack for the tap
console.log('\nwith a lease on the wire and a tap to answer for:')

{
  const s = scenario('lease', repo, { lease: 4242 })
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()

  const doorHello = s.hub!.got.find(f => f.t === 'hello')!
  check('the_door_promises_the_hub_it_will_answer_for_every_tap',
    Array.isArray(doorHello.confirms) && doorHello.confirms.includes('choice'),
    `hello said ${JSON.stringify(doorHello.confirms)}`)

  const P = rawProducer(sock)
  await P.ready
  // A producer that promises to answer for a tap, and that stamps a lease of its own on everything
  // it says — the shape a stranger's adapter has the moment it remembers its own welcome.
  P.send({ ...hello({ instance: 'the-one-with-a-lease', confirms: ['choice'] }), generation: 999999 })
  await until('the producer to be greeted', () => P.got.some(f => f.t === 'welcome'), 10000)

  const welcomed = P.got.find(f => f.t === 'welcome')!
  check('the_door_hands_a_producer_the_welcome_the_hub_sent_it_unchanged',
    welcomed.generation === 4242, JSON.stringify(welcomed))

  P.send({ v: 1, id: 'b1', generation: 999999, t: 'ask', ask_id: 'a1',
    text: 'ship it?', options: [{ option_id: 'y', label: 'Yes' }] })
  await until('the question at the hub', () => s.hub!.got.some(f => f.t === 'ask'), 10000)
  const asked = s.hub!.got.find(f => f.t === 'ask')!
  // The lease the hub reads is the DOOR's own — the one it granted this connection — and never the
  // producer's. It is not `undefined`: a door that stamped nothing would be a door the hub cannot
  // fence at all. This one passes with either strip deleted (the stamp overwrites the payload); the
  // block below is the one that holds them.
  check('the_lease_the_hub_reads_is_the_one_it_granted_this_door',
    asked.generation === 4242, `the hub read ${JSON.stringify(asked)}`)

  // Taken.
  s.hub!.to({ v: 1, id: 'h-tap-took', t: 'choice', msg_id: 'm1', ask_id: asked.ask_id, option_id: 'y' })
  await until('the tap at the producer', () => P.got.some(f => f.t === 'choice'), 10000)
  const tap = P.got.find(f => f.t === 'choice')!
  P.send({ v: 1, id: 'b2', t: 'ack', ref: tap.id, status: 'accepted' })
  await until('the answer at the hub',
    () => s.hub!.got.some(f => f.t === 'ack' && f.ref === 'h-tap-took'), 8000).catch(() => {})
  const took = s.hub!.got.find(f => f.t === 'ack' && f.ref === 'h-tap-took')
  check('a_tap_a_producer_took_is_answered_to_the_hub_as_taken',
    took?.status === 'accepted', took ? JSON.stringify(took) : 'no ack for the tap')

  // Refused, with a reason that must travel word for word: the hub puts it under his own receipt.
  P.send({ v: 1, id: 'b3', t: 'ask', ask_id: 'a2', text: 'and this?',
    options: [{ option_id: 'y', label: 'Yes' }] })
  await until('the second question at the hub', () => s.hub!.got.filter(f => f.t === 'ask').length >= 2, 10000)
  const asked2 = s.hub!.got.filter(f => f.t === 'ask')[1]
  s.hub!.to({ v: 1, id: 'h-tap-no', t: 'choice', msg_id: 'm2', ask_id: asked2.ask_id, option_id: 'y' })
  await until('the second tap at the producer', () => P.got.filter(f => f.t === 'choice').length >= 2, 10000)
  const tap2 = P.got.filter(f => f.t === 'choice')[1]
  P.send({ v: 1, id: 'b4', t: 'ack', ref: tap2.id, status: 'refused',
    reason: 'the worker had already closed that question' })
  await until('the second answer at the hub',
    () => s.hub!.got.some(f => f.t === 'ack' && f.ref === 'h-tap-no'), 8000).catch(() => {})
  const refused = s.hub!.got.find(f => f.t === 'ack' && f.ref === 'h-tap-no')
  check('a_tap_a_producer_refused_is_answered_with_its_own_reason',
    refused?.status === 'refused' && refused?.reason === 'the worker had already closed that question',
    refused ? JSON.stringify(refused) : 'no ack for the tap')

  // A tap for a question this door never asked. Dropped, the hub waits out its window and tells him
  // the session never confirmed it — when the truth is that nothing here could have.
  s.hub!.to({ v: 1, id: 'h-tap-orphan', t: 'choice', msg_id: 'm3', ask_id: 'nobody~asked-this', option_id: 'y' })
  await until('the orphan answered',
    () => s.hub!.got.some(f => f.t === 'ack' && f.ref === 'h-tap-orphan'), 8000).catch(() => {})
  const orphan = s.hub!.got.find(f => f.t === 'ack' && f.ref === 'h-tap-orphan')
  check('a_tap_for_a_question_no_producer_holds_is_answered_rather_than_dropped',
    orphan?.status === 'refused' && typeof orphan?.reason === 'string' && orphan.reason.length > 0,
    orphan ? JSON.stringify(orphan) : 'no ack for the tap')

  // The producer takes a tap and dies with it in its hand.
  P.send({ v: 1, id: 'b5', t: 'ask', ask_id: 'a3', text: 'last one?',
    options: [{ option_id: 'y', label: 'Yes' }] })
  await until('the third question at the hub', () => s.hub!.got.filter(f => f.t === 'ask').length >= 3, 10000)
  const asked3 = s.hub!.got.filter(f => f.t === 'ask')[2]
  s.hub!.to({ v: 1, id: 'h-tap-gone', t: 'choice', msg_id: 'm4', ask_id: asked3.ask_id, option_id: 'y' })
  await until('the third tap at the producer', () => P.got.filter(f => f.t === 'choice').length >= 3, 10000)
  P.end()
  await Bun.sleep(1500)
  const gone = s.hub!.got.find(f => f.t === 'ack' && f.ref === 'h-tap-gone')
  // Nobody said no. The socket died with the tap in the producer's hand, and this door cannot tell
  // "it never read the frame" from "it read it, acted on it, and died before saying so" — so
  // "not taken" is a sentence about a tap the worker very probably did take, and his receipt is
  // corrected with something nothing here knows. It is the argument this door already makes for a
  // producer that promised nothing, and the same one applies to a producer that promised and went:
  // the hub's own window closes on silence and says the session did not confirm it, which is true.
  check('a_tap_a_producer_died_holding_is_left_unconfirmed_rather_than_called_not_taken',
    gone === undefined, `the door said ${JSON.stringify(gone ?? null)}`)

  s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

{
  // The same producer against a hub that grants no lease at all — every hub built before the lease
  // existed, and the compatibility case §6 promises. Nothing overwrites the payload here, so a
  // number a producer stamped travels unless something takes it OUT: the door strips it off every
  // frame it forwards, and the writer deletes it before stamping. Either alone is enough; both are
  // kept because the door is where a stranger's number enters and one layer is not a proof.
  //
  // A number this hub never granted is the worst thing that can arrive on this wire: it raises the
  // floor for the address, and the project's real run is then refused for ever with nothing on the
  // operator's phone.
  //
  // RED, with both strips removed:
  //   FAIL a_producers_own_lease_number_never_reaches_a_hub_that_granted_the_door_none
  //        the hub read {"v":1,"id":"n1","generation":999999,"t":"ask","ask_id":"a1",...}
  const s = scenario('nolease', repo)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()
  const P = rawProducer(sock)
  await P.ready
  P.send({ ...hello({ instance: 'the-one-a-stranger-wrote', confirms: ['choice'] }), generation: 999999 })
  await until('the producer to be greeted', () => P.got.some(f => f.t === 'welcome'), 10000)
  P.send({ v: 1, id: 'n1', generation: 999999, t: 'ask', ask_id: 'a1',
    text: 'ship it?', options: [{ option_id: 'y', label: 'Yes' }] })
  await until('the question at the hub', () => s.hub!.got.some(f => f.t === 'ask'), 10000)
  const asked = s.hub!.got.find(f => f.t === 'ask')!
  check('a_producers_own_lease_number_never_reaches_a_hub_that_granted_the_door_none',
    asked.generation === undefined, `the hub read ${JSON.stringify(asked)}`)
  // And the door's own hello — the first frame of all, written before any welcome — claims none
  // either, so a hub that fences on what it reads has nothing to raise its floor with.
  const doorHello = s.hub!.got.find(f => f.t === 'hello')!
  check('and_the_door_itself_claims_no_lease_it_was_never_granted',
    doorHello.generation === undefined, `the hub read ${JSON.stringify(doorHello)}`)

  s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

// ── J. a worker too old to answer for a tap ───────────────────────────────────────────────────
//
// The door promises the hub it will answer for every tap, and it keeps that promise by folding
// what its producers say. A producer that never promised says nothing — and the door must not
// invent an answer on its behalf. "Taken" would be a guess; "not taken" would be a lie about a tap
// the producer very probably did take. Silence is the one true answer, and the hub already has a
// line for it: the session did not confirm it took the answer.
//
// RED, before the fix:
//   FAIL a_tap_taken_by_a_worker_too_old_to_answer_for_it_is_left_unconfirmed_rather_than_called_taken
//        (there was no fold at all, so this passed for the wrong reason — watched against the fold
//         with the promise check removed, where the door answered refused)
console.log('\nwith a worker too old to answer for a tap:')

{
  const s = scenario('old', repo)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()
  const P = rawProducer(sock)
  await P.ready
  P.send(hello({ instance: 'the-one-from-before-answers' }))
  await until('the producer to be greeted', () => P.got.some(f => f.t === 'welcome'), 10000)
  P.send({ v: 1, id: 'o1', t: 'ask', ask_id: 'a1', text: 'go?', options: [{ option_id: 'y', label: 'Yes' }] })
  await until('the question at the hub', () => s.hub!.got.some(f => f.t === 'ask'), 10000)
  const asked = s.hub!.got.find(f => f.t === 'ask')!
  s.hub!.to({ v: 1, id: 'h-tap-old', t: 'choice', msg_id: 'm1', ask_id: asked.ask_id, option_id: 'y' })
  await until('the tap at the producer', () => P.got.some(f => f.t === 'choice'), 10000)
  // It answers nothing, because its build has never heard of answering a tap. Then it goes, which
  // is the moment the door would otherwise speak for it.
  P.end()
  await Bun.sleep(1200)
  const spoken = s.hub!.got.find(f => f.t === 'ack' && f.ref === 'h-tap-old')
  check('a_tap_taken_by_a_worker_too_old_to_answer_for_it_is_left_unconfirmed_rather_than_called_taken',
    spoken === undefined, `the door said ${JSON.stringify(spoken ?? null)}`)

  s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

// ── K. a run a newer one replaced ───────────────────────────────────────────────
//
// Every other refusal this door does not recognise is treated as something waiting will mend, on
// purpose: a hub shipped after this build may refuse for a reason a person fixes at a terminal
// while the run lives. `stale_generation` is the one where waiting mends nothing — the hub has
// given this address to a NEWER run, and the document says in as many words that the way back is a
// new run and never a redial. A door that redials spins until somebody kills it, with its own
// socket still open in front of producers it can never carry anything for.
//
// The unit makes it ordinary rather than exotic: `Restart=always` means a restarted attach whose
// predecessor is still between sockets is two runs on one address, which is what the lease exists
// for. The loser stops, and whatever starts walls starts a new one.
//
// RED, before the fix:
//   FAIL a_door_a_newer_run_replaced_never_dials_again 4 hello(s)
//   FAIL and_it_stops_rather_than_holding_a_door_that_can_carry_nothing still running after 8s
//   FAIL a_greeted_producer_is_told_the_run_is_over_and_its_socket_ends still open
console.log('\nwith a newer run of the same project holding the address:')

/** A hub with one answer for every hello, so a door's whole dial loop is visible. */
function refusingHub(path: string, reason: string) {
  const hellos: Record<string, any>[] = []
  const server = Bun.listen({
    unix: path,
    socket: {
      open() {},
      data(sock: any, chunk: any) {
        for (const line of chunk.toString().split('\n')) {
          if (!line.trim()) continue
          const f = JSON.parse(line)
          if (f.t !== 'hello') continue
          hellos.push(f)
          sock.write(JSON.stringify({ v: 1, id: 'h-r', t: 'refused', reason }) + '\n')
          sock.end()
        }
      },
      close() {}, error() {},
    },
  })
  return { hellos, stop: () => server.stop(true) }
}

{
  // The control, and it must stay green: `not_enabled` is permanent for the connection and a PERSON
  // mends it at a terminal while this run lives, so the dial loop behind it is the recovery.
  const sock = join(dir, 'k-enabled.sock')
  const hub = refusingHub(sock, 'not_enabled')
  const relay = startAttach(laneDir, {
    KICKOFF_HUB_SOCKET: sock, KICKOFF_HUB_RELAY_DIR: join(dir, 'k-enabled-fanin'),
  }, true)
  await until('the door to dial once', () => hub.hellos.length >= 1, 15000)
  await Bun.sleep(8000)
  check('a_refusal_a_person_can_mend_while_the_run_lives_is_still_dialled_behind',
    hub.hellos.length > 1, `${hub.hellos.length} hello(s)`)
  relay.kill(); hub.stop()
  await Bun.sleep(300)
}

{
  const sock = join(dir, 'k-stale.sock')
  const hub = refusingHub(sock, 'stale_generation')
  const relay = startAttach(laneDir, {
    KICKOFF_HUB_SOCKET: sock, KICKOFF_HUB_RELAY_DIR: join(dir, 'k-stale-fanin'),
  }, true)
  await until('the door to dial once', () => hub.hellos.length >= 1, 15000)
  let stopped = false
  void relay.exited.then(() => { stopped = true })
  await until('the door to stop', () => stopped, 8000).catch(() => {})
  check('a_door_a_newer_run_replaced_never_dials_again',
    hub.hellos.length === 1, `${hub.hellos.length} hello(s)`)
  check('and_it_stops_rather_than_holding_a_door_that_can_carry_nothing',
    stopped, 'still running after 8s')
  relay.kill(); hub.stop()
  await Bun.sleep(300)
}

{
  // The same word on a LIVE link — the hub kicking a run whose address a newer one took. A producer
  // that has been greeted is holding an agent's turn open; it has to be ENDED, and it must not be
  // handed the word itself.
  //
  // The lease is a fact about THIS door's connection to the hub, and the producer holds no lease at
  // the hub at all. Passed down, it says something about the producer that is not true and that the
  // producer acts on for good: `server.ts` reads `stale_generation` and calls `giveUp`, which stops
  // its dial loop for the life of the session. The next run of this wall is five seconds away
  // (`deploy/kickoff-hub-attach@.service`, `Restart=always`), and a session that joined the relay
  // rather than being started by it is still alive to find it — unless it was told to stop looking.
  // A close is the truth the wire already has for this: the door went away, wait and dial again.
  //
  // RED, before the fix:
  //   FAIL and_is_never_told_the_word_that_would_stop_it_looking_for_the_next_run
  //        producer heard [{"t":"refused","reason":"stale_generation"}]
  const s = scenario('k-live', repo)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const sock = await s.sockOf()
  const P = rawProducer(sock)
  await P.ready
  P.send(hello({ instance: 'the-one-on-a-replaced-run', confirms: ['choice'] }))
  await until('the producer to be greeted', () => P.got.some(f => f.t === 'welcome'), 10000)
  s.hub!.to({ v: 1, id: 'h-stale', t: 'refused', reason: 'stale_generation' })
  await until('its socket to end', () => !P.connected, 8000).catch(() => {})
  check('a_greeted_producer_behind_a_replaced_run_has_its_socket_ended',
    !P.connected, P.connected ? 'still open' : 'ended')
  check('and_is_never_told_the_word_that_would_stop_it_looking_for_the_next_run',
    !P.got.some(f => f.t === 'refused' && f.reason === 'stale_generation'),
    `producer heard ${JSON.stringify(P.got.filter(f => f.t === 'refused'))}`)
  s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

{
  // The whole path the door's half is for: a REAL tool server behind a door whose run was replaced,
  // and the next run of the same wall. This is the session the finding is about — one that joined
  // the relay (`KICKOFF_HUB_RELAY=1`, the sentence `server.ts` prints for whoever starts one), so it
  // is not attach's child and outlives the run that was replaced.
  //
  // RED, before the fix:
  //   FAIL a_session_behind_a_door_that_was_replaced_finds_the_next_run_of_that_wall
  //        the next door carried 0 of its messages; not … yet — he has not seen this. It is waiting
  //        in line and goes out when the link to his phone comes back.
  const s = scenario('k-back', repo)
  await until('the relay to reach its hub', () => s.hub!.got.some(f => f.t === 'hello'), 15000)
  const via = { KICKOFF_HUB_SOCKET: s.hubSock, KICKOFF_HUB_RELAY_DIR: s.faninDir, KICKOFF_HUB_RELAY: '1' }
  const A = startServer({ CLAUDE_PROJECT_DIR: repo, ...via })
  await handshake(A, CLAUDE_CODE.capabilities, CLAUDE_CODE.clientInfo)
  const before = await call(A, 'reply', { text: 'before the wall was replaced' })
  check('a session behind the door reaches him while its run holds the address', before.text === 'said', before.text)

  s.hub!.to({ v: 1, id: 'h-stale2', t: 'refused', reason: 'stale_generation' })
  let stopped = false
  void s.relay.exited.then(() => { stopped = true })
  await until('the replaced run to stop', () => stopped, 10000).catch(() => {})

  // What the unit does five seconds later, on the same address and the same door directory. Started
  // as the WALL starts it — `KICKOFF_HUB_RELAY` is the producer's variable, and it is what tells a
  // session to join a door rather than to be one.
  const next = startAttach(repo, {
    KICKOFF_HUB_SOCKET: s.hubSock, KICKOFF_HUB_RELAY_DIR: s.faninDir,
  }, true)
  await until('the next run to reach the hub',
    () => s.hub!.got.filter(f => f.t === 'hello').length >= 2, 20000).catch(() => {})
  const after = await call(A, 'reply', { text: 'after the wall came back' })
  await until('the message to reach the hub through the next run',
    () => s.hub!.got.some(f => f.t === 'say' && f.text === 'after the wall came back'), 30000).catch(() => {})
  check('a_session_behind_a_door_that_was_replaced_finds_the_next_run_of_that_wall',
    s.hub!.got.some(f => f.t === 'say' && f.text === 'after the wall came back'),
    `the next door carried ${s.hub!.got.filter(f => f.t === 'say').length - 1} of its messages; ${after.text}`)

  A.child.kill(); next.kill(); s.relay.kill(); s.hub!.stop()
  await Bun.sleep(300)
}

rmSync(dir, { recursive: true, force: true })
const n = failed()
console.log(`\n${n === 0 ? 'all checks passed' : `${n} FAILED`}`)
process.exit(n === 0 ? 0 : 1)
