/**
 * A fake opencode server, in a file of its own so a PROCESS can start one.
 *
 * It moved out of `test-harness.ts` for one reason: that file points `XDG_STATE_HOME` at a
 * directory of its own the moment it is imported, and a fixture started as somebody else's child
 * must take its state home from its parent — importing the harness to reach this would silently
 * retarget the channel's home of whatever started it. `test-harness.ts` re-exports both of these,
 * so every suite keeps its import and there is still exactly one of each: the last time this
 * project kept two fakes of one server they drifted, and a suite that agrees only with its own
 * fake proves nothing about the real one.
 */

/**
 * One session as opencode 1.18.25 lists it, captured rather than invented.
 *
 * Measured on this box on 6 September against two running servers: `agent` is a plain string on a
 * session and is ABSENT on one created before agents were named, `parentID` is absent on a root and
 * present on a subagent's, and `directory` is the session's own. A fixture invented here would only
 * prove the suite agrees with itself, which is how the event payload came to be read out of the
 * wrong field once.
 */
export function aSession(id: string, directory: string, updated: number, extra: Record<string, unknown> = {}) {
  return {
    id,
    slug: 'jolly-wizard',
    projectID: '0000000000000000000000000000000000000000',
    directory,
    path: '',
    cost: 0,
    tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
    title: 'New session - 2026-09-05T11:26:25.115Z',
    version: '1.18.25',
    time: { created: 1788607585115, updated },
    ...extra,
  }
}

/**
 * A fake opencode server: the event stream, the session listing and the two POSTs the watcher uses.
 *
 * Shared rather than copied, because the last time this project kept two fakes of one server they
 * drifted, and a suite that agrees only with its own fake proves nothing about the real one. The
 * listing answers the two rules measured against 1.18.25: `directory=` is an exact match on the
 * session's own directory (a trailing slash resolves to it, a subfolder does not), `roots=true`
 * drops every session carrying a `parentID`, and the answer is most recently updated first.
 */
export function fakeOpencode() {
  let sessions: Record<string, any>[] = []
  const posted: { path: string; body: any }[] = []
  const sessionQueries: string[] = []
  let promptStatus = 204
  // Every subscriber of the event stream, not the last one to arrive.
  //
  // It was a single variable, overwritten by each new `/event` request, and one watcher is the only
  // shape any suite had ever put on it. A fleet trial runs several walls against ONE server: with
  // the single variable, three of four watchers were subscribed, quiet, and looked healthy while
  // every question went to the fourth — which passes a test that only ever asserts a question did
  // not arrive.
  //
  // What was MEASURED against the real 1.18.25 on this box, and what was not: two concurrent
  // `GET /event` subscribers both connected and each received its own `server.connected`, so the
  // single-subscriber shape was wrong and holding several is right. That one published event
  // reaches all of them was NOT observed — the server was idle — so it is this fixture's
  // assumption, of a piece with the version caveat at the top of the fleet trial.
  const subscribers = new Set<(e: unknown) => void>()
  const push = (e: unknown) => { for (const s of subscribers) s(e) }
  /** The messages this server holds, by id — see the message endpoint below. */
  const messages = new Map<string, Record<string, unknown>>()
  // A listing that takes its time, once. The window between reading the note and hearing the
  // server back is where a launcher's rollover lands in the wrong session if nothing re-reads.
  let delayNextListingMs = 0
  // The same, for the one request that says which agent a turn ran under. A server too busy to
  // answer it is the case where the fence has nothing to decide on, and a fence that lets a
  // question through because the server was slow is a fence with a hole in it.
  let delayNextMessageMs = 0
  // The same again, but for EVERY message lookup rather than one. A server that is merely slow —
  // well inside the deadline, and slower than the gap between one re-offer of a kept question and
  // the next — is the state where two of those re-offers were in flight at once.
  let messageDelayMs = 0
  // What the server answers a turn-down with. A refusal it will not take leaves the worker exactly
  // where withholding alone would have left it, and this is how a test can tell whether the
  // sentence in the topic is describing the world or hoping.
  let refusalStatus = 200
  // A server that answers `directory=` with sessions from elsewhere. It is not how 1.18.25 behaves
  // and that is the point: the acceptance must not rest on the query having been obeyed.
  let ignoreDirectoryFilter = false
  // The agent names this server can resolve. `null` is a server that will not say — an older one
  // with no such route — and it is the DEFAULT, because that is what every suite written before the
  // question was asked stood on, and nothing may be refused on an answer nobody gave.
  let agents: string[] | null = null
  // How many times the watcher has ASKED which agents this server knows. A test that changes the
  // answer proves nothing unless the question was put again — the set is remembered for the run,
  // so a check standing on a stale one passes whatever the server now says.
  let agentQueries = 0

  const server = Bun.serve({
    port: 0,
    hostname: '127.0.0.1',
    // Bun closes an idle request after ten seconds by default, which would end a deliberately hung
    // request from the server's side; the longest Bun allows.
    idleTimeout: 255,
    async fetch(req) {
      const url = new URL(req.url)
      if (url.pathname === '/event') {
        // Held out here so `cancel` can take the same one off again. A watcher whose process is
        // gone leaves a closed stream behind, and enqueueing into one throws — which on a server
        // several walls share would take the push to every LIVING watcher down with it.
        let mine: ((e: unknown) => void) | null = null
        return new Response(
          new ReadableStream({
            start(c) {
              const enc = new TextEncoder()
              mine = e => {
                try {
                  c.enqueue(enc.encode(`data: ${JSON.stringify(e)}\n\n`))
                } catch {
                  if (mine) subscribers.delete(mine)
                }
              }
              subscribers.add(mine)
            },
            cancel() {
              if (mine) subscribers.delete(mine)
            },
          }),
          { headers: { 'content-type': 'text/event-stream' } },
        )
      }
      // `GET /agent`, measured on 1.18.25 on 7 September: 200 and a JSON array of
      // `{name, description, mode, native, permission, options}` — the whole server's set, not one
      // session's. Only `name` is read here; the rest is carried so the shape is the real one.
      if (url.pathname === '/agent' && req.method === 'GET') {
        agentQueries++
        if (agents === null) return new Response('not found', { status: 404 })
        return Response.json(agents.map(name => ({ name, description: '', mode: 'primary', native: true })))
      }
      if (url.pathname === '/session' && req.method === 'GET') {
        sessionQueries.push(url.search)
        if (delayNextListingMs > 0) {
          const wait = delayNextListingMs
          delayNextListingMs = 0
          await new Promise(r => setTimeout(r, wait))
        }
        const dir = (url.searchParams.get('directory') ?? '').replace(/\/+$/, '')
        return Response.json(
          sessions
            .filter(s => ignoreDirectoryFilter || !dir || s.directory === dir)
            .filter(s => url.searchParams.get('roots') !== 'true' || !s.parentID)
            .sort((a, b) => b.time.updated - a.time.updated),
        )
      }
      // `GET /session/{id}/message/{id}` answers `{info, parts}`, and `info.agent` is the agent the
      // TURN ran under. Captured from the 1.18.25 OpenAPI at `/doc`: `AssistantMessage` carries
      // `agent` and the asked events do not, which is why the agent of a turn has to be asked for.
      const asMessage = /^\/session\/([^/]+)\/message\/([^/]+)$/.exec(url.pathname)
      if (asMessage && req.method === 'GET') {
        if (delayNextMessageMs > 0) {
          const wait = delayNextMessageMs
          delayNextMessageMs = 0
          await new Promise(r => setTimeout(r, wait))
        } else if (messageDelayMs > 0) {
          await new Promise(r => setTimeout(r, messageDelayMs))
        }
        const info = messages.get(asMessage[2])
        if (!info) return new Response('not found', { status: 404 })
        return Response.json({ info, parts: [] })
      }
      if (req.method === 'POST') {
        // Not every POST on this wire carries a body: `question/{id}/reject` takes none at all
        // (measured off 1.18.25's own `/doc`), and a fake that insisted on JSON answered the one
        // request that unblocks a stranded worker with a 500.
        let body: unknown = null
        try {
          body = await req.json()
        } catch {
          body = null
        }
        posted.push({ path: url.pathname, body })
        if (refusalStatus !== 200 && (url.pathname.endsWith('/reject') || (body as any)?.reply === 'reject')) {
          return new Response('no', { status: refusalStatus })
        }
        if (url.pathname.endsWith('/prompt_async')) {
          if (promptStatus === 204) return new Response(null, { status: 204 })
          return Response.json({ name: 'NotFoundError', data: { message: 'Session not found' } }, { status: promptStatus })
        }
        return new Response('{}', { headers: { 'content-type': 'application/json' } })
      }
      return new Response('not found', { status: 404 })
    },
  })

  return {
    posted,
    sessionQueries,
    /** What agent a turn ran under: `messages.set(<messageID>, {id, role: 'assistant', agent})`. */
    messages,
    get url() { return `http://127.0.0.1:${server.port}` },
    get sessions() { return sessions },
    set sessions(v: Record<string, any>[]) { sessions = v },
    set promptStatus(v: number) { promptStatus = v },
    /** Make the next session listing take this long before it answers. One shot. */
    set delayNextListingMs(v: number) { delayNextListingMs = v },
    /** Make the next message lookup take this long before it answers. One shot. */
    set delayNextMessageMs(v: number) { delayNextMessageMs = v },
    /** Make EVERY message lookup take this long. A server that is slow rather than broken. */
    set messageDelayMs(v: number) { messageDelayMs = v },
    /** What every turn-down is answered with. 200 is a server that takes it. */
    set refusalStatus(v: number) { refusalStatus = v },
    /** Answer every listing with every session, whatever `directory=` asked for. */
    set ignoreDirectoryFilter(v: boolean) { ignoreDirectoryFilter = v },
    /** The agent names this server resolves; `null` answers 404, as a server without the route. */
    set agents(v: string[] | null) { agents = v },
    /** How many `GET /agent` requests have been made, so a test can see the question was put. */
    get agentQueries() { return agentQueries },
    /** Whether anything is listening yet; false until a watcher has subscribed. */
    get pushing() { return subscribers.size > 0 },
    /** How many watchers are subscribed — the number a fleet rig waits to reach. */
    get watchers() { return subscribers.size },
    /** Push one event to every subscriber. */
    push,
    prompts: () => posted.filter(p => p.path.endsWith('/prompt_async')),
    stop: () => server.stop(true),
  }
}
