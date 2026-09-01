#!/usr/bin/env bun
/**
 * The plugin, against a fake hub. The mirror image of the Rust side's
 * `an_ask_becomes_a_tap_becomes_a_choice`, which tests the hub against a fake bridge.
 *
 *     bun test-against-a-fake-hub.ts
 *
 * What is real: the plugin process, its MCP stdio handshake, the Unix socket, and the framing.
 * What is faked is the hub, because a test that needed a bot token would never run.
 */

import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'

const dir = mkdtempSync(join(tmpdir(), 'kc-'))
const sock = join(dir, 'hub.sock')
const repo = join(dir, 'repo')
mkdirSync(join(repo, '.kickoff'), { recursive: true })
writeFileSync(join(repo, '.kickoff', 'hub.token'), 'a'.repeat(64), { mode: 0o600 })

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

const child = Bun.spawn(['bun', 'server.ts'], {
  cwd: import.meta.dir,
  env: { ...process.env, KICKOFF_HUB_SOCKET: sock, KICKOFF_CHANNEL_REPO: repo },
  stdin: 'pipe',
  stdout: 'pipe',
  stderr: 'inherit',
})

const toPlugin = (o: unknown) => child.stdin.write(JSON.stringify(o) + '\n')
const outLines: Record<string, any>[] = []
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
      if (l.trim()) { try { outLines.push(JSON.parse(l)) } catch { /* not json */ } }
    }
  }
})()

const until = async (what: string, cond: () => boolean, ms = 8000) => {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    if (cond()) return
    await Bun.sleep(25)
  }
  throw new Error(`timed out waiting for: ${what}`)
}

let failures = 0
const check = (name: string, ok: boolean, detail = '') => {
  if (ok) console.log(`  ok   ${name}`)
  else { console.log(`  FAIL ${name} ${detail}`); failures++ }
}

toPlugin({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {
  protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'test', version: '0' } } })
await until('the MCP handshake', () => outLines.some(l => l.id === 1))
toPlugin({ jsonrpc: '2.0', method: 'notifications/initialized' })

await until('hello', () => fromBridge.some(f => f.t === 'hello'))
const hello = fromBridge.find(f => f.t === 'hello')!
check('it says hello with the secret from the repo', hello.token === 'a'.repeat(64))
check('the envelope carries the protocol version', hello.v === 1)
check('hello carries no display name', !('name' in hello) && !('title' in hello))
check('hello names its own repo and pid', hello.repo === repo && typeof hello.pid === 'number')

toBridge({ v: 1, id: 'h-ping-1', t: 'ping' })
await until('pong', () => fromBridge.some(f => f.t === 'pong'))
check('a pong names the ping it answers', fromBridge.find(f => f.t === 'pong')!.ref === 'h-ping-1')

toPlugin({ jsonrpc: '2.0', id: 2, method: 'tools/list' })
await until('the tool list', () => outLines.some(l => l.id === 2))
const tools = outLines.find(l => l.id === 2)!.result.tools.map((t: any) => t.name).sort()
check('it offers reply, ask, done and ask_resolved',
  JSON.stringify(tools) === JSON.stringify(['ask','ask_resolved','done','reply']), JSON.stringify(tools))

const t0 = Date.now()
toPlugin({ jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'ask', arguments: {
  text: 'Overwrite deploy/prod.yaml?', options: [{ id: 'y', label: 'Yes' }, { id: 'n', label: 'No' }] } } })
await until('the ask to return', () => outLines.some(l => l.id === 3))
check('ask returns without waiting for the operator', Date.now() - t0 < 2000)
await until('the ask frame', () => fromBridge.some(f => f.t === 'ask'))
const ask = fromBridge.find(f => f.t === 'ask')!
check('the question reaches the hub verbatim', ask.text === 'Overwrite deploy/prod.yaml?')
check('both options travel with it', ask.options?.length === 2 && ask.options[0].option_id === 'y')

toPlugin({ jsonrpc: '2.0', id: 4, method: 'tools/call', params: { name: 'ask', arguments: {
  text: 'ok?', options: [{ id: 'a|b', label: 'Yes' }] } } })
await until('the refusal', () => outLines.some(l => l.id === 4))
check('an option id containing "|" is refused', outLines.find(l => l.id === 4)!.result.isError === true)

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

child.kill()
hub.stop()
rmSync(dir, { recursive: true, force: true })
console.log(`\n${failures === 0 ? 'all checks passed' : `${failures} FAILED`}`)
process.exit(failures === 0 ? 0 : 1)
