/**
 * One connection to the hub, and the queue behind it.
 *
 * This is the wire — NDJSON over `AF_UNIX`, the frames in `crates/hub-proto` — with none of the
 * policy about what the frames mean. It was lifted out of `server.ts` unchanged, because the same
 * arithmetic is now needed by two processes and a second copy is how the first one rotted: the
 * opencode bridge forked an early version of it and drifted by thirteen invariants, every one of
 * them a defect that had already been found and fixed once on the other side.
 *
 * # The three framing rules, and why they are worth the lines
 *
 * Each cost a real debugging session on the Rust side of this same socket:
 *
 *   1. The trailing newline is appended in ONE place and nowhere else. Omitting it made the far end
 *      hang forever with no error and no close — 5.01s, zero bytes, connection still open.
 *   2. Read exactly one line. Never to EOF: a reset after a good frame loses the frame with it.
 *   3. Bound one frame, not the conversation. A cumulative ceiling ends a healthy stream in silence
 *      that reads as a disconnect the peer never performed.
 *
 * # What is policy and lives with the owner
 *
 * What a `welcome` has to contain before it counts, what each `refused` reason means to whoever is
 * listening, and what an `ack` should be turned into. This module answers exactly one frame on its
 * own — `ping`, with a `pong` — because liveness must not depend on anybody upstairs being awake.
 */

export const PROTOCOL_VERSION = 1
export const MAX_FRAME_BYTES = 64 * 1024

/**
 * What became of one frame, and the only thing a caller is allowed to report from.
 *
 * `send` used to return nothing, so "written to a live socket", "parked in a queue for a link that
 * has never once come up" and "refused for being too big" were indistinguishable to the caller —
 * and every one of the three was reported to the agent as success. The agent then told the operator
 * his phone had buzzed while the bridge had no secret and had connected to nothing.
 */
export type Delivery = ({ delivered: true } | { delivered: false; permanent: boolean; why: string }) & {
  /**
   * The envelope id the hub will name in this frame's `ack`.
   *
   * Returned rather than read back off the counter afterwards, because a relay has to map the
   * hub's ack onto the frame a PRODUCER sent, and a correlation that depends on nothing else
   * having minted an id in between is a correlation that breaks the first time something does.
   */
  id: string
}

/**
 * Whether the operator is reachable, and when he is not, whether waiting will mend it.
 *
 * `permanent` means nothing changes until a person changes something — no secret on disk, or a hub
 * that will not have this project. It is carried this far because it is the difference between an
 * agent that waits and an agent that gives up, and only the bridge knows which is right.
 */
export type LinkState = { up: true } | { up: false; permanent: boolean; why: string }

/** One frame on its way out, and the plain words for what is lost if it never goes. */
export type Outbound = {
  id: string
  bytes: Uint8Array
  what: string
  askId?: string
  control?: true
  /**
   * Who this frame is for, when one link carries several callers.
   *
   * A relay hands frames from more than one producer to one queue, and the queue is a shared,
   * bounded resource: without a way to ask how much of it one caller is holding, the first caller
   * to speak fills it and every later caller's question is refused for a backlog that is not its
   * own.
   */
  owner?: string
}

/** A frame the kernel has taken only the first `sent` bytes of. */
type Started = Outbound & { sent: number }

/**
 * A frame the kernel took whole that nobody will ever answer for, and what it was.
 *
 * Not an `Outbound`: its bytes are gone and must stay gone. Re-sending it is the one thing a caller
 * must not do with it — if it DID reach the hub, a second copy is a second message on his phone,
 * and for a question a second live menu.
 */
export type Unanswered = { id: string; what: string; askId?: string }

/** Where to dial and what to say on arrival — or why neither can be worked out yet. */
export type Identity =
  | { socket: string; hello: Record<string, unknown> }
  | { refuse: { permanent: boolean; why: string; note: string; retryMs?: number } }

/**
 * How one dial ended, for a caller that has to REPORT reachability rather than keep trying.
 *
 * The errno is the fact `--check` turns into a sentence: `ENOENT` is no socket file (the hub is not
 * running, or its directory is not mounted), `ECONNREFUSED` is a file with nothing behind it,
 * `EACCES` is the wrong user against a 0600 socket. `closedBeforeWelcome` is the silent close —
 * a uid mismatch and a malformed `hello` are indistinguishable from outside, as `docs/ATTACHING.md`
 * §10 says. The module used to discard the errno entirely (the defect `docs/TAXONOMY.md` §8 named).
 */
export type DialEnd = { code: string } | { closedBeforeWelcome: true }

export type HubLinkOptions = {
  /**
   * Resolved AFRESH on every attempt, never once: the operator may run `kickoff-channel enroll` while the
   * session is running, and that is the recovery a tool result tells him to perform.
   */
  identify: () => Identity
  /** Every hub frame except `ping`, which this module answers by itself. */
  onFrame: (frame: Record<string, any>) => void
  /**
   * Frames released because the link went down for good. Each was reported to its caller as
   * waiting in line and certain to go out; leaving them to rot in silence is the original defect
   * wearing a different coat.
   */
  onLost?: (lost: Outbound[], why: string) => void
  /**
   * Frames the kernel took whole on a connection that then ended before the hub answered for them.
   *
   * An `ack` names a frame by an id minted for ONE connection, and the next connection's hub has
   * never seen it — so at the moment a connection closes, every flushed frame without an ack is a
   * frame no ack is ever coming for. They used to stay on the books in silence: the agent had been
   * told "said", the hub had destroyed them (or delivered them and lost the ack with the socket),
   * and no correction ever came. Measured against the real hub: fourteen frames flushed whole into
   * two refused connections, zero corrections to the agent, nothing on the phone.
   *
   * Their fate is UNKNOWN, and that is what a caller must say — not "lost" (`onLost` is for frames
   * that never went) and never "reached him". Nothing here re-sends them: if one did land, a second
   * copy is a duplicate on his phone, and for a question a second live menu.
   */
  onUnanswered?: (frames: Unanswered[], why: string) => void
  /** Say something in this process's own transcript. The operator cannot see it; a developer can. */
  note: (msg: string) => void
  /**
   * The link came up, or went down — for a caller that is standing in for the hub to somebody else.
   *
   * A relay's producers have their own link to the RELAY, and it is up whenever the relay is
   * running. Without this they were told he had been reached while there was nothing behind the
   * relay at all: the tool said "said" and the agent told the operator his phone had buzzed. Called
   * on every transition and on repeats of one, so a handler must be safe to run twice.
   */
  onState?: (up: boolean) => void
  /** What the agent is told when nothing is listening on the socket at all. */
  whenUnreachable: string
  /**
   * What the agent is told when a link that WAS up has dropped.
   *
   * Overridable because behind a relay the local socket always answers, so `whenUnreachable` can
   * never be reached and this sentence is the only one a hub outage ever produces.
   */
  whenDropped?: string
  /** Letter the frame ids on this connection start with. Opaque to the hub; useful in a log. */
  framePrefix?: string
  maxPending?: number
  /**
   * Dial ONCE and never redial, and do not answer the hub's `ping`.
   *
   * For a caller that is PROVING reachability rather than holding a claim — `--check`. It must not
   * redial (a check is one round trip, not a supervised link) and it must not `pong`: the hub
   * creates the topic only AFTER a pong, so a check that answered the ping would greet a topic on
   * the operator's phone, which is the one thing a check must never do. `welcome` arriving is proof
   * enough; the check sends `bye` and closes without ponging.
   */
  once?: boolean
  /**
   * How a dial ended, when it did not reach `welcome`. Called at most once per dial, so a caller
   * that only ever dials once (see `once`) hears exactly what to report.
   */
  onDial?: (end: DialEnd) => void
}

export class HubLink {
  private readonly o: HubLinkOptions
  private readonly maxPending: number
  private sock: import('bun').Socket | null = null

  state: LinkState = { up: false, permanent: false, why: 'The bridge has not reached the hub yet.' }

  /** Frames written before the link was up. Bounded: a queue that grows is a leak with a plan. */
  private pending: Outbound[] = []

  /**
   * Frames this connection owes the hub regardless of whether it has been admitted — the `hello`
   * that asks to be, the `pong` that keeps it, the `bye` that ends it.
   *
   * Separate from `pending` because the two are held back for opposite reasons: nothing an agent
   * sent may go out before `welcome`, and these three are what makes `welcome` happen at all. They
   * also die with the connection, where `pending` outlives it.
   */
  private control: Outbound[] = []

  /**
   * The tail of a frame the kernel took only part of, held so it can be finished.
   *
   * Bun's `Socket.write` is not Node's: it returns how many bytes the kernel accepted and DROPS the
   * rest — it buffers nothing of its own. Throwing that number away cost whole messages and, worse,
   * left a headless prefix on the wire that swallowed the NEXT healthy frame into one line the hub
   * could not read. Measured against a peer that had stopped reading: 131 MB offered, 245 KB
   * accepted, everything else gone, and every one of those frames reported to the agent as said.
   */
  private unsent: Started | null = null

  /**
   * What each frame the hub has not answered for was, so an `ack` saying he never got it can name
   * it. Bounded, because a hub that stopped acking would otherwise turn this into a slow leak.
   */
  private readonly inFlight = new Map<string, { what: string; askId?: string }>()

  private backoff = 1000
  private saidItWasDown = false
  private seq = 0

  /**
   * The lease this run holds for its address, granted on the `welcome`'s own envelope.
   *
   * `undefined` until a hub grants one, and again the moment a hub says it grants none — an older
   * hub fences nothing, and stamping it a number it never minted would be a claim about a thing it
   * has no opinion on. A zero is not a lease either: the hub reads one as "this peer holds none",
   * so a bridge that wrote a zero would look as though it held a number and lose every comparison
   * that number was ever in — turned away, for ever, on every restart.
   *
   * It outlives the connection it came from on purpose. A run that lost its socket comes back
   * saying which lease it is holding, and the hub uses it to tell a run that is merely redialling
   * from one a later run has already replaced.
   */
  private lease: number | undefined

  /**
   * Has this run been told there is no way back for it? Then it never dials again.
   *
   * Separate from `state.permanent`, which nearly always means "until a person mends something"
   * — no secret on disk, a project not enrolled — and which the dial loop keeps retrying behind
   * on purpose, because the recovery a tool result prescribes happens while this session runs.
   */
  private givenUp = false

  /** Has this link ever been welcomed? Tells a close-before-welcome from a live link dropping. */
  private reachedWelcome = false

  /**
   * Did THIS dial hear a `refused` before it closed? The hub closes right after refusing, and that
   * close is not the silent one: reporting it as such made `--check` print, after the refusal it
   * had already named, a second line blaming the uid — a fix that was not the fix, with the count
   * one too many. Reset on every dial, so a later dial's genuinely silent close is still reported.
   */
  private sawRefusal = false

  constructor(o: HubLinkOptions) {
    this.o = o
    this.maxPending = o.maxPending ?? 64
  }

  /**
   * One counter for every id this connection mints.
   *
   * Shared with the caller on purpose: an ask id and a frame id come off the same sequence, which
   * is what the reviewed version did, and splitting them would renumber every question an agent
   * asks for no reason at all.
   */
  nextSeq(): number {
    return ++this.seq
  }

  private nextId(): string {
    return `${this.o.framePrefix ?? 'b'}${this.nextSeq()}`
  }

  get isUp(): boolean {
    return this.state.up
  }

  /**
   * How many frames the queue holds in total.
   *
   * Read by a caller that has to divide one queue between several producers: a share worked out
   * from a constant it guessed would drift the moment the queue was resized, and a share too big
   * for the queue is no share at all.
   */
  get capacity(): number {
    return this.maxPending
  }

  /** What the hub owes an answer for, for a caller that has to name a lost frame. */
  frameInFlight(id: string): { what: string; askId?: string } | undefined {
    return this.inFlight.get(id)
  }

  forgetInFlight(id: string): void {
    this.inFlight.delete(id)
  }

  /**
   * Push as much of the queue onto the socket as the kernel will take, stopping at the first byte
   * it refuses. `true` when everything handed to it is out.
   *
   * The drain used to be `while (pending.length) s.write(pending.shift()!)`, which took each frame
   * off the queue before knowing it had gone: a backlog past the socket's send buffer was destroyed
   * in silence, after every one of those frames had been reported as waiting in line and certain to
   * go out. Measured at 11 of 64 delivered against a peer reading as fast as it could. Nothing
   * leaves the queue here until its last byte is accepted.
   */
  private flush(s: import('bun').Socket): boolean {
    // A write into a socket that has already gone counts as nothing written, never as an exception
    // thrown out through a tool call: the frame stays queued, `close` fires, and it goes out on the
    // next connection — which is exactly what the agent was told would happen.
    const put = (b: Uint8Array): number => {
      try {
        return s.write(b)
      } catch {
        return 0
      }
    }
    for (;;) {
      if (this.unsent) {
        const rest = this.unsent.bytes.subarray(this.unsent.sent)
        const n = put(rest)
        if (n < rest.length) {
          this.unsent.sent += Math.max(n, 0)
          return false
        }
        this.unsent = null
      }
      // Nothing the agent sent goes out before the hub has said it will have us. A connected socket
      // proves only that something accepted; a frame written into a connection about to be refused
      // was reported delivered and then thrown away.
      const next = this.control.shift() ?? (this.state.up ? this.pending.shift() : undefined)
      if (!next) return this.unsent === null && this.control.length === 0 && this.pending.length === 0
      const n = put(next.bytes)
      if (n < next.bytes.length) {
        // The whole frame is kept, not just the tail: if this connection dies before the kernel
        // takes the rest, its head died with it, and putting a headless tail on the next one would
        // be read as a single unparseable line that takes a healthy frame down with it.
        this.unsent = { ...next, sent: Math.max(n, 0) }
        return false
      }
    }
  }

  /**
   * One frame's bytes: the envelope, the lease this run holds, and the payload.
   *
   * The lease goes on the ENVELOPE and nowhere else. An envelope flattens its payload, so a second
   * field of that name is the same key twice in one object — which the hub's reader refuses
   * outright — and a payload that named it alone would have the envelope swallow it and read back
   * as nothing. So anything arriving under that key in a payload is an envelope field that leaked
   * (a relay forwarding a producer's whole envelope is how), and it is dropped here rather than
   * put on the wire as this connection's number: one producer holding a number from before a
   * re-claim would otherwise fence the whole link.
   *
   * Stamped when the frame is written down, and NEVER rewritten afterwards. A run that lost its
   * socket comes back holding a newer lease and flushes what it queued under the old one, and
   * those bytes keep the old number on purpose: the hub refuses only a number it never granted, so
   * a backlog stamped behind the current lease is delivered. A re-stamp invented here is exactly
   * what would make a healthy run's whole backlog look like somebody else's.
   */
  private lineFor(id: string, payload: Record<string, unknown>): Uint8Array {
    const frame: Record<string, unknown> = { v: PROTOCOL_VERSION, id, ...payload }
    delete frame.generation
    if (this.lease !== undefined) frame.generation = this.lease
    return Buffer.from(JSON.stringify(frame) + '\n', 'utf8')
  }

  /** THE ONLY WRITER. Rule 1: the newline is appended here and nowhere else. */
  send(payload: Record<string, unknown>, what: string, askId?: string, owner?: string): Delivery {
    const id = this.nextId()
    const bytes = this.lineFor(id, payload)
    if (bytes.length > MAX_FRAME_BYTES) {
      // Refused rather than truncated. Half a message on a phone is worse than none and looks the
      // same as a whole one.
      this.o.note(`a frame was too big to send (${bytes.length} bytes); it was NOT sent`)
      return {
        delivered: false,
        permanent: true,
        why: `It is ${bytes.length} bytes and one message carries at most ${MAX_FRAME_BYTES}, so it was refused rather than cut in half. Say it in smaller pieces.`,
        id,
      }
    }
    // A frame the agent is about to be told will NEVER arrive is not held for later. Holding it
    // meant the tool said "no answer is coming, do not wait for one" and the queue then delivered
    // the question anyway, the moment the operator ran the enrol command that same message had
    // printed — so his phone buzzed with a question the agent had given up on, nothing would ever
    // take the buttons off it, and an answer came back for an ask that, as far as the agent knew,
    // was never made. The queue is cover for a gap that mends itself, and nothing else.
    if (!this.state.up && this.state.permanent) {
      return { delivered: false, permanent: true, why: this.state.why, id }
    }
    const stalled = this.state.up
      ? 'The hub is not keeping up, so this is waiting behind what is already going out.'
      : this.state.why
    if (this.pending.length >= this.maxPending) {
      // The reason the link is down travels WITH this refusal. Replacing it lost the only
      // actionable half — what to run — for every message from the 65th on, and left the agent
      // reading that a backlog was waiting for a link that was never coming back.
      this.o.note('the line of waiting frames is full; this one was dropped')
      return {
        delivered: false,
        permanent: true,
        why: `${this.maxPending} messages are already waiting to go out, so this one was let go — he will never see it, not even once the link is back. ${stalled}`,
        id,
      }
    }
    this.pending.push({ id, bytes, what, ...(askId ? { askId } : {}), ...(owner ? { owner } : {}) })
    this.remember(id, what, askId)
    if (this.state.up && this.sock && this.flush(this.sock)) return { delivered: true, id }
    return { delivered: false, permanent: false, why: stalled, id }
  }

  /**
   * How much of the queue one caller is currently holding — the frames not yet on the wire.
   *
   * The half-written frame counts, because it is occupying the head of the line. Frames already
   * accepted by the kernel do not: they are gone, and holding them against a caller's share would
   * make a busy but healthy producer look like a stuck one.
   */
  queuedFor(owner: string): number {
    let n = this.pending.reduce((acc, o) => acc + (o.owner === owner ? 1 : 0), 0)
    if (this.unsent?.owner === owner) n++
    return n
  }

  /**
   * Take every frame one caller still has waiting here off the queue, and hand them back.
   *
   * For a relay whose hub connection has just ended and is about to end its producers' sockets.
   * A producer's own close-time report then counts everything it handed over and heard nothing
   * back on — and frames still waiting HERE are among them: it told its agent they had gone out
   * with their fate unknown and would not be sent again, and this queue then sent them on the
   * next connection. They never went. Off the queue, and answered `no` by the caller before the
   * socket ends, "never got it" is the truth and nothing the agent was told to forget goes out
   * later. What the kernel took whole is not here; that is `onUnanswered`'s.
   */
  dropQueuedFor(owner: string): Outbound[] {
    const mine = this.pending.filter(o => o.owner === owner)
    if (!mine.length) return []
    this.pending = this.pending.filter(o => o.owner !== owner)
    for (const o of mine) this.inFlight.delete(o.id)
    return mine
  }

  private remember(id: string, what: string, askId?: string): void {
    this.inFlight.set(id, askId ? { what, askId } : { what })
    while (this.inFlight.size > this.maxPending * 2) {
      const oldest = this.inFlight.keys().next()
      if (oldest.done) break
      this.inFlight.delete(oldest.value)
    }
  }

  /**
   * Put a frame on the wire that nothing is waiting on — a pong, a goodbye.
   *
   * It goes through the same queue as everything else rather than straight to `s.write`, because a
   * direct write while a partly-sent frame is still waiting for room would splice itself into the
   * middle of that frame and destroy both.
   */
  sendControl(payload: Record<string, unknown>): void {
    const s = this.sock
    if (!s) return
    const id = this.nextId()
    this.control.push({ id, bytes: this.lineFor(id, payload), what: 'a liveness answer', control: true })
    this.flush(s)
  }

  /**
   * Record why the operator is out of reach, so a caller can say it instead of guessing.
   *
   * When nothing but a person can mend it, whatever is still queued is let go HERE and handed to
   * `onLost`. Those frames were each reported as waiting in line and certain to go out.
   */
  markDown(permanent: boolean, why: string): void {
    const wasUp = this.state.up
    this.state = { up: false, permanent, why }
    // Said before the queue is emptied, because a caller standing in for the hub has to stop
    // telling anybody he was reached BEFORE it starts telling them what was lost.
    if (wasUp || permanent) this.o.onState?.(false)
    if (!permanent) return
    const lost = [...(this.unsent && !this.unsent.control ? [this.unsent] : []), ...this.pending]
    if (!lost.length) return
    this.pending = []
    if (this.unsent && !this.unsent.control) this.unsent = null
    for (const o of lost) this.inFlight.delete(o.id)
    this.o.onLost?.(lost, why)
  }

  /**
   * The hub has taken us, so the queue drains HERE and not at `open`.
   *
   * The backoff resets here too: it used to reset on every connect, which meant a hub that accepted
   * and then refused was dialled again a second later, forever, instead of being left alone.
   */
  /**
   * Hand back every frame this connection took whole and never heard an answer for.
   *
   * Called once per close, BEFORE the link is marked down: a relay standing in for the hub ends
   * its producers' sockets on the way down, and what is said after that is said into a socket
   * that is closing. What is still queued is not touched — it goes out on the next connection, and
   * that is what its caller was told.
   */
  private answerForTheFlushed(why: string): void {
    const queued = new Set(this.pending.map(o => o.id))
    const gone: Unanswered[] = []
    for (const [id, f] of this.inFlight) if (!queued.has(id)) gone.push({ id, ...f })
    if (!gone.length) return
    for (const g of gone) this.inFlight.delete(g.id)
    this.o.onUnanswered?.(gone, why)
  }

  markUp(): void {
    this.state = { up: true }
    this.reachedWelcome = true
    this.backoff = 1000
    this.saidItWasDown = false
    if (this.sock) this.flush(this.sock)
    this.o.onState?.(true)
  }

  /** End this connection now — a refusal this side is making, rather than one it was given. */
  end(): void {
    this.sock?.end()
  }

  /**
   * Stop for good: this run is over and no amount of dialling can change it.
   *
   * `markDown(true, …)` alone is not that. Nearly every permanent refusal is one a PERSON mends
   * while the session is still running — a project enrolled, a secret granted — and the dial loop
   * goes on running behind it precisely so that the recovery a tool result prescribes takes effect
   * without a restart. One refusal is not like that: a run a later run has replaced can never be
   * admitted again under this identity, and dialling on would be the session spending the rest of
   * its life asking to come back to an address that has moved on.
   */
  giveUp(why: string): void {
    this.givenUp = true
    this.markDown(true, why)
    this.sock?.end()
  }

  start(): void {
    this.connect()
  }

  private connect(): void {
    const who = this.o.identify()
    if ('refuse' in who) {
      const r = who.refuse
      this.markDown(r.permanent, r.why)
      if (!this.saidItWasDown) {
        this.o.note(r.note)
        this.saidItWasDown = true
      }
      if (r.retryMs !== undefined) setTimeout(() => this.connect(), r.retryMs)
      return
    }

    // Rule 3: the ceiling bounds ONE frame. `buf` is cleared at every newline, so a long
    // conversation cannot accumulate into a false "frame too large".
    let buf = ''
    this.sawRefusal = false

    Bun.connect({
      unix: who.socket,
      socket: {
        open: s => {
          this.sock = s
          // The link is NOT up yet. A connected socket proves only that something accepted; the hub
          // can still refuse this project and close, and a frame written into a connection about to
          // be refused is a frame the agent was told had been delivered and which was then thrown
          // away. `welcome` is the hub saying it took us, and that is where the queue drains.
          this.sendControl(who.hello)
        },
        data: (s, chunk) => {
          buf += chunk.toString()
          // Rule 2: read exactly one line at a time, and never to EOF.
          for (;;) {
            const nl = buf.indexOf('\n')
            if (nl < 0) break
            const line = buf.slice(0, nl)
            buf = buf.slice(nl + 1)
            if (line.trim()) this.handle(line)
          }
          if (buf.length > MAX_FRAME_BYTES) {
            this.o.note('the hub sent a line past the ceiling; dropping the connection')
            buf = ''
            s.end()
          }
        },
        // The kernel has room again. Whatever it would not take last time goes now — without this,
        // a frame the queue is still holding waits for the next thing the agent happens to send.
        drain: s => {
          this.flush(s)
        },
        close: () => {
          const wasUp = this.state.up
          this.sock = null
          // A frame half-written into a socket that has closed cannot be finished, and its head is
          // already gone. Put nothing of it on the next connection: a headless tail there would be
          // read as one unparseable line and take a healthy frame down with it. The frames it had
          // not started go back to waiting, which is what the caller was told they were doing.
          if (this.unsent && !this.unsent.control) {
            const { sent: _started, ...whole } = this.unsent
            this.pending.unshift(whole)
          }
          this.unsent = null
          this.control = []
          // Everything the kernel took whole on this connection and the hub never answered for is
          // answered for by nobody now. Said before the link is marked down, for the reason on
          // `answerForTheFlushed`; said with the reason already recorded when there is one.
          const dropped = this.o.whenDropped ?? 'The link to his phone dropped and is being rebuilt.'
          this.answerForTheFlushed(wasUp ? dropped : this.state.why)
          // A reason already recorded — a refusal, say — outlives the close it caused, because it
          // explains the silence far better than "the link dropped" does.
          if (wasUp) this.markDown(false, dropped)
          // A socket that opened and then closed before `welcome` is the silent close: a uid the
          // hub reads as another user, or a `hello` it could not decode, look identical from here.
          // Reported so a caller proving reachability can say so; not reported when it closed after
          // `welcome` (a live link dropping) or after a `refused` the caller already heard — the
          // hub closes right after refusing, and that close explains nothing the refusal did not.
          else if (!this.reachedWelcome && !this.sawRefusal) this.o.onDial?.({ closedBeforeWelcome: true })
          // A caller proving reachability dials once and stops; a caller holding a claim redials —
          // unless it has been told there is no way back for it at all.
          if (this.o.once || this.givenUp) return
          setTimeout(() => this.connect(), this.backoff)
          this.backoff = Math.min(this.backoff * 2, 60_000)
        },
        error: (_s, e) => {
          this.o.note(`socket error: ${(e as Error)?.message ?? e}`)
        },
      },
    }).catch((e: unknown) => {
      this.sock = null
      this.unsent = null
      this.control = []
      this.markDown(false, this.o.whenUnreachable)
      // The errno, not just "something is down". `ENOENT`, `ECONNREFUSED` and `EACCES` are three
      // different fixes, and the module used to throw the distinction away (TAXONOMY §8).
      const code = (e as { code?: string })?.code ?? 'UNKNOWN'
      this.o.onDial?.({ code })
      if (!this.saidItWasDown) {
        this.o.note(`nothing is listening at ${who.socket} (${code}); retrying`)
        this.saidItWasDown = true
      }
      if (this.o.once || this.givenUp) return
      setTimeout(() => this.connect(), this.backoff)
      this.backoff = Math.min(this.backoff * 2, 60_000)
    })
  }

  private handle(line: string): void {
    let frame: Record<string, any>
    try {
      frame = JSON.parse(line)
    } catch {
      // A line this build cannot read is one bad frame, not a dead hub — and it is exactly what a
      // hub one version ahead sends. Ignored, never fatal.
      this.o.note('a frame from the hub could not be read; ignoring it')
      return
    }
    if (frame.t === 'ping') {
      // A caller proving reachability (`once`) must NOT pong: the hub makes the topic only after a
      // pong, so answering would greet a topic on the phone — the one thing a check must not do.
      // `welcome` is proof enough there; the ping is simply ignored.
      if (this.o.once) return
      // Answered HERE and never handed upstairs. The nonce is the ping's own envelope id; the
      // answer names it. Liveness is what keeps the claim, so it must not wait on anything above
      // this module being awake — a fan-in with a wedged producer would otherwise lose the lane.
      this.sendControl({ t: 'pong', ref: frame.id })
      return
    }
    if (frame.t === 'welcome') {
      // The lease, read off the welcome's OWN envelope — there is no payload field of that name
      // and there cannot be one, for the reason on `lineFor`. Read on every welcome and not once:
      // a hub replaced under a reconnecting bridge grants its own number, and a hub that grants
      // none is saying it fences nothing, which is a thing to forget rather than to remember.
      const granted = frame.generation
      // A caller proving reachability (`once`) holds no place and stamps nothing. The hub gives the
      // number back to a connection that never became live, so a number this link put on its `bye`
      // would be a claim to an address the check is deliberately not taking — and it is the one
      // thing that could make the hub say a check's lease was over, for a check that has none.
      this.lease =
        !this.o.once && typeof granted === 'number' && Number.isSafeInteger(granted) && granted > 0
          ? granted
          : undefined
    }
    if (frame.t === 'refused') this.sawRefusal = true
    this.o.onFrame(frame)
  }
}
