/**
 * `--run` — start the engine as attach's child, so a wall has one entrypoint.
 *
 * This is the ONE place in the whole command that spawns anything, and the comment at the spawn
 * site says why it may: **the ADAPTER may spawn; the HUB never does.** The hub's closed list of
 * capabilities (`docs/INTERFACES.md`) puts "spawning, supervising, or killing anything" under things
 * the hub is not, and its argument is that no string from the wire may ever reach a command line.
 * Nothing here contradicts that: the command attach runs comes from its OWN argv — typed by a person
 * or written by a wrapper — and no frame from the hub can add to it, change it or start it. A
 * `choice` reaches an opencode reply endpoint or a tool server's turn, never this file. The hub
 * still has zero `Command` in its binary, and the test that pins the deletion of the keystroke path
 * does not know `adapters/` exists.
 *
 * # Signals and status, measured on this box
 *
 * A process with no handler for SIGTERM at PID 1 IGNORES it — the kernel applies no default action —
 * so a wall that cannot be stopped is a wall that gets killed with its questions still open. attach
 * therefore installs handlers and forwards the signal to the child, waits, then SIGKILLs. It does
 * NOT reap orphans: a grandchild the engine leaves reparents to PID 1 and, if PID 1 is a JavaScript
 * runtime, stays a zombie for ever — also measured. Reaping is one job an init does in a kilobyte of
 * C, so `main.ts` refuses `--run` at PID 1 and names the fix rather than doing it badly here.
 */

import { constants } from 'os'

/** What attach should exit with once the child is gone. */
export type ChildExit = {
  /** The status to propagate: the child's own code, or `128 + signal`, or 127 if it never started. */
  code: number
}

export type RunOptions = {
  /** Everything after `--run` — the command and its arguments, verbatim. */
  command: string[]
  /** The child's working directory: the project directory attach speaks for. */
  cwd: string
  /**
   * The child's environment: attach's own, plus the eight namespace variables pinned so that any
   * adapter descending from the engine finds the door with nothing else configured (§13.3).
   */
  env: Record<string, string | undefined>
  /** Say something in this process's own transcript, prefixed as attach's own line. */
  note: (msg: string) => void
  /**
   * Run just before attach exits, after the child is gone: write the ledger, say `bye` on the hub
   * link, unlink the door. `main.ts` hands `relay.goodbye` here.
   */
  onBeforeExit: () => void
}

/**
 * How long the child gets to honour a forwarded signal before it is killed outright.
 *
 * Ten seconds, matching what `docs/ATTACHING.md` §13.5 promises and what the unit's `TimeoutStopSec`
 * leaves room for. Overridable ONLY so a test can watch the kill happen without waiting it out; a
 * value that is not a positive whole number is ignored rather than coerced.
 */
const KILL_AFTER_MS = (() => {
  const v = process.env.KICKOFF_HUB_ATTACH_STOP_MS
  return v && /^[0-9]+$/.test(v) && Number(v) > 0 ? Number(v) : 10_000
})()

/** How long to let the kernel take the `bye` before exiting — `process.exit` on the same tick loses it. */
const BYE_GRACE_MS = 200

/**
 * Spawn the child, wire the signals, and never return: attach's life is now the child's life.
 *
 * When the child exits — on its own, or because a signal we forwarded reached it — attach says its
 * own goodbye and exits with the child's status, so `docker stop` and systemd read the right code.
 */
export function runChild(opts: RunOptions): void {
  const { command, note } = opts

  // Declared BEFORE the spawn, because a spawn that throws (a command not on PATH) calls `finish`
  // from inside its own catch — and a `let` referenced before its declaration line has run is a
  // temporal-dead-zone throw, which is how "exit 127" turned into an uncaught ReferenceError once.
  let ending = false
  /** Say attach's own goodbye, wait for the kernel to take the `bye`, then exit with the status. */
  function finish(exit: ChildExit): void {
    if (ending) return
    ending = true
    opts.onBeforeExit()
    setTimeout(() => process.exit(exit.code), BYE_GRACE_MS)
  }

  let child: import('bun').Subprocess
  try {
    // THE SPAWN. The ADAPTER may spawn; the HUB never does — see the file header. The command is
    // attach's own argv, and no wire frame can reach it.
    child = Bun.spawn(command, {
      cwd: opts.cwd,
      env: opts.env,
      // The child inherits attach's own stdio: its output is the operator's to read in the journal,
      // and attach never reads stdin itself — it is not an MCP server and nothing on that stream is
      // for it.
      stdin: 'inherit',
      stdout: 'inherit',
      stderr: 'inherit',
    })
  } catch (e) {
    // A command that cannot be started at all — not on PATH, not executable. `127` is what a shell
    // returns for it, and what a wall's supervisor expects.
    note(`could not start ${command[0]}: ${(e as Error)?.message ?? e}`)
    finish({ code: 127 })
    return
  }

  let killTimer: ReturnType<typeof setTimeout> | null = null
  function onSignal(sig: NodeJS.Signals): void {
    note(`got ${sig}; passing it to the engine and waiting up to ${KILL_AFTER_MS / 1000}s`)
    try {
      child.kill(sig === 'SIGINT' ? 'SIGINT' : 'SIGTERM')
    } catch {
      /* already gone; the exit handler below will fire */
    }
    // A wall that cannot be stopped is worse than one that stops hard. If the child ignores the
    // signal, SIGKILL it and proceed as if it had exited on its own.
    if (!killTimer) {
      killTimer = setTimeout(() => {
        note('the engine did not stop in time; killing it')
        try {
          child.kill('SIGKILL')
        } catch {
          /* already gone */
        }
      }, KILL_AFTER_MS)
    }
  }
  for (const sig of ['SIGTERM', 'SIGINT'] as const) process.on(sig, () => onSignal(sig))

  void child.exited.then(() => {
    if (killTimer) clearTimeout(killTimer)
    // Bun reports a signal death with `signalCode` as the signal's NAME (e.g. "SIGTERM"), and leaves
    // `exitCode` null — NOT a number, which an earlier version assumed and so reported every
    // signalled child as exit 0. Map the name to its number and propagate `128 + n`, which is what
    // `docker stop` and systemd read; otherwise the child's own exit code.
    const signalName = (child as any).signalCode as string | null | undefined
    const signalNum = signalName ? (constants.signals as Record<string, number>)[signalName] : undefined
    const code = signalNum ? 128 + signalNum : (child.exitCode ?? 0)
    note(`the engine exited (${signalName ? `signal ${signalName}` : `status ${child.exitCode ?? 0}`})`)
    finish({ code })
  })
}
