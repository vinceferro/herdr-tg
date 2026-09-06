#!/usr/bin/env bun
/**
 * `--run`, the lifecycle of §13.5.
 *
 *     bun test-run.ts
 *
 * A child that exits takes attach down with the child's status, after a `bye`. A signal forwarded to
 * a child that ignores it is followed by a kill. The child is started in the project directory, with
 * the eight namespace variables pinned and the door among them, so any adapter descending from it
 * finds the door — which is why the stranger written from the document alone, given as the "engine"
 * with no git and no door named, is welcomed through a private door and heard. A command that cannot
 * be started is `127`. And attach refuses to be PID 1 with a child, because nothing would reap for it.
 */

import { existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, rmSync, writeFileSync } from 'fs'
import { join } from 'path'

import { OPENCODE, SERVER, call, claimingHub, handshake, makeRepo, rawProducer, startServer, until } from './test-harness.ts'

const ATTACH = join(import.meta.dir, 'main.ts')
const STRANGER = join(import.meta.dir, '..', '..', 'docs', 'examples', 'attach-from-the-document.ts')

let failures = 0
function check(what: string, ok: boolean, detail = ''): void {
  if (ok) console.log(`  ok   ${what}`)
  else {
    console.log(`  FAIL ${what}${detail ? `  ${detail}` : ''}`)
    failures++
  }
}

const dir = mkdtempSync('/tmp/rn-')
const { repo, laneDir } = makeRepo(dir, 'lane-0904-run')
// A short, absolute temporary directory for the private-door path, so this suite does not depend on
// whatever TMPDIR the box hands an agent session (which this repo has measured as a 110-byte
// non-path that would blow the 108-byte socket limit).
const shortTmp = join(dir, 't')
mkdirSync(shortTmp, { recursive: true })

/**
 * Spawn attach with `--run`, capturing its child's stdout and attach's own exit code.
 *
 * attach's own stderr is captured when asked for, for the same reason `startAttach` captures it:
 * a false alarm in the one log a developer reads to find real ones is reportable only there, and
 * a test that could not read it would be asserting the absence of a crash instead of the absence
 * of a line.
 */
function startRun(env: Record<string, string>, runCmd: string[], capture = false) {
  const child = Bun.spawn(['bun', ATTACH, '--run', ...runCmd], {
    cwd: repo,
    env: { ...process.env, TMPDIR: shortTmp, KICKOFF_HUB_PROJECT_DIR: repo, ...env },
    stdout: 'pipe',
    stderr: capture ? 'pipe' : 'inherit',
  })
  let out = ''
  ;(async () => {
    const dec = new TextDecoder()
    for await (const chunk of child.stdout as any) out += dec.decode(chunk)
  })()
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
          console.log(`    [attach] ${l}`)
        }
      }
    })()
  }
  return { child, out: () => out, said }
}

// ── A. a child that dies takes attach down with its status, and a bye ─────────────────────────
console.log('\nwhen the child exits on its own:')
{
  const hubSock = join(dir, 'a-hub.sock')
  const hub = claimingHub(hubSock)
  const r = startRun(
    { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: join(dir, 'a-fanin') },
    ['sh', '-c', 'sleep 0.4; exit 7'],
    true,
  )
  await until('attach to reach the hub', () => hub.got.some(f => f.t === 'hello'), 15000)
  const code = await r.child.exited
  await Bun.sleep(100)
  check('a_child_that_dies_takes_attach_down_with_its_status_and_a_bye',
    code === 7 && hub.got.some(f => f.t === 'bye'),
    `attach exit ${code}; hub saw ${JSON.stringify(hub.got.map(f => f.t))}`)
  // The hub acks the `bye` like every other frame. The door minted that frame for itself, so the
  // ack is its own to recognise — reported as "no producer is waiting on this", it is a false
  // alarm in the one log a developer reads to find the real ones, at the end of every single run.
  check('attachs_own_goodbye_is_acked_by_the_hub_and_attach_does_not_report_the_ack_as_lost',
    !r.said.some(l => /an ack named .*no producer is waiting on/.test(l)),
    JSON.stringify(r.said.filter(l => /an ack named/.test(l))))
  hub.stop()
}

// ── A2. the engine's exit takes the buttons off every question the door still holds ──────────
//
// A producer that goes away gets a grace period before its questions are withdrawn, because it may
// be a tool server restarting in place. Under `--run` the ENGINE's exit is a different fact: nothing
// behind the door can answer any more, attach itself is about to exit, and the grace timer dies
// with it — so the question kept its buttons on his phone with nothing behind them, and a tap
// went to a lane that no longer existed.
console.log('\nwhen the engine exits with a question still open:')
{
  const hubSock = join(dir, 'a2-hub.sock')
  const hub = claimingHub(hubSock)
  const door = join(shortTmp, 'a2-door.sock')
  const flag = join(dir, 'a2-engine-may-exit')
  const r = startRun(
    { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_SOCKET: door,
      // Long enough that the ordinary grace timer cannot fire and fake a pass.
      KICKOFF_HUB_RELAY_GRACE_MS: '30000' },
    // An engine that exits ON ITS OWN, when told to, rather than on a signal from the test.
    ['sh', '-c', `while [ ! -e "${flag}" ]; do sleep 0.05; done; exit 0`],
    true,
  )
  await until('attach to reach the hub', () => hub.got.some(f => f.t === 'hello'), 15000)
  const P = rawProducer(door)
  await P.ready
  P.send({ v: 1, id: 'h1', t: 'hello', project_id: 'p', token: 'a'.repeat(64), repo,
    pid: process.pid, instance: 'the-engines-tool-server' })
  P.send({ v: 1, id: 'p-ask', t: 'ask', ask_id: 'a1', text: 'ship it?',
    options: [{ option_id: 'y', label: 'Yes' }] })
  await until('the question at the hub', () => hub.got.some(f => f.t === 'ask'), 15000)
  const atHub = hub.got.find(f => f.t === 'ask')!.ask_id as string
  // The tool server goes first, as it does for real: the engine's MCP children end before the
  // engine's own process does.
  P.end()
  await until('the door to notice it went', () => r.said.some(l => /went away/.test(l)), 10000)
  writeFileSync(flag, '')
  const code = await r.child.exited
  await Bun.sleep(100)
  const kinds = hub.got.map(f => f.t)
  const withdrawn = hub.got.find(f => f.t === 'ask_resolved')
  check('when_the_engine_exits_every_question_the_door_still_holds_is_withdrawn_before_the_bye',
    code === 0 && withdrawn?.ask_id === atHub && withdrawn?.how === 'withdrawn' &&
      withdrawn?.outcome === 'the session that asked has ended' &&
      kinds.indexOf('ask_resolved') < kinds.indexOf('bye'),
    `attach exit ${code}; hub saw ${JSON.stringify(hub.got.filter(f => f.t !== 'hello'))}`)
  // What the door wrote down beside its socket is what the next attach routes and withdraws from.
  // A question withdrawn here but still written there would have its buttons taken off twice.
  let remembered: any = null
  try { remembered = JSON.parse(readFileSync(`${door}.state`, 'utf8')) } catch { /* asserted below */ }
  check('and the door does not write the withdrawn question down for the next run to withdraw again',
    Array.isArray(remembered?.asks) && remembered.asks.length === 0, JSON.stringify(remembered))
  hub.stop()
}

// ── B. a signal is forwarded, and its death becomes attach's status ───────────────────────────
console.log('\nwhen attach is told to stop and the child obeys:')
{
  const hubSock = join(dir, 'b-hub.sock')
  const hub = claimingHub(hubSock)
  const r = startRun(
    { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: join(dir, 'b-fanin') },
    ['sleep', '60'],
  )
  await until('attach to reach the hub', () => hub.got.some(f => f.t === 'hello'), 15000)
  r.child.kill('SIGTERM')
  const code = await r.child.exited
  check('a_forwarded_signal_that_kills_the_child_becomes_128_plus_that_signal',
    code === 143, `attach exit ${code} (wanted 143 = 128 + SIGTERM)`)
  hub.stop()
}

// ── C. a child that ignores the signal is killed after the wait ───────────────────────────────
console.log('\nwhen the child ignores the signal:')
{
  const hubSock = join(dir, 'c-hub.sock')
  const hub = claimingHub(hubSock)
  const r = startRun(
    { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: join(dir, 'c-fanin'),
      // Test-only: shorten the ten-second grace so this does not wait it out.
      KICKOFF_HUB_ATTACH_STOP_MS: '600' },
    ['sh', '-c', 'trap "" TERM; sleep 60'],
  )
  await until('attach to reach the hub', () => hub.got.some(f => f.t === 'hello'), 15000)
  r.child.kill('SIGTERM')
  const code = await r.child.exited
  check('a_child_that_ignores_the_signal_is_killed_and_attach_exits_137',
    code === 137, `attach exit ${code} (wanted 137 = 128 + SIGKILL)`)
  hub.stop()
}

// ── D. the child's environment and cwd ────────────────────────────────────────────────────────
console.log('\nwhat the child inherits:')
{
  const hubSock = join(dir, 'd-hub.sock')
  const hub = claimingHub(hubSock)
  const relayDir = join(dir, 'd-fanin')
  const r = startRun(
    { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: relayDir },
    ['sh', '-c', 'pwd; env | grep ^KICKOFF_HUB_'],
  )
  const code = await r.child.exited
  await Bun.sleep(50)
  const lines = r.out().split('\n')
  const seen = (re: RegExp) => lines.some(l => re.test(l))
  check('the child runs in the project directory',
    lines[0] === laneDir || lines[0] === repo, `pwd was ${lines[0]}`)
  check('the_run_child_inherits_the_eight_pinned_variables_and_the_door',
    // All eight pinned, RELAY on, and the door named. The tool server inherits exactly this.
    seen(/^KICKOFF_HUB_PROJECT_DIR=/) && seen(/^KICKOFF_HUB_ADDRESS=/) &&
      seen(/^KICKOFF_HUB_TOKEN_FILE=/) && seen(/^KICKOFF_HUB_SOCKET=/) &&
      seen(/^KICKOFF_HUB_RELAY=1$/) && seen(/^KICKOFF_HUB_RELAY_SOCKET=.+\.sock$/) &&
      seen(/^KICKOFF_HUB_RELAY_DIR=-$/) && seen(/^KICKOFF_HUB_RELAY_GRACE_MS=-$/),
    JSON.stringify(lines.filter(l => l.startsWith('KICKOFF_HUB_'))))
  // The secret's path is the one attach RESOLVED, not `-` because nothing told it: attach found it
  // by the upward search, and a child that does not search (the stranger, §5) must not have to.
  check('and the secret is pinned as the path attach found, not left for the child to find again',
    lines.includes(`KICKOFF_HUB_TOKEN_FILE=${join(repo, '.kickoff', 'hub.token')}`),
    JSON.stringify(lines.filter(l => l.startsWith('KICKOFF_HUB_TOKEN_FILE'))))
  check('and it exits cleanly', code === 0, `exit ${code}`)
  hub.stop()
}

// ── D2. the child of a ROOM inherits the conversation, and the overlay of §2 cannot lose it ────
//
// attach pinned `KICKOFF_HUB_CONVERSATION=-` and carried a room's identity only in the token path,
// and the documented second-engine overlay (ATTACHING §2) blanks the token path with `-` and never
// touches the ninth variable — so a tool server behind that overlay derived the SEED's door and
// read the SEED's secret, and every word of the room's agent landed in the seed's topic. The one
// variable an overlay must not blank is the one that has to carry the answer.
console.log('\nwhat the child of a room inherits:')
{
  const xdg = join(dir, 'd2-xdg')
  const room = 'c-0d0d0d0d0d0d'
  mkdirSync(join(xdg, 'herdr-tg', 'conversations', room), { recursive: true, mode: 0o700 })
  writeFileSync(join(xdg, 'herdr-tg', 'conversations', room, 'secret'), 'r'.repeat(64), { mode: 0o600 })
  const hubSock = join(dir, 'd2-hub.sock')
  const hub = claimingHub(hubSock)
  const relayDir = join(dir, 'd2-fanin')
  const r = startRun(
    { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: relayDir, KICKOFF_HUB_CONVERSATION: room, XDG_STATE_HOME: xdg },
    ['sh', '-c', 'env | grep ^KICKOFF_HUB_'],
  )
  await r.child.exited
  await Bun.sleep(50)
  const env: Record<string, string> = {}
  for (const l of r.out().split('\n')) {
    const eq = l.indexOf('=')
    if (l.startsWith('KICKOFF_HUB_') && eq > 0) env[l.slice(0, eq)] = l.slice(eq + 1)
  }
  check('the_run_child_of_a_room_inherits_the_conversation_and_no_path_to_a_secret',
    env.KICKOFF_HUB_CONVERSATION === room && env.KICKOFF_HUB_TOKEN_FILE === '-',
    JSON.stringify(env))
  // The §2 overlay on top of the pinned environment, as opencode applies it. The relay directory
  // is kept rather than blanked only so the two doors are comparable: attach was told one for
  // this test, where the unit and the overlay both take the default.
  const { readConfig, secretFor } = await import('../../plugins/kickoff-channel/attach.ts')
  const overlaid = { ...env, KICKOFF_HUB_PROJECT_DIR: '.', KICKOFF_HUB_RELAY: '1', KICKOFF_HUB_ADDRESS: '-',
    KICKOFF_HUB_TOKEN_FILE: '-', KICKOFF_HUB_SOCKET: '-', KICKOFF_HUB_RELAY_SOCKET: '-', KICKOFF_HUB_RELAY_DIR: relayDir,
    KICKOFF_HUB_RELAY_GRACE_MS: '-', CLAUDE_PROJECT_DIR: '', XDG_STATE_HOME: xdg, HOME: process.env.HOME }
  const read = readConfig(overlaid, repo)
  const secret = 'config' in read ? secretFor(read.config) : null
  check('and_behind_the_documented_overlay_a_tool_server_derives_the_rooms_door_and_reads_the_rooms_secret',
    'config' in read && read.config.relaySocket === env.KICKOFF_HUB_RELAY_SOCKET &&
      secret?.token === 'r'.repeat(64) && secret?.conversation === room,
    `door ${'config' in read ? read.config.relaySocket : read.problem.note} vs ${env.KICKOFF_HUB_RELAY_SOCKET}; secret ${JSON.stringify(secret && { how: secret.how, conversation: secret.conversation, token: secret.token.slice(0, 4) })}`)
  hub.stop()
}

// ── E. the stranger, as the engine attach starts, with no git and no door named ───────────────
console.log('\nthe stranger written from the document alone, as the engine:')
{
  // A project dir that is NOT a git checkout, so no door derives — attach must make a private one
  // and hand it to the child. The secret is found without git because it sits right here.
  const noGit = join(dir, 'walled')
  mkdirSync(join(noGit, '.kickoff'), { recursive: true })
  writeFileSync(join(noGit, '.kickoff', 'hub.token'), 'a'.repeat(64), { mode: 0o600 })

  const hubSock = join(dir, 'e-hub.sock')
  const hub = claimingHub(hubSock)
  const child = Bun.spawn(['bun', ATTACH, '--run', 'bun', STRANGER], {
    cwd: noGit,
    env: { ...process.env, TMPDIR: shortTmp,
      KICKOFF_HUB_PROJECT_DIR: noGit, KICKOFF_HUB_SOCKET: hubSock },
    stdout: 'pipe',
    stderr: 'inherit',
  })
  let out = ''
  ;(async () => {
    const dec = new TextDecoder()
    for await (const chunk of child.stdout as any) out += dec.decode(chunk)
  })()
  const code = await child.exited
  await Bun.sleep(100)
  check('a_stranger_written_from_the_document_still_attaches',
    /WELCOME/.test(out) && /ACK reached/.test(out) &&
      hub.got.some(f => f.t === 'say' && f.text === 'attached from the document alone'),
    `stranger said ${JSON.stringify(out.split('\n').filter(Boolean))}`)
  check('and the whole wall cost the hub exactly one claim',
    hub.got.filter(f => f.t === 'hello').length === 1 && hub.refusals.length === 0,
    `${hub.got.filter(f => f.t === 'hello').length} hellos, ${hub.refusals.length} refusals`)
  check('and attach exits with the stranger\'s own clean status', code === 0, `exit ${code}`)
  hub.stop()
}

// ── E2. the stranger as the engine, from a LANE worktree ──────────────────────────────────────
//
// A linked worktree holds no secret of its own — it is gitignored, so it is never checked out into
// one — and attach finds it in the main tree by the search. The stranger does not search (§5 says
// being told is cheaper), and looks only at `<KICKOFF_HUB_PROJECT_DIR>/.kickoff/hub.token`. So the
// pinned `KICKOFF_HUB_TOKEN_FILE` has to be the path attach resolved: the first version pinned `-`
// whenever nothing had TOLD attach the path, attach authenticated fine, and its child refused —
// one claim spent for nothing.
console.log('\nthe stranger as the engine, from a lane worktree:')
{
  const hubSock = join(dir, 'e2-hub.sock')
  const hub = claimingHub(hubSock)
  const child = Bun.spawn(['bun', ATTACH, '--run', 'bun', STRANGER], {
    cwd: laneDir,
    env: { ...process.env, TMPDIR: shortTmp,
      KICKOFF_HUB_PROJECT_DIR: laneDir, KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: join(dir, 'e2-fanin') },
    stdout: 'pipe',
    stderr: 'inherit',
  })
  const out = await new Response(child.stdout).text()
  const code = await child.exited
  check('a_stranger_started_from_a_lane_is_handed_the_secret_attach_found_for_it',
    code === 0 && /WELCOME/.test(out) && hub.got.some(f => f.t === 'say' && f.text === 'attached from the document alone'),
    `attach exit ${code}; stranger said ${JSON.stringify(out.split('\n').filter(Boolean))}`)
  hub.stop()
}

// ── F. a command that cannot be started ───────────────────────────────────────────────────────
console.log('\nwhen the command does not exist:')
{
  const hubSock = join(dir, 'f-hub.sock')
  const hub = claimingHub(hubSock)
  const r = startRun(
    { KICKOFF_HUB_SOCKET: hubSock, KICKOFF_HUB_RELAY_DIR: join(dir, 'f-fanin') },
    ['this-command-does-not-exist-kdj38'],
  )
  const code = await r.child.exited
  check('a_command_that_cannot_be_started_exits_127', code === 127, `exit ${code}`)
  hub.stop()
}

// ── G. PID 1 with a child, and the wall's init in front ───────────────────────────────────────
console.log('\nas PID 1, and behind an init:')
if (Bun.which('bwrap')) {
  // The config is fully named (project, told door), so the ONLY thing that can refuse here is the
  // PID-1 guard — otherwise a "nothing named a project directory" exit 2 would pass this by the
  // wrong door. So the message is asserted, not just the code.
  const g1door = join(shortTmp, 'g1-door.sock')
  const asPid1 = Bun.spawn(
    ['bwrap', '--unshare-pid', '--as-pid-1', '--dev-bind', '/', '/', '--chdir', repo,
      'bun', ATTACH, '--run', 'true'],
    { env: { ...process.env, TMPDIR: shortTmp, KICKOFF_HUB_PROJECT_DIR: repo,
        KICKOFF_HUB_SOCKET: join(dir, 'g-hub.sock'), KICKOFF_HUB_RELAY_SOCKET: g1door },
      stdout: 'ignore', stderr: 'pipe' },
  )
  const err1 = await new Response(asPid1.stderr).text()
  const rc1 = await asPid1.exited
  check('attach refuses --run at PID 1 rather than reaping badly',
    rc1 === 2 && /PID 1/.test(err1), `exit ${rc1}; stderr ${JSON.stringify(err1.trim())}`)

  // Behind bwrap's default reaper (attach is PID 2), the same line runs and the child's clean exit
  // is attach's.
  const g2door = join(shortTmp, 'g2-door.sock')
  const behindInit = Bun.spawn(
    ['bwrap', '--unshare-pid', '--dev-bind', '/', '/', '--chdir', repo,
      'bun', ATTACH, '--run', 'true'],
    { env: { ...process.env, TMPDIR: shortTmp, KICKOFF_HUB_PROJECT_DIR: repo,
        KICKOFF_HUB_SOCKET: join(dir, 'g2-hub.sock'), KICKOFF_HUB_RELAY_SOCKET: g2door },
      stdout: 'ignore', stderr: 'ignore' },
  )
  const rc2 = await behindInit.exited
  check('and behind an init that reaps, --run runs and exits with the child', rc2 === 0, `exit ${rc2}`)

  // ── two walls, one TMPDIR ──────────────────────────────────────────────────────────────────
  // Under `--unshare-pid` attach is PID 2 in EVERY wall, so a private door named by pid was the
  // same path in every wall that shared the host's /tmp — and the second wall died "another attach
  // is already holding it". The folder has to be unique by construction, not by pid.
  console.log('\ntwo bwrap walls sharing one TMPDIR:')
  const noGit = join(dir, 'walled-twice')
  mkdirSync(join(noGit, '.kickoff'), { recursive: true })
  writeFileSync(join(noGit, '.kickoff', 'hub.token'), 'a'.repeat(64), { mode: 0o600 })
  const wallHub = claimingHub(join(dir, 'w-hub.sock'))
  const wall = (address: string) => Bun.spawn(
    ['bwrap', '--unshare-pid', '--dev-bind', '/', '/', '--chdir', noGit,
      'bun', ATTACH, '--run', 'sh', '-c', 'sleep 1.5; exit 0'],
    { env: { ...process.env, TMPDIR: shortTmp, KICKOFF_HUB_PROJECT_DIR: noGit,
        KICKOFF_HUB_SOCKET: join(dir, 'w-hub.sock'), KICKOFF_HUB_ADDRESS: address },
      stdout: 'ignore', stderr: 'pipe' },
  )
  const one = wall('wall-one')
  await Bun.sleep(600)
  const two = wall('wall-two')
  const [errOne, errTwo] = await Promise.all([new Response(one.stderr).text(), new Response(two.stderr).text()])
  const [rcOne, rcTwo] = await Promise.all([one.exited, two.exited])
  const doorOf = (err: string) => err.match(/the door is (\S+)/)?.[1]
  check('two_walls_that_share_a_tmpdir_each_get_a_private_door_of_their_own',
    rcOne === 0 && rcTwo === 0 && doorOf(errOne) !== undefined && doorOf(errOne) !== doorOf(errTwo),
    `exits ${rcOne}/${rcTwo}; doors ${doorOf(errOne)} / ${doorOf(errTwo)}; ${JSON.stringify(errTwo.trim().split('\n').at(-1))}`)
  wallHub.stop()

  // ── what a signal to bwrap does, measured, so the document cannot drift ────────────────────
  // bwrap's init reaps and does nothing else. A SIGTERM to the bwrap process ends bwrap and reaches
  // NOTHING inside: attach keeps running, the door stays bound, the claim stays held — the exact
  // corpse-squatting-the-claim §13 opens with. The wrapper stops a bwrap wall by signalling
  // attach's own host pid, which is the child of the pid `--info-fd` reports (that pid is the init,
  // and it ignores SIGTERM). Pinned here because the first version of the document said the
  // opposite in three places.
  console.log('\na signal to bwrap itself:')
  const g3hub = claimingHub(join(dir, 'g3-hub.sock'))
  const g3info = join(shortTmp, 'g3-info.json')
  const wallOf = (extra: string) => Bun.spawn(
    ['sh', '-c',
      `exec 3>"$1"; exec bwrap --info-fd 3 ${extra} --unshare-pid --dev-bind / / --chdir "$2" bun "$3" --run sh -c 'trap "exit 0" TERM; while :; do sleep 0.2; done'`,
      'sh', g3info, repo, ATTACH],
    { env: { ...process.env, TMPDIR: shortTmp, KICKOFF_HUB_PROJECT_DIR: repo,
        KICKOFF_HUB_SOCKET: join(dir, 'g3-hub.sock'), KICKOFF_HUB_RELAY_SOCKET: join(shortTmp, 'g3-door.sock') },
      stdout: 'ignore', stderr: 'ignore' },
  )
  const alive = (pid: number) => { try { process.kill(pid, 0); return true } catch { return false } }
  const initOf = () => Number(JSON.parse(readFileSync(g3info, 'utf8'))['child-pid'])
  const childOf = (pid: number) => Number(Bun.spawnSync(['pgrep', '-P', String(pid)]).stdout.toString().trim().split('\n')[0])
  {
    const bw = wallOf('')
    await until('the wall to reach the hub', () => g3hub.got.some(f => f.t === 'hello'), 20000)
    const init = initOf()
    const attachPid = childOf(init)
    bw.kill('SIGTERM')
    const rc = await bw.exited
    await Bun.sleep(500)
    check('a_signal_to_bwrap_ends_bwrap_and_reaches_nothing_inside_the_wall',
      rc === 143 && g3hub.live === 1 && !g3hub.got.some(f => f.t === 'bye') && alive(init) && alive(attachPid),
      `bwrap exit ${rc}; hub connections ${g3hub.live}; bye ${g3hub.got.some(f => f.t === 'bye')}; init alive ${alive(init)}; attach alive ${alive(attachPid)}`)
    // The wrapper's way: attach's own host pid, the child of the pid --info-fd reported.
    process.kill(attachPid, 'SIGTERM')
    await until('the bye', () => g3hub.got.some(f => f.t === 'bye'), 15000).catch(() => {})
    await until('the wall to be empty', () => !alive(init), 5000).catch(() => {})
    check('and_signalling_attachs_own_host_pid_stops_the_wall_with_a_bye',
      g3hub.got.some(f => f.t === 'bye') && !alive(attachPid) && !alive(init),
      `bye ${g3hub.got.some(f => f.t === 'bye')}; attach alive ${alive(attachPid)}; init alive ${alive(init)}`)
    if (alive(attachPid)) process.kill(attachPid, 'SIGKILL')
  }
  // With --die-with-parent the wall dies WITH bwrap — killed outright, so no bye; the hub releases
  // the claim when the socket closes. That is the guard against a wall whose wrapper died.
  {
    g3hub.got.length = 0
    const bw = wallOf('--die-with-parent')
    await until('the wall to reach the hub', () => g3hub.got.some(f => f.t === 'hello'), 20000)
    const init = initOf()
    bw.kill('SIGTERM')
    await bw.exited
    await until('the hub to see the socket close', () => g3hub.live === 0, 5000).catch(() => {})
    check('with_die_with_parent_a_signal_to_bwrap_kills_the_wall_and_the_claim_is_released_without_a_bye',
      g3hub.live === 0 && !g3hub.got.some(f => f.t === 'bye') && !alive(init),
      `hub connections ${g3hub.live}; bye ${g3hub.got.some(f => f.t === 'bye')}; init alive ${alive(init)}`)
    if (alive(init)) { const k = childOf(init); if (k) process.kill(k, 'SIGKILL') }
  }
  g3hub.stop()
} else {
  console.log('  (skipped: bwrap is not on this box)')
}

// ── H. the engine's own tool server, under the operator's config, unchanged ──────────────────
//
// The property in this name needs the real engine: `opencode serve` under `--run`, with the
// operator's own `mcp.kickoff-channel` entry — its `environment` block copied from his file at test
// time, read-only, or none when the box has no such file — and a session opened so the engine
// spawns its MCP child. What is asserted is what only that can show: the child attached at
// attach's door, opencode reports it connected, its environment (read from /proc) carries the
// pinned door with his block overlaid on top, and a tool server run with EXACTLY that environment
// and cwd says something that the fake hub receives. The earlier check with this name ran
// `sh -c env` and would have stayed green if opencode had dropped the parent environment.
console.log('\nthe engine\'s own tool server, under the operator\'s config:')
if (Bun.which('opencode')) {
  const hisFile = join(process.env.HOME ?? '', '.config', 'opencode', 'opencode.json')
  let environment: Record<string, string> | null = null
  try {
    environment = JSON.parse(readFileSync(hisFile, 'utf8')).mcp?.['kickoff-channel']?.environment ?? null
    console.log(`  (his environment block, copied: ${JSON.stringify(environment)})`)
  } catch {
    console.log('  (no ~/.config/opencode/opencode.json on this box; the entry gets no environment block)')
  }
  // A home of its own, so nothing of his is read or written beyond the block copied above.
  const home = join(dir, 'oc-home')
  for (const d of ['config/opencode', 'data', 'cache', 'state']) mkdirSync(join(home, d), { recursive: true })
  writeFileSync(join(home, 'config/opencode/opencode.json'), JSON.stringify({
    mcp: { 'kickoff-channel': { type: 'local', command: ['bun', SERVER], ...(environment ? { environment } : {}), enabled: true } },
  }))
  const probe = Bun.listen({ hostname: '127.0.0.1', port: 0, socket: { data() {} } })
  const port = probe.port
  probe.stop(true)
  const hubSock = join(dir, 'h-hub.sock')
  const hub = claimingHub(hubSock)
  const said: string[] = []
  const attach = Bun.spawn(
    ['bun', ATTACH, '--opencode', `http://127.0.0.1:${port}`, '--run', 'opencode', 'serve', '--port', String(port), '--hostname', '127.0.0.1'],
    { cwd: repo,
      env: { ...process.env, TMPDIR: shortTmp, KICKOFF_HUB_PROJECT_DIR: repo, KICKOFF_HUB_SOCKET: hubSock,
        KICKOFF_HUB_RELAY_DIR: join(dir, 'h-fanin'),
        XDG_CONFIG_HOME: join(home, 'config'), XDG_DATA_HOME: join(home, 'data'),
        XDG_CACHE_HOME: join(home, 'cache'), XDG_STATE_HOME: join(home, 'state') },
      stdout: 'ignore', stderr: 'pipe' },
  )
  ;(async () => {
    const dec = new TextDecoder()
    for await (const chunk of attach.stderr as any) for (const l of dec.decode(chunk).split('\n')) if (l.trim()) said.push(l)
  })()
  let up = false
  for (let i = 0; i < 120 && !up; i++) {
    try { up = (await fetch(`http://127.0.0.1:${port}/session`)).ok } catch { /* not yet */ }
    if (!up) await Bun.sleep(250)
  }
  if (up) await fetch(`http://127.0.0.1:${port}/session`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' })
  await until('the engine\'s tool server to attach', () => said.some(l => /a producer attached \(2 now\)/.test(l)), 30000).catch(() => {})
  let mcp: any = null
  try { mcp = await (await fetch(`http://127.0.0.1:${port}/mcp`)).json() } catch { /* asserted below */ }
  // The MCP child is a descendant of attach: attach → opencode → bun server.ts.
  const descendants = (pid: number): number[] => {
    const kids = Bun.spawnSync(['pgrep', '-P', String(pid)]).stdout.toString().trim().split('\n').filter(Boolean).map(Number)
    return kids.flatMap(k => [k, ...descendants(k)])
  }
  const toolServer = descendants(attach.pid).find(pid => {
    try { return readFileSync(`/proc/${pid}/cmdline`, 'utf8').includes('kickoff-channel/server.ts') } catch { return false }
  })
  let childEnv: Record<string, string> = {}
  let childCwd = ''
  if (toolServer) {
    for (const kv of readFileSync(`/proc/${toolServer}/environ`, 'utf8').split('\0')) {
      const eq = kv.indexOf('=')
      if (eq > 0 && (kv.startsWith('KICKOFF_HUB_') || kv.startsWith('CLAUDE_PROJECT_DIR'))) childEnv[kv.slice(0, eq)] = kv.slice(eq + 1)
    }
    childCwd = readlinkSync(`/proc/${toolServer}/cwd`)
  }
  const door = said.find(l => /the door is /.test(l))?.match(/the door is (\S+)/)?.[1]
  check('the_engines_own_tool_server_attaches_to_it_with_the_operators_config_unchanged',
    up && said.some(l => /a producer attached \(2 now\)/.test(l)) && mcp?.['kickoff-channel']?.status === 'connected' &&
      toolServer !== undefined && childEnv.KICKOFF_HUB_RELAY === '1' && childEnv.KICKOFF_HUB_RELAY_SOCKET === door,
    `up ${up}; attach said ${JSON.stringify(said.filter(l => /producer|door/.test(l)))}; /mcp ${JSON.stringify(mcp)}; tool server ${toolServer}; its env ${JSON.stringify(childEnv)}`)
  // And that environment, in that cwd, is a voice the hub hears — proven with a second tool server
  // driven through the MCP handshake, since the engine's own only speaks when a model calls a tool.
  if (toolServer) {
    const voice = startServer(childEnv, childCwd)
    await handshake(voice, OPENCODE.capabilities, OPENCODE.clientInfo)
    const r = await call(voice, 'reply', { text: 'through his config, under --run' })
    check('and a tool server given exactly that environment is heard at the hub',
      r.text.startsWith('said') && hub.got.some(f => f.t === 'say' && f.text === 'through his config, under --run'),
      `${r.text}; hub saw ${JSON.stringify(hub.got.map(f => f.t))}`)
    voice.child.kill()
  }
  attach.kill('SIGTERM')
  const gone = await Promise.race([attach.exited, Bun.sleep(15000).then(() => null)])
  if (gone === null) attach.kill('SIGKILL')
  hub.stop()
} else {
  console.log('  (skipped: opencode is not on this box)')
}

rmSync(dir, { recursive: true, force: true })
console.log(failures === 0 ? '\nall checks passed' : `\n${failures} FAILED`)
process.exit(failures === 0 ? 0 : 1)
