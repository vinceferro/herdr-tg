#!/usr/bin/env bun
/**
 * `--check`, against fakes.
 *
 *     bun test-check.ts
 *
 * The one property that matters most is that a check creates NO topic: it dials, is welcomed, and
 * closes WITHOUT ponging, because the hub makes the topic only after a pong. So the fake hub here
 * records every frame it is sent and the checks assert it saw `hello` then `bye` and never a `pong`.
 * The rest are the sentences §13.4 promises for every way this can go wrong — a wrong user, no
 * socket, a silent close, each refusal, a name the hub will not address, a secret handed by value.
 *
 * And the other property a check has to keep: what it blesses, the worker can do. Three environments
 * the start refused with exit 2 got "everything a worker here needs is in place" from the first
 * version of this command, and a wrapper that trusted the check started a worker that died at once
 * — so the refusals the start makes are proven here to be the check's `NOT` lines too.
 */

import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'fs'
import { join } from 'path'

import { relaySocketPath } from '../../plugins/kickoff-channel/attach.ts'
import { seedIdOf } from '../../plugins/kickoff-channel/where.ts'
import { makeRepo } from './test-harness.ts'

const ATTACH = join(import.meta.dir, 'main.ts')

let failures = 0
function check(what: string, ok: boolean, detail = ''): void {
  if (ok) console.log(`  ok   ${what}`)
  else {
    console.log(`  FAIL ${what}${detail ? `  ${detail}` : ''}`)
    failures++
  }
}

const dir = mkdtempSync('/tmp/ck-')
const { repo, laneDir, lane } = makeRepo(dir, 'lane-0904-check')

/** Run `main.ts --check` with an env, capturing its lines and exit code. */
async function runCheck(env: Record<string, string>, args: string[] = [], cwd = repo): Promise<{ code: number; out: string[] }> {
  const child = Bun.spawn(['bun', ATTACH, '--check', ...args], {
    cwd,
    env: { ...process.env, ...env },
    stdout: 'pipe',
    stderr: 'inherit',
  })
  const text = await new Response(child.stdout).text()
  const code = await child.exited
  return { code, out: text.split('\n').filter(l => l.length) }
}

/**
 * A hub that welcomes, pings, and REMEMBERS every frame it is sent — so a test can prove a pong was
 * never sent and therefore no topic could have been made. It creates nothing.
 */
function recordingHub(path: string, opts: { refuse?: string; silentAfterHello?: boolean; mute?: boolean; project?: string } = {}) {
  const seen: Record<string, any>[] = []
  const server = Bun.listen({
    unix: path,
    socket: {
      open() {},
      data(s: any, chunk: any) {
        for (const line of chunk.toString().split('\n')) {
          if (!line.trim()) continue
          const f = JSON.parse(line)
          seen.push(f)
          if (f.t !== 'hello') continue
          // A hub that accepts and never speaks — running, but wedged.
          if (opts.mute) continue
          if (opts.silentAfterHello) {
            // The silent close: read the hello, say nothing, and close — which from outside is a
            // uid mismatch and a malformed hello both at once.
            s.end()
            continue
          }
          if (opts.refuse) {
            s.write(JSON.stringify({ v: 1, id: 'h-r', t: 'refused', reason: opts.refuse }) + '\n')
            s.end()
            continue
          }
          s.write(JSON.stringify({
            v: 1, id: 'h-w', t: 'welcome', project: opts.project ?? 'the-fake-project',
            ...(f.lane ? { lane: f.lane } : {}),
            limits: { max_frame: 262144, max_text: 3500, frames_per_min: 20 },
          }) + '\n')
          // The ping the check must NOT answer. If it did, a real hub would make the topic.
          s.write(JSON.stringify({ v: 1, id: 'h-ping', t: 'ping' }) + '\n')
        }
      },
      close() {},
      error() {},
    },
  })
  return { seen, stop: () => server.stop(true) }
}

const relayDir = join(dir, 'fanin')

// ── A. all well: reachable, admitted, and NO topic made ───────────────────────────────────────
console.log('\nwhen everything a worker needs is in place:')
{
  const hubSock = join(dir, 'a-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('check_proves_reachability_without_creating_a_topic',
    r.code === 0 &&
      r.out.some(l => /^ok\s+reached the hub/.test(l)) &&
      r.out.some(l => /^ok\s+admitted as "the-fake-project"/.test(l)) &&
      hub.seen.some(f => f.t === 'hello') &&
      hub.seen.some(f => f.t === 'bye') &&
      !hub.seen.some(f => f.t === 'pong'),
    `code ${r.code}; hub saw ${JSON.stringify(hub.seen.map(f => f.t))}`)
  check('and it names the conversation git derived and the door, free',
    r.out.some(l => new RegExp(`the conversation: ${lane} \\(git's name`).test(l)) &&
      r.out.some(l => /^ok\s+the door:.*free/.test(l)),
    JSON.stringify(r.out))
  check('and the last line says everything is in place',
    r.out.at(-1) === 'everything a worker here needs is in place', JSON.stringify(r.out.at(-1)))
  hub.stop()
}

// ── A2. the real hub's title for a conversation already names the address ─────────────────────
//
// `welcome.project` from the real hub is the registry's title for the CONVERSATION — the project's
// title, a separator, and the address clipped from the left with an ellipsis (`lane_title` in
// `registry.rs`). The first version of this command appended the address again after it, so a real
// lane read `admitted as "hub-dogfood · …fy-0904-check" · verify-0904-check`, which a stranger reads
// as a bug. The fake hubs above send a bare title, which is why the doubling never showed there.
console.log('\nwhen the hub names the conversation in its own title:')
{
  const hubSock = join(dir, 'a2-hub.sock')
  const hub = recordingHub(hubSock, { project: 'hub-dogfood · …fy-0904-check' })
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_ADDRESS: 'verify-0904-check',
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('the admitted line is the hub\'s own title and nothing after it',
    r.out.some(l => l === 'ok   admitted as "hub-dogfood · …fy-0904-check"'),
    JSON.stringify(r.out.filter(l => /admitted/.test(l))))
  hub.stop()
}

// ── B. the socket refuses this user ───────────────────────────────────────────────────────────
console.log('\nwhen the socket refuses this user (EACCES):')
{
  // A real listening socket, then stripped of every permission so connect() is denied — the shape
  // rootful docker at a foreign kuid hits against the 0600 hub socket.
  const hubSock = join(dir, 'b-hub.sock')
  const hub = recordingHub(hubSock)
  chmodSync(hubSock, 0o000)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('check_names_the_wrong_user_in_words_when_the_socket_refuses',
    r.code === 1 && r.out.some(l => /run as the same user as the hub/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
  chmodSync(hubSock, 0o600)
  hub.stop()
}

// ── C. nothing is listening ─────────────────────────────────────────────────────────────────────
console.log('\nwhen there is no hub socket at all:')
{
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: join(dir, 'not-there.sock'),
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('a missing socket says the hub is not running or its directory is not mounted',
    r.code === 1 && r.out.some(l => /nothing at .*not-there\.sock/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
}

// ── D. the hub takes the hello and closes without a word ──────────────────────────────────────
console.log('\nwhen the hub reads the hello and closes silently:')
{
  const hubSock = join(dir, 'd-hub.sock')
  const hub = recordingHub(hubSock, { silentAfterHello: true })
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('a silent close names the two indistinguishable causes',
    r.code === 1 && r.out.some(l => /closed without a word/.test(l) && /malformed/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
  hub.stop()
}

// ── E. every refusal the hub gives, beside a live door ───────────────────────────────────────
//
// Beside a LIVE door on purpose: dialling it takes a round trip, which is exactly the yield the
// first version of this command needed to print a second, contradictory line after a refusal —
// "the hub took the hello and closed without a word … not running as the hub's user" — because the
// wire reported the close that FOLLOWS a refusal as a silent one. With no door on disk the process
// exited before the close event landed and the phantom never showed, which is how the original
// refusal case passed. The operator's own situation — running the check beside his worker — is a
// held claim AND a live door, so it is the shape asserted here for every reason.
console.log('\nwhen the hub refuses, beside a live door:')
{
  const { mkdirSync: mk } = await import('fs')
  mk(relayDir, { recursive: true, mode: 0o700 })
  // Keyed on the CONVERSATION — the id the registry mints for this repo — not the repo path.
  const doorPath = relaySocketPath(relayDir, seedIdOf(repo), lane)
  const holder = Bun.listen({ unix: doorPath, socket: { open() {}, data() {}, close() {}, error() {} } })
  const sentences: Record<string, RegExp> = {
    unknown_project: /the hub does not know this project.*herdr-tg open/,
    bad_token: /the secret is not one the hub knows/,
    not_enabled: /enrolled but switched off/,
    version_skew: /do not speak the same version/,
    bad_lane: /will not address a conversation called/,
    already_claimed: /another connection holds this conversation right now/,
  }
  for (const [reason, sentence] of Object.entries(sentences)) {
    const hubSock = join(dir, `e-${reason}.sock`)
    const hub = recordingHub(hubSock, { refuse: reason })
    const r = await runCheck({
      KICKOFF_HUB_PROJECT_DIR: laneDir,
      KICKOFF_HUB_SOCKET: hubSock,
      KICKOFF_HUB_RELAY_DIR: relayDir,
    })
    const nots = r.out.filter(l => l.startsWith('NOT'))
    check(`${reason} is one NOT line with its own sentence, and the held door is the only other`,
      r.code === 1 && nots.length === 2 && nots.some(l => sentence.test(l)) &&
        nots.some(l => /another attach already holds the door/.test(l)) &&
        !nots.some(l => /closed without a word/.test(l)),
      JSON.stringify(nots))
    hub.stop()
  }
  holder.stop(true)
}

// ── F. a name the hub will not address — caught before the hub is dialled ─────────────────────
console.log('\nwhen the conversation name has a slash in it:')
{
  const hubSock = join(dir, 'f-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_ADDRESS: 'CEO/steering',
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('a bad address is refused, and the hub is never dialled',
    r.code === 1 && r.out.some(l => /NOT/.test(l) && /slash/.test(l)) && hub.seen.length === 0,
    `hub saw ${hub.seen.length} frame(s); ${JSON.stringify(r.out.filter(l => l.startsWith('NOT')))}`)
  hub.stop()
}

// ── G. a secret handed by value — refused before the hub is dialled ───────────────────────────
console.log('\nwhen the secret is handed by value:')
{
  const hubSock = join(dir, 'g-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_TOKEN: 'a'.repeat(64),
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('KICKOFF_HUB_TOKEN is refused, and the hub is never dialled',
    r.code === 1 && r.out.some(l => /KICKOFF_HUB_TOKEN is set.*never travels as a value/.test(l)) && hub.seen.length === 0,
    `hub saw ${hub.seen.length}; ${JSON.stringify(r.out.filter(l => l.startsWith('NOT')))}`)
  hub.stop()
}

// ── H. a token FILE that is the secret itself ─────────────────────────────────────────────────
console.log('\nwhen the token-file variable holds the secret instead of a path:')
{
  const hubSock = join(dir, 'h-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_TOKEN_FILE: 'a'.repeat(64),
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('a 64-hex token file is named for what it is',
    r.code === 1 && r.out.some(l => /looks like the secret itself/.test(l)) && hub.seen.length === 0,
    `hub saw ${hub.seen.length}; ${JSON.stringify(r.out.filter(l => l.startsWith('NOT')))}`)
  hub.stop()
}

// ── I. the door is already held ───────────────────────────────────────────────────────────────
console.log('\nwhen a worker already holds the door:')
{
  const hubSock = join(dir, 'i-hub.sock')
  const hub = recordingHub(hubSock)
  // A live socket at the derived door, so `--check`'s door fact finds it held. The check dials it
  // like the door-binder does, so a plain listener is enough to read as "held".
  const { relaySocketPath } = await import('../../plugins/kickoff-channel/attach.ts')
  const doorPath = relaySocketPath(relayDir, seedIdOf(repo), lane)
  const { mkdirSync } = await import('fs')
  mkdirSync(relayDir, { recursive: true, mode: 0o700 })
  const holder = Bun.listen({ unix: doorPath, socket: { open() {}, data() {}, close() {}, error() {} } })
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('the door fact says another attach already holds it',
    r.code === 1 && r.out.some(l => /another attach already holds the door/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
  holder.stop(true)
  hub.stop()
}

// ── J. what the check blesses, the worker can do: KICKOFF_HUB_RELAY on attach itself ──────────
//
// The worker refuses `KICKOFF_HUB_RELAY=1` in its own environment (it would dial its own door). The
// pinned environment of every `--run` child carries that variable, so a worker started from a shell
// inside a worker inherits it — and the first check said "everything is in place" to it.
console.log('\nwhen KICKOFF_HUB_RELAY is set on attach itself:')
{
  const hubSock = join(dir, 'j-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
    KICKOFF_HUB_RELAY: '1',
  })
  check('the_check_refuses_what_the_start_refuses_a_relay_flag_on_attach_itself',
    r.code === 1 && r.out.some(l => /^NOT\s+KICKOFF_HUB_RELAY is set on attach itself/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
  hub.stop()
}

// ── K. what the check blesses, the worker can do: a private door under a TMPDIR that is not one ──
//
// Under `--run` with no git and no door named, the worker makes a private door under TMPDIR and
// refuses a TMPDIR that is not absolute — this box hands every agent session the literal string
// `%h/.cache/tmp`. The first check printed "will be made in a private folder" without looking.
console.log('\nwhen --run would need a private door and TMPDIR is not a path:')
{
  const noGit = join(dir, 'walled')
  mkdirSync(join(noGit, '.kickoff'), { recursive: true })
  writeFileSync(join(noGit, '.kickoff', 'hub.token'), 'a'.repeat(64), { mode: 0o600 })
  const hubSock = join(dir, 'k-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: noGit,
    KICKOFF_HUB_SOCKET: hubSock,
    TMPDIR: '%h/.cache/tmp',
  }, ['--run', 'true'], noGit)
  check('the_check_refuses_what_the_start_refuses_a_tmpdir_no_private_door_can_be_made_under',
    r.code === 1 && r.out.some(l => /^NOT\s+TMPDIR is "%h\/.cache\/tmp", which is not an absolute path/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))

  // And with a real TMPDIR the same line is the promise it was.
  const ok = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: noGit,
    KICKOFF_HUB_SOCKET: hubSock,
    TMPDIR: dir,
  }, ['--run', 'true'], noGit)
  check('and with a real TMPDIR the door will be made there',
    ok.out.some(l => /^ok\s+the door: will be made in a private folder under/.test(l)),
    JSON.stringify(ok.out.filter(l => /door/.test(l))))
  hub.stop()
}

// ── L. --opencode is read for what it can verify: a URL with a port ───────────────────────────
//
// The unit expands `${OPENCODE_PORT}` into the URL; with the variable missing the URL is
// `http://127.0.0.1:` — no port — and a watcher on port 80 beside a server on 4096 held the claim,
// got a topic, and delivered nothing, with a green check in front of it.
console.log('\nwhen --opencode names no port:')
{
  const hubSock = join(dir, 'l-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  }, ['--opencode', 'http://127.0.0.1:'])
  check('the_check_refuses_what_the_start_refuses_an_opencode_url_with_no_port',
    r.code === 1 && r.out.some(l => /^NOT\s+--opencode http:\/\/127\.0\.0\.1: names no port/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
  const ok = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  }, ['--opencode', 'http://127.0.0.1:9711'])
  check('and a whole URL is its own ok line',
    ok.code === 0 && ok.out.some(l => l === 'ok   the engine\'s address: http://127.0.0.1:9711'),
    JSON.stringify(ok.out.filter(l => /address/.test(l))))
  hub.stop()
}

// ── L2. an opencode engine with no --opencode is half a phone, and the check says so ─────────
//
// Without `--opencode`, an opencode worker's questions, its permission prompts and what the operator
// types on his phone all reach nothing: the watcher is what carries every one of them, and the tool
// server the engine spawns can only say what the agent chose to say. The start warns too, from the
// same sentence, so the check and the worker cannot disagree.
console.log('\nwhen the engine is opencode and no --opencode was given:')
{
  const hubSock = join(dir, 'l2-hub.sock')
  const hub = recordingHub(hubSock)
  const env = { KICKOFF_HUB_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: relayDir }
  const r = await runCheck(env, ['--run', 'opencode', 'serve', '--port', '9711'])
  check('the_check_warns_that_an_opencode_engine_started_without_opencode_reaches_half_a_phone',
    r.out.some(l => /^warn\s+.*--opencode/.test(l)),
    JSON.stringify(r.out))
  const ok = await runCheck(env, ['--opencode', 'http://127.0.0.1:9711', '--run', 'opencode', 'serve', '--port', '9711'])
  check('and says nothing of the kind when --opencode is there',
    !ok.out.some(l => /^warn\s+.*--opencode/.test(l)),
    JSON.stringify(ok.out.filter(l => /^warn/.test(l))))
  hub.stop()
}

// ── M. the hub's directory is 0700, and this is not the hub's user ────────────────────────────
//
// The real directory is `drwx------`, so a foreign uid cannot even stat the socket inside it. The
// first check turned every stat failure into "the hub is not running, or the directory is not
// mounted" — the one fix that is not the fix. The same user with the directory at mode 000
// produces the identical error (EACCES on the stat), so the suite needs no second uid.
console.log('\nwhen the hub\'s directory cannot be looked into:')
{
  const locked = join(dir, 'locked')
  mkdirSync(locked, { recursive: true })
  const hubSock = join(locked, 'hub.sock')
  const hub = recordingHub(hubSock)
  chmodSync(locked, 0o000)
  try {
    const r = await runCheck({
      KICKOFF_HUB_PROJECT_DIR: laneDir,
      KICKOFF_HUB_SOCKET: hubSock,
      KICKOFF_HUB_RELAY_DIR: relayDir,
    })
    check('a directory this user may not look into says to run as the hub\'s user',
      r.code === 1 && r.out.some(l => /may not look inside .*locked\/.*run as the same user as the hub/.test(l)) &&
        !r.out.some(l => /is not mounted here/.test(l)),
      JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
  } finally {
    chmodSync(locked, 0o700)
  }
  hub.stop()
}

// ── N. a reader refusal is printed for a person at a terminal, not an agent in a tool result ──
console.log('\nwhen nothing named a project directory:')
{
  const r = await runCheck({ KICKOFF_HUB_SOCKET: join(dir, 'none.sock'), CLAUDE_PROJECT_DIR: '' })
  check('the sentence names the variable, in the register main.ts dies with',
    r.code === 1 && r.out.some(l => /^NOT\s+nothing named a project directory \(KICKOFF_HUB_PROJECT_DIR\)/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
}

// ── O. a hub that accepts and says nothing ────────────────────────────────────────────────────
console.log('\nwhen the hub accepts and never speaks:')
{
  const hubSock = join(dir, 'o-hub.sock')
  const hub = recordingHub(hubSock, { mute: true })
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
  })
  check('a mute hub is named as running but wedged, with the fix',
    r.code === 1 && r.out.some(l => /said nothing for 6 seconds.*wedged.*restart herdr-tg/.test(l)) &&
      !r.out.some(l => /settling window/.test(l)),
    JSON.stringify(r.out.filter(l => l.startsWith('NOT'))))
  hub.stop()
}

// ── P. the door was told, so the tool server must be told the same ────────────────────────────
//
// The address IS git's name here, so "use the worktree's own name as the address" — the first
// remedy the old line offered — was a wild-goose chase; the mismatch is because the door was named.
console.log('\nwhen the door was named by KICKOFF_HUB_RELAY_SOCKET:')
{
  const hubSock = join(dir, 'p-hub.sock')
  const hub = recordingHub(hubSock)
  const r = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: laneDir,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_DIR: relayDir,
    KICKOFF_HUB_RELAY_SOCKET: join(dir, 'told.sock'),
  })
  const line = r.out.find(l => /tool server/.test(l)) ?? ''
  check('with git present, the line says to give the engine the same door and does not blame the address',
    /^ok\s+/.test(line) && /give the engine the same KICKOFF_HUB_RELAY_SOCKET=.*told\.sock/.test(line) &&
      !/worktree's own name/.test(line),
    JSON.stringify(line))
  const noGit = join(dir, 'walled')
  const r2 = await runCheck({
    KICKOFF_HUB_PROJECT_DIR: noGit,
    KICKOFF_HUB_SOCKET: hubSock,
    KICKOFF_HUB_RELAY_SOCKET: join(dir, 'told2.sock'),
  }, [], noGit)
  const line2 = r2.out.find(l => /tool server/.test(l)) ?? ''
  check('with no git and no --run, a told door is not a NOT',
    r2.code === 0 && /^ok\s+/.test(line2) && /must be given the same KICKOFF_HUB_RELAY_SOCKET=.*told2\.sock/.test(line2),
    `code ${r2.code}; ${JSON.stringify(r2.out.filter(l => /tool server|NOT/.test(l)))}`)
  hub.stop()
}

// ── Q. told a conversation: what a tool server working from git would find ────────────────────
//
// `doorDerivedFromGit` keyed the comparison door on the conversation attach was TOLD, so the
// check compared attach's door with itself and said a tool server working from git "finds this
// one". A tool server that works from git is, by definition, not told a conversation: hand-started
// beside a bare attach it derives the seed's door. Under `--run` the child is handed the
// conversation, and only then is the sentence true.
console.log('\nwhen attach is told a conversation:')
{
  const xdg = join(dir, 'q-xdg')
  const room = 'c-0e0e0e0e0e0e'
  mkdirSync(join(xdg, 'herdr-tg', 'conversations', room), { recursive: true, mode: 0o700 })
  writeFileSync(join(xdg, 'herdr-tg', 'conversations', room, 'secret'), 'q'.repeat(64), { mode: 0o600 })
  const hubSock = join(dir, 'q-hub.sock')
  const hub = recordingHub(hubSock)
  const env = { KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: relayDir,
    KICKOFF_HUB_CONVERSATION: room, XDG_STATE_HOME: xdg }
  const r = await runCheck(env)
  const line = r.out.find(l => /tool server/.test(l)) ?? ''
  check('without --run, a tool server working from git would find another door, and the check says which variable to give it',
    /^NOT\s+/.test(line) && /KICKOFF_HUB_CONVERSATION=c-0e0e0e0e0e0e/.test(line),
    JSON.stringify(line))
  const r2 = await runCheck(env, ['--run', 'true'])
  const line2 = r2.out.find(l => /tool server/.test(l)) ?? ''
  check('under --run the child is handed the conversation, so a tool server there finds this door',
    /^ok\s+/.test(line2) && /finds this one/.test(line2),
    JSON.stringify(line2))
  hub.stop()
}

// ── R. a secret the channel keeps that the hub refuses ─────────────────────────────────────────
//
// A stale channel copy — left by a rotation typed with a herdr-tg from before conversations
// existed — arrives as `unknown_project`, and the sentence sent whoever read it to `open`, which
// says "already open". The verb that mends it is adopt-secrets.
console.log('\nwhen the secret the channel keeps is one the hub refuses:')
{
  const xdg = join(dir, 'r-xdg')
  const id = seedIdOf(repo)
  const { createHash } = await import('crypto')
  const { realpathSync } = await import('fs')
  const key = createHash('sha256').update(realpathSync(repo)).digest('hex').slice(0, 16)
  mkdirSync(join(xdg, 'herdr-tg', 'conversations', id), { recursive: true, mode: 0o700 })
  writeFileSync(join(xdg, 'herdr-tg', 'conversations', id, 'secret'), 'b'.repeat(64), { mode: 0o600 })
  mkdirSync(join(xdg, 'herdr-tg', 'by-repo'), { recursive: true, mode: 0o700 })
  writeFileSync(join(xdg, 'herdr-tg', 'by-repo', key), `${id}\n`, { mode: 0o600 })
  const hubSock = join(dir, 'r-hub.sock')
  const hub = recordingHub(hubSock, { refuse: 'unknown_project' })
  const r = await runCheck({ KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: relayDir, XDG_STATE_HOME: xdg })
  check('a bound secret the hub refuses names adopt-secrets --apply, not open',
    r.out.some(l => /the secret: .*bound to/.test(l)) &&
      r.out.some(l => /^NOT\s+.*adopt-secrets --apply/.test(l)) &&
      !r.out.some(l => /^NOT\s+.*herdr-tg open/.test(l)),
    JSON.stringify(r.out.filter(l => /secret|NOT/.test(l))))
  hub.stop()
}

rmSync(dir, { recursive: true, force: true })
console.log(failures === 0 ? '\nall checks passed' : `\n${failures} FAILED`)
process.exit(failures === 0 ? 0 : 1)
