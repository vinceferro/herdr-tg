/**
 * A fake opencode server as a PROCESS, so something that is not a bun test can drive one.
 *
 * This is a fixture, not a command. Nothing on a real box starts it, it is not in `deploy/`, and it
 * is not a second thing to run under `adapters/` — there is still exactly one of those, `main.ts`.
 * It exists because the hermetic fleet trial is a Rust test (the only place a hub with a faked
 * Telegram can be built at all — `herdr-tg` ships no library) and the fake engine the real adapter
 * watches is TypeScript. A pipe is the whole of the seam between them.
 *
 * It starts one `fakeOpencode()` — the same one every bun suite here uses, so a fleet trial and the
 * adapter's own suites cannot come to disagree about what an opencode server does — prints where it
 * is, and then does exactly what it is told, one line at a time. It decides nothing: it starts no
 * process, reads no binding, resolves no spec and chooses no session. Every one of those belongs to
 * whoever dispatches, and putting any of them here would make this repo a fleet controller.
 *
 *     bun fake-engine.ts
 *     < {"url":"http://127.0.0.1:41234"}
 *     > {"sessions":[{"id":"ses_a","directory":"/tmp/repo","updated":1788607585115}]}
 *     < {"ok":true}
 *     > {"push":{"type":"question.v2.asked","properties":{…}}}
 *     < {"ok":true}
 *     > {"dump":true}
 *     < {"ok":true,"posted":[{"path":"/session/ses_a/prompt_async","body":{…}}],"watchers":4}
 *     > {"stop":true}
 *
 * Every command is answered with one line, and the answer is the whole point: a driver that pushed
 * an event and then slept would be timing the fixture rather than waiting for it.
 */

import { aSession, fakeOpencode } from './fake-opencode.ts'

const oc = fakeOpencode()

/** One line out, flushed, because a driver blocked on reading it is what everything here waits on. */
const say = (o: unknown) => {
  process.stdout.write(JSON.stringify(o) + '\n')
}

say({ url: oc.url })

type Command = {
  /** The sessions this server lists, as `{id, directory, updated, extra?}`. Replaces the set. */
  sessions?: { id: string; directory: string; updated: number; extra?: Record<string, unknown> }[]
  /** What a turn ran under, by message id: `{"<messageID>": {id, role, agent}}`. Merged in. */
  messages?: Record<string, Record<string, unknown>>
  /** One event, pushed to every subscribed watcher verbatim. */
  push?: unknown
  /** Answer with everything posted to this server so far, and how many watchers are subscribed. */
  dump?: boolean
  /** Stop the server and exit 0. */
  stop?: boolean
}

const obey = (c: Command): Record<string, unknown> => {
  if (c.sessions) {
    oc.sessions = c.sessions.map(s => aSession(s.id, s.directory, s.updated, s.extra ?? {}))
  }
  if (c.messages) {
    for (const [id, info] of Object.entries(c.messages)) oc.messages.set(id, info)
  }
  if (c.push !== undefined) oc.push(c.push)
  if (c.dump) return { posted: oc.posted, watchers: oc.watchers }
  return {}
}

for await (const line of console) {
  const text = line.trim()
  if (text.length === 0) continue
  let c: Command
  try {
    c = JSON.parse(text) as Command
  } catch {
    // Said rather than swallowed: a driver whose command did not parse would otherwise wait for an
    // answer that is never coming, and time out somewhere far from the line it got wrong.
    say({ ok: false, why: 'that is not one JSON object on a line' })
    continue
  }
  if (c.stop) {
    say({ ok: true })
    break
  }
  say({ ok: true, ...obey(c) })
}

oc.stop()
process.exit(0)
