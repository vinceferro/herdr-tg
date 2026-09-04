/**
 * The rig both fan-in suites run on: a fake hub, real producers, real relays.
 *
 * It lives in its own file because there are now two suites — the design (`test-two-producers.ts`)
 * and the failures four reviewers found (`test-what-breaks-it.ts`) — and a second copy of a fake
 * hub is how two suites come to disagree about what the wire is. That is not a hypothetical here:
 * the opencode bridge forked this project's link once and drifted by thirteen fixed defects.
 *
 * What is real in both suites: the relay process, the tool-server processes, their MCP stdio
 * handshakes, the Unix sockets and the framing on all of them. Only the hub is faked, because a
 * test that needed a bot token would never run — and the operator's phone is not a test fixture.
 */

import { mkdirSync, writeFileSync } from 'fs'
import { join } from 'path'

export const HERE = import.meta.dir
export const SERVER = join(HERE, '..', '..', 'plugins', 'kickoff-channel', 'server.ts')
export const FANIN = join(HERE, 'fanin.ts')

let failures = 0
export const check = (name: string, ok: boolean, detail = '') => {
  if (ok) console.log(`  ok   ${name}`)
  else { console.log(`  FAIL ${name} ${detail}`); failures++ }
}
export const failed = () => failures

export const until = async (what: string, cond: () => boolean, ms = 10000) => {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    if (cond()) return
    await Bun.sleep(25)
  }
  throw new Error(`timed out waiting for: ${what}`)
}

/**
 * A repository with a real linked worktree.
 *
 * `--git-common-dir` is the fact the whole address rests on, and a fake of it would prove nothing.
 */
export function makeRepo(dir: string, laneName: string) {
  const repo = join(dir, 'repo')
  mkdirSync(join(repo, '.kickoff'), { recursive: true })
  writeFileSync(join(repo, '.kickoff', 'hub.token'), 'a'.repeat(64), { mode: 0o600 })
  const git = (...args: string[]) => {
    const r = Bun.spawnSync(['git', '-C', repo, ...args], { stdout: 'ignore', stderr: 'ignore' })
    if (r.exitCode !== 0) { console.log(`git ${args.join(' ')} failed`); process.exit(1) }
  }
  git('init', '-q')
  git('-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-q', '--allow-empty', '-m', 'x')
  const laneDir = join(dir, laneName)
  git('worktree', 'add', '-q', laneDir, '-b', `lane/${laneName}`)
  return { repo, laneDir, lane: laneName }
}

/**
 * A hub that admits ONE live connection per (repo, lane) and refuses the second, which is the rule
 * `hub.rs` actually enforces and the only reason any of this exists.
 */
export function claimingHub(path: string) {
  type Conn = { s: any; acc: string; addr?: string }
  const conns = new Map<any, Conn>()
  const claims = new Map<string, any>()
  const got: Record<string, any>[] = []
  const refusals: string[] = []
  /** What to answer every relayed frame with. `null` means answer nothing, like a hub gone quiet. */
  let ackAs: 'yes' | 'no' | null = 'yes'
  let claiming = true

  const write = (s: any, o: Record<string, unknown>) => s.write(JSON.stringify(o) + '\n')

  const server = Bun.listen({
    unix: path,
    socket: {
      open(s: any) { conns.set(s, { s, acc: '' }) },
      data(s: any, chunk: any) {
        const c = conns.get(s)!
        c.acc += chunk.toString()
        for (;;) {
          const nl = c.acc.indexOf('\n')
          if (nl < 0) break
          const line = c.acc.slice(0, nl)
          c.acc = c.acc.slice(nl + 1)
          if (!line.trim()) continue
          const f = JSON.parse(line)
          got.push(f)
          if (f.t === 'hello') {
            const addr = `${f.repo} ${f.lane ?? ''}`
            if (claiming && claims.has(addr) && conns.has(claims.get(addr))) {
              refusals.push('already_claimed')
              write(s, { v: 1, id: 'h-r', t: 'refused', reason: 'already_claimed' })
              s.end()
              continue
            }
            claims.set(addr, s)
            c.addr = addr
            write(s, {
              v: 1, id: 'h-w', t: 'welcome', project: 'repo',
              ...(f.lane ? { lane: f.lane } : {}),
              limits: { max_frame: 262144, max_text: 3500, frames_per_min: 20 },
            })
            continue
          }
          if (f.t === 'pong' || f.t === 'bye') continue
          if (ackAs) write(s, { v: 1, id: `h-a${got.length}`, t: 'ack', ref: f.id, delivered: ackAs,
            ...(ackAs === 'no' ? { why: 'too-fast' } : {}) })
        }
      },
      close(s: any) {
        const c = conns.get(s)
        if (c?.addr && claims.get(c.addr) === s) claims.delete(c.addr)
        conns.delete(s)
      },
      error() {},
    },
  })

  return {
    got,
    refusals,
    /** The connection the relay is holding — there is only ever one. */
    to: (o: Record<string, unknown>) => write([...conns.keys()][0], o),
    /** How many connections are open right now, so a test can see the relay come and go. */
    get live() { return conns.size },
    set ack(v: 'yes' | 'no' | null) { ackAs = v },
    set enforcing(v: boolean) { claiming = v },
    stop: () => server.stop(true),
  }
}

/** One real tool server, started the way an engine starts it. */
export function startServer(env: Record<string, string>, cwd = HERE) {
  const child = Bun.spawn(['bun', SERVER], {
    cwd, env: { ...process.env, ...env }, stdin: 'pipe', stdout: 'pipe', stderr: 'inherit',
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
        const l = acc.slice(0, nl); acc = acc.slice(nl + 1)
        if (l.trim()) { try { out.push(JSON.parse(l)) } catch { /* not json */ } }
      }
    }
  })()
  return { child, out, to: (o: unknown) => child.stdin.write(JSON.stringify(o) + '\n') }
}

export type Srv = ReturnType<typeof startServer>

/**
 * The two real engines introducing themselves, captured from the real clients.
 *
 * A producer that hands over `{name: 'test'}` is an engine the bridge does not recognise, and the
 * bridge answers those the careful way on purpose — so a test that used the placeholder was reading
 * the unknown-engine wording no matter which engine it was standing in for, and could not have
 * caught a sentence that was wrong for one of them.
 */
export const CLAUDE_CODE = {
  capabilities: { roots: { listChanged: true }, elicitation: {} },
  clientInfo: { name: 'claude-code', title: 'Claude Code', version: '2.1.250' },
}
export const OPENCODE = { capabilities: { roots: {} }, clientInfo: { name: 'opencode', version: '1.18.25' } }

export async function handshake(b: Srv, capabilities: Record<string, unknown> = {},
                                clientInfo = { name: 'test', version: '0' }) {
  b.to({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {
    protocolVersion: '2024-11-05', capabilities, clientInfo } })
  await until('the MCP handshake', () => b.out.some(l => l.id === 1))
  b.to({ jsonrpc: '2.0', method: 'notifications/initialized' })
}

let callId = 100
export async function call(b: Srv, name: string, args: unknown) {
  const id = ++callId
  b.to({ jsonrpc: '2.0', id, method: 'tools/call', params: { name, arguments: args } })
  await until(`${name} to return`, () => b.out.some(l => l.id === id))
  const r = b.out.find(l => l.id === id)!.result
  return { text: String(r.content?.[0]?.text ?? ''), isError: r.isError === true }
}

export const channelMessages = (b: Srv) => b.out.filter(l => l.method === 'notifications/claude/channel')
export const noticesTo = (b: Srv) =>
  channelMessages(b).filter(l => l.params?.meta?.user === 'the channel itself')

/**
 * The relay, as its own process.
 *
 * Its stderr is captured rather than inherited when asked for, because some of what it does — a tap
 * that reached nobody, a question withdrawn because its asker never came back — is reportable ONLY
 * where a developer can see it, and a test that could not read it would be asserting the absence of
 * a crash instead of the presence of a note.
 */
export function startFanin(projectDir: string, env: Record<string, string>, capture = false) {
  const child = Bun.spawn(['bun', FANIN], {
    cwd: HERE,
    env: { ...process.env, KICKOFF_HUB_PROJECT_DIR: projectDir, ...env },
    stdout: 'inherit', stderr: capture ? 'pipe' : 'inherit',
  })
  const said: string[] = []
  if (capture) {
    ;(async () => {
      const dec = new TextDecoder()
      let acc = ''
      for await (const chunk of child.stderr as any) {
        acc += dec.decode(chunk)
        for (;;) {
          const nl = acc.indexOf('\n')
          if (nl < 0) break
          const l = acc.slice(0, nl); acc = acc.slice(nl + 1)
          said.push(l)
          console.log(`    [relay] ${l}`)
        }
      }
    })()
  }
  return Object.assign(child, { said })
}

/** A producer with no MCP in it, for the frames a real one will not send. */
export function rawProducer(sock: string) {
  const got: Record<string, any>[] = []
  let acc = ''
  let s: any = null
  let open = false
  const ready = Bun.connect({
    unix: sock,
    socket: {
      open(x: any) { s = x; open = true },
      data(_x: any, chunk: any) {
        acc += chunk.toString()
        for (;;) {
          const nl = acc.indexOf('\n')
          if (nl < 0) break
          const l = acc.slice(0, nl); acc = acc.slice(nl + 1)
          if (l.trim()) got.push(JSON.parse(l))
        }
      },
      close() { s = null; open = false }, error() {},
    },
  })
  return {
    got,
    ready,
    get connected() { return open },
    send: (o: Record<string, unknown>) => s?.write(JSON.stringify(o) + '\n'),
    end: () => s?.end(),
  }
}
