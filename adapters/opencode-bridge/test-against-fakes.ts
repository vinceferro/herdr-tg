/**
 * The real bridge, against a fake hub on a real Unix socket and a fake opencode over real HTTP.
 *
 * Nothing here is mocked inside the bridge's own process: it is spawned as `bun bridge.ts`, exactly
 * as it will run, and everything is observed from outside. A test that reached into the module
 * would prove the mapping and miss the two things that have actually broken this project — the
 * handshake, and what goes on the wire before `welcome`.
 *
 *     bun test-against-fakes.ts
 */

import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'

let failures = 0
function check(what: string, ok: boolean, detail?: string): void {
  if (ok) {
    console.log(`  ok   ${what}`)
  } else {
    failures++
    console.log(`  FAIL ${what}${detail ? `\n       ${detail}` : ''}`)
  }
}

const SECRET = 'a'.repeat(64)

/** Frames the fake hub has received, in order. */
const seen: Record<string, any>[] = []
/** Requests the fake opencode has received, in order. */
const posted: { path: string; body: any }[] = []

// ── the fake hub ───────────────────────────────────────────────────────────────────────────────

const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'ocb-'))
const sockPath = join(dir, 'hub.sock')
const repo = join(dir, 'repo')
mkdirSync(join(repo, '.kickoff'), { recursive: true })
writeFileSync(join(repo, '.kickoff', 'hub.token'), SECRET)

let hubSock: import('bun').Socket | null = null
/** Set once the fake hub has sent `welcome`, so a test can prove nothing arrived before it. */
let welcomed = false
let beforeWelcome = 0

const hub = Bun.listen({
  unix: sockPath,
  socket: {
    open(s) {
      hubSock = s
    },
    data(s, chunk) {
      for (const line of chunk.toString().split('\n')) {
        if (!line.trim()) continue
        const f = JSON.parse(line)
        if (!welcomed && f.t !== 'hello') beforeWelcome++
        seen.push(f)
        if (f.t === 'hello') {
          // Resolve by secret, exactly as the hub does: the name comes from the registry, never
          // from anything the bridge sent.
          const ok = f.token === SECRET
          if (!ok) {
            s.write(JSON.stringify({ v: 1, id: 'h0', t: 'refused', reason: 'bad_token' }) + '\n')
            return
          }
          s.write(
            JSON.stringify({
              v: 1,
              id: 'h1',
              t: 'welcome',
              project: 'the-fake-project',
              limits: { max_frame: 65536, max_text: 4000, frames_per_min: 18 },
            }) + '\n',
          )
          welcomed = true
          return
        }
        // Every frame gets exactly one ack.
        s.write(JSON.stringify({ v: 1, id: `a${seen.length}`, t: 'ack', ref: f.id, delivered: 'yes' }) + '\n')
      }
    },
  },
})

/** Push a tap down to the bridge, the way a real tap arrives. */
function tap(askId: string, optionId: string): void {
  hubSock?.write(
    JSON.stringify({ v: 1, id: 'h9', t: 'choice', msg_id: 'm1', ask_id: askId, option_id: optionId }) + '\n',
  )
}

// ── the fake opencode ──────────────────────────────────────────────────────────────────────────

let pushEvent: ((e: unknown) => void) | null = null

const oc = Bun.serve({
  port: 0,
  hostname: '127.0.0.1',
  async fetch(req) {
    const url = new URL(req.url)
    if (url.pathname === '/event') {
      return new Response(
        new ReadableStream({
          start(c) {
            const enc = new TextEncoder()
            pushEvent = e => c.enqueue(enc.encode(`data: ${JSON.stringify(e)}\n\n`))
          },
        }),
        { headers: { 'content-type': 'text/event-stream' } },
      )
    }
    if (req.method === 'POST') {
      posted.push({ path: url.pathname, body: await req.json() })
      return new Response('{}', { headers: { 'content-type': 'application/json' } })
    }
    return new Response('not found', { status: 404 })
  },
})

// ── run the real bridge ────────────────────────────────────────────────────────────────────────

const child = Bun.spawn(['bun', join(import.meta.dir, 'bridge.ts')], {
  env: {
    ...process.env,
    OPENCODE_BRIDGE_REPO: repo,
    KICKOFF_HUB_SOCKET: sockPath,
    OPENCODE_URL: `http://127.0.0.1:${oc.port}`,
  },
  stdout: 'pipe',
  stderr: 'pipe',
})

const until = async (what: string, cond: () => boolean, ms = 5000): Promise<boolean> => {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    if (cond()) return true
    await new Promise(r => setTimeout(r, 25))
  }
  console.log(`  (gave up waiting for ${what})`)
  return false
}

const frame = (t: string) => seen.find(f => f.t === t)
const frames = (t: string) => seen.filter(f => f.t === t)

try {
  console.log('the bridge and the hub agree on the handshake')
  check('it says hello with the secret and no name of its own', await until('hello', () => !!frame('hello')))
  check('the hello carries no display name', frame('hello') !== undefined && !('title' in (frame('hello') ?? {})))
  check('nothing at all went out before the hub said welcome', beforeWelcome === 0, `${beforeWelcome} frame(s) did`)

  console.log('\na question opencode asks becomes a question on the phone')
  await until('the stream', () => pushEvent !== null)
  pushEvent!({
    type: 'question.v2.asked',
    data: {
      id: 'que_1',
      sessionID: 'ses_1',
      questions: [
        {
          question: 'Delete the staging database?',
          header: 'Destructive',
          options: [
            { label: 'Delete it', description: 'goes away' },
            { label: 'Leave it', description: 'stays' },
          ],
        },
      ],
    },
  })
  check('it arrives as an ask', await until('the ask', () => !!frame('ask')))
  const ask = frame('ask')
  check('it carries the question opencode published', ask?.text?.startsWith('Delete the staging database?') === true, JSON.stringify(ask?.text))
  check(
    'its buttons are the labels opencode published',
    JSON.stringify(ask?.options?.map((o: any) => o.label)) === JSON.stringify(['Delete it', 'Leave it']),
    JSON.stringify(ask?.options),
  )

  console.log('\na tap goes back to opencode as the label it published')
  tap(ask.ask_id, ask.options[0].option_id)
  check('opencode is told', await until('the reply', () => posted.length > 0))
  check(
    'at the endpoint the spec names for a v2 question',
    posted[0]?.path === '/api/session/ses_1/question/que_1/reply',
    posted[0]?.path,
  )
  check(
    'carrying the label, not the button id',
    JSON.stringify(posted[0]?.body) === JSON.stringify({ answers: [['Delete it']] }),
    JSON.stringify(posted[0]?.body),
  )
  check('and only then do the buttons come off', await until('the retirement', () => !!frame('ask_resolved')))

  console.log('\na tap on a question that is already answered answers nothing twice')
  const before = posted.length
  tap(ask.ask_id, ask.options[0].option_id)
  await new Promise(r => setTimeout(r, 300))
  check('nothing more is posted to opencode', posted.length === before, `${posted.length - before} were`)

  console.log('\na permission request offers only the three answers opencode accepts')
  pushEvent!({
    type: 'permission.v2.asked',
    data: { id: 'per_1', sessionID: 'ses_1', action: 'run a command', resources: ['rm -rf /'] },
  })
  check('it arrives as an ask', await until('the permission ask', () => frames('ask').length > 1))
  const perm = frames('ask')[1]
  check('it says what is wanted, in plain words', perm?.text?.includes('run a command') === true, perm?.text)
  check(
    'three buttons and no more',
    perm?.options?.length === 3 && JSON.stringify(perm.options.map((o: any) => o.option_id)) === JSON.stringify(['once', 'always', 'reject']),
    JSON.stringify(perm?.options),
  )
  // Tapped while the question is still open, so this proves the option lookup refuses it — not
  // merely that the record was already consumed, which is what an answered question would prove.
  const beforeBogus = posted.length
  tap(perm.ask_id, 'an-option-nobody-minted')
  await new Promise(r => setTimeout(r, 300))
  check('a tap naming an option it never offered posts nothing', posted.length === beforeBogus, `${posted.length - beforeBogus} did`)

  tap(perm.ask_id, 'reject')
  check('a tap replies at the permission endpoint', await until('the permission reply', () => posted.length > before + 0 && posted.some(p => p.path.includes('permission'))))
  const pr = posted.find(p => p.path.includes('permission'))
  check('with the enum opencode accepts', JSON.stringify(pr?.body) === JSON.stringify({ reply: 'reject' }), JSON.stringify(pr?.body))

  console.log('\na question answered at the keyboard has its buttons taken off the phone')
  pushEvent!({
    type: 'question.v2.asked',
    data: {
      id: 'que_2',
      sessionID: 'ses_1',
      questions: [{ question: 'Ship it?', header: 'Ship', options: [{ label: 'Yes', description: 'go' }] }],
    },
  })
  await until('the second question', () => frames('ask').length > 2)
  const retiredBefore = frames('ask_resolved').length
  pushEvent!({ type: 'question.v2.replied', data: { sessionID: 'ses_1', requestID: 'que_2', answers: [['Yes']] } })
  check(
    'the keyboard is retired without anyone tapping it',
    await until('the second retirement', () => frames('ask_resolved').length > retiredBefore),
  )
} finally {
  child.kill()
  hub.stop(true)
  oc.stop(true)
  rmSync(dir, { recursive: true, force: true })
}

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} FAILED`)
process.exit(failures === 0 ? 0 : 1)
