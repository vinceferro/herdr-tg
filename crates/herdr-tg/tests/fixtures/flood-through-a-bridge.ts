// The shared wire — `plugins/kickoff-channel/hub-link.ts` — driven exactly as `server.ts` drives
// it (`identify`, `markUp` on `welcome`, `forgetInFlight` on `ack`), with a backlog queued BEFORE
// it dials, which is what a bridge that outlived a hub restart is holding.
//
// WHICH build of the wire is named by `FLOOD_LINK`. The Rust test that starts this points it at
// the file as it shipped before the hub learned to answer for what it destroys — the build running
// in the operator's session until his next restart — and at the current one, and compares what
// each reports against the same hub. A driver that imported the current file by path would only
// ever prove the two halves were changed together.
//
// It prints ONE line of JSON and exits. Nothing here is a test; the assertions are in
// `a_bridge_from_before_this_change_still_works_against_the_new_hub`.
import { readFileSync } from 'node:fs'

const { HubLink } = await import(process.env.FLOOD_LINK!)
const socket = process.env.FLOOD_SOCKET!
const token = readFileSync(process.env.FLOOD_TOKEN_FILE!, 'utf8').trim()
const N = Number(process.env.FLOOD_N ?? 64)
const SIZE = Number(process.env.FLOOD_SIZE ?? 60000)
const WAIT = Number(process.env.FLOOD_WAIT_MS ?? 5000)

const acks: Record<string, number> = { yes: 0, no: 0, unseen: 0 }
const seen = new Set<string>()
const ackedTwice: string[] = []
const refused: string[] = []
let welcomes = 0
let lost = 0
let unanswered = 0

const link = new HubLink({
  identify: () => ({
    socket,
    hello: {
      t: 'hello',
      project_id: 'unknown-until-the-hub-says',
      token,
      instance: 'flood-1',
      repo: '/flood',
      pid: process.pid,
    },
  }),
  onFrame: (f: Record<string, any>) => {
    if (f.t === 'welcome') {
      welcomes++
      link.markUp()
    } else if (f.t === 'ack') {
      const ref = String(f.ref)
      if (seen.has(ref)) ackedTwice.push(ref)
      seen.add(ref)
      acks[String(f.delivered)] = (acks[String(f.delivered)] ?? 0) + 1
      link.forgetInFlight(ref)
    } else if (f.t === 'refused') {
      refused.push(String(f.reason))
      link.markDown(false, String(f.reason))
    }
  },
  onLost: (l: unknown[]) => {
    lost += l.length
  },
  // Unknown to the build from before this change, which ignores it: that build reports nothing here
  // and keeps the frames on its books, and `owed` below is what shows it.
  onUnanswered: (g: unknown[]) => {
    unanswered += g.length
  },
  note: () => {},
  whenUnreachable: 'unreachable',
  whenDropped: 'dropped',
  framePrefix: 'f',
})

const ids: string[] = []
for (let i = 0; i < N; i++) {
  ids.push(link.send({ t: 'say', text: 'x'.repeat(SIZE), hint: 'prose' }, `message ${i}`).id)
}
link.start()

setTimeout(() => {
  console.log(
    JSON.stringify({
      welcomes,
      acks,
      acked_twice: ackedTwice,
      lost,
      unanswered,
      refused,
      // Frames this build still thinks the hub owes an answer for. For the build from before this
      // change that is its blind spot: flushed whole, never answered, never reported.
      owed: ids.filter(id => link.frameInFlight(id)).length,
      up: link.state.up,
    }),
  )
  process.exit(0)
}, WAIT)
