/**
 * What the plugin manifest starts — the install decision, then `server.ts`.
 *
 * `.mcp.json` used to run `bun install && bun server.ts` through bun's shell. Two ways that failed
 * before the server ever ran. In a wall the plugin is mounted read-only, and `bun install` there
 * dies re-linking `node_modules/.bin` (EEXIST), so the engine reported the channel as failed and
 * the agent had no way to reach the phone. Deciding with `test -d node_modules/…` in the shell line
 * fixed that box and not a wall: bun's shell has no `test` of its own, a minimal image has none
 * on `PATH`, and "command not found" fell through to the same install.
 *
 * So the decision is made here, with nothing but bun: install when the directory can be written,
 * which is every developer's checkout — that is what brings `node_modules` up to date after a
 * pull, and "skip it when the directory is present" would start the server against whatever the
 * pull left behind, saying nothing. Skip it only where it could not succeed anyway. A failed
 * install stops the start, as it always did: a server that cannot load its SDK is not a server.
 *
 * The install's stdout goes to stderr on purpose. This process's stdout is the MCP pipe, and one
 * line of the installer's on it is a frame the engine cannot parse.
 */
import { accessSync, constants } from 'node:fs'

const here = import.meta.dir
let writable = true
try {
  accessSync(here, constants.W_OK)
} catch {
  writable = false
}
if (writable) {
  const install = Bun.spawnSync([process.execPath, 'install', '--no-summary'], {
    cwd: here,
    stdin: 'ignore',
    stdout: 'pipe',
    stderr: 'pipe',
  })
  process.stderr.write(install.stdout)
  process.stderr.write(install.stderr)
  if (install.exitCode !== 0) {
    process.stderr.write('kickoff-channel: bun install failed, so the server was not started\n')
    process.exit(install.exitCode || 1)
  }
}

await import('./server.ts')
