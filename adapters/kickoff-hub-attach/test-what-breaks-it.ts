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

import { existsSync, mkdtempSync, readdirSync, rmSync } from 'fs'
import { join } from 'path'

import {
  CLAUDE_CODE, call, check, claimingHub, failed, handshake, makeRepo, rawProducer, startAttach,
  startServer, until,
} from './test-harness.ts'

const dir = mkdtempSync('/tmp/fb-')
const LANE = 'lane-0903-201500-9999'
const { repo, laneDir } = makeRepo(dir, LANE)

/** One relay, one fake hub, one address — a scenario that cannot disturb the next one. */
function scenario(name: string, projectDir: string, opts: { hub?: boolean; env?: Record<string, string> } = {}) {
  const hubSock = join(dir, `${name}.sock`)
  const faninDir = join(dir, `${name}-fanin`)
  const hub = opts.hub === false ? null : claimingHub(hubSock)
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
    /herdr-tg/.test(after.text) && !/being rebuilt/.test(after.text), after.text)

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

rmSync(dir, { recursive: true, force: true })
const n = failed()
console.log(`\n${n === 0 ? 'all checks passed' : `${n} FAILED`}`)
process.exit(n === 0 ? 0 : 1)
