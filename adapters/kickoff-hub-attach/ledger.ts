/**
 * Who the producers are, what they are waiting on, and what has to outlive this process.
 *
 * Standing in front of the hub means taking on the hub's lifecycle job. The hub retires a dead
 * asker's questions from two facts it reads off the CLAIM — the `instance` in `hello`, and the pid
 * holding the socket — and behind a relay both of those are the RELAY's, for every producer, for
 * ever. Neither of the hub's sweeps can see a producer die, so the operator would be left looking
 * at a keyboard for a question whose agent no longer exists, and his tap on it would be answered
 * once, at the hub, into nothing.
 *
 * This file does that job with the facts the relay actually has, and they are the same two facts in
 * a different form: a producer's socket is gone, AND nothing has come back under its name.
 *
 * It is separate from the door and the wire because it is a different question. `fanin.ts` answers
 * "how does a frame get from this socket to that one"; this answers "whose was it, and is that
 * somebody still there". Nothing here knows what a socket is.
 */

import { randomBytes } from 'crypto'
import { readFileSync, writeFileSync } from 'fs'

/** One question a producer is still waiting on: who asked it, the id THEY used, and when. */
export type OpenAsk = { key: string; askId: string; at: number }

export type LedgerOptions = {
  /** Where to write down what a restart will need. Beside the socket, so it shares its lifetime. */
  file: string
  /** Say something in this process's own transcript. The operator cannot see it; a developer can. */
  note: (msg: string) => void
  /**
   * How long a producer has to come home before its open questions are taken off the phone.
   *
   * A producer redials on its own backoff, which doubles up to a minute, so anything much shorter
   * withdraws a question that is about to be answered. Longer costs only a keyboard sitting on his
   * phone a little while after the agent behind it has gone.
   */
  graceMs: number
  /** Is that producer here right now? Half of the proof that its questions can be withdrawn. */
  isAttached: (key: string) => boolean
  /**
   * Put a frame on the hub link on the relay's OWN behalf. False when it will never go.
   *
   * The caller sends it, because the caller owns the wire — and it is the caller that has to
   * recognise the `ack` for one of these as its own rather than as one nobody is waiting on.
   */
  /** `ok` is taken by the link; `delivered` is on the wire, as opposed to waiting in its queue. */
  sendUp: (payload: Record<string, unknown>, what: string, owner: string) => { ok: boolean; delivered?: boolean; why?: string }
}

/**
 * How long a question can still be tapped.
 *
 * The hub drops a record past Telegram's 48-hour edit window, because past it no keyboard can be
 * taken off by anybody. A routing entry older than that can never be used, and keeping it is how a
 * map that is only ever added to becomes a leak.
 */
const SHELF_MS = 48 * 60 * 60 * 1000

/**
 * A ceiling on the routing map, as a backstop and nothing more.
 *
 * The map used to be capped at 256 and evicted by INSERTION ORDER, so the 257th question un-routed
 * the 1st while its buttons were still live on his phone — and a permission prompt per command
 * reaches 257 in an afternoon. Entries are dropped when their question CLOSES and when they age out
 * now; this only bounds a hub that has stopped answering entirely, and it says so out loud.
 */
const MAX_OPEN = 4096

export class Ledger {
  private readonly o: LedgerOptions

  /**
   * This relay's name at the hub, kept across a hub reconnect AND across a restart of this process.
   *
   * The hub voids every open question when a bridge presents a NEW instance. Producers outlive this
   * process — an opencode server holds its MCP children across a relay restart — so a new instance
   * here would void questions those agents are still waiting on, with nothing to tell them.
   *
   * The direct-to-hub path has no version of this problem: a channel plugin mints one instance and
   * keeps it for the life of the session, and the hub reloads its ledger from disk, so restarting
   * `herdr-tg` under a live session leaves that session's questions answerable.
   */
  readonly instance: string

  /** namespaced ask id → who asked it. A tap from the hub is routed by this and nothing else. */
  private readonly open = new Map<string, OpenAsk>()

  /** key → its small number, kept so a producer that comes back keeps its ask ids. */
  private readonly numbers = new Map<string, number>()

  /** Producers whose socket has gone, and the timer that will withdraw their questions. */
  private readonly leaving = new Map<string, ReturnType<typeof setTimeout>>()

  private count = 0
  private saving: ReturnType<typeof setTimeout> | null = null

  constructor(o: LedgerOptions) {
    this.o = o
    const past = this.readBack()
    // Random and then the millisecond, never a pid: this string is what the HUB knows this door by
    // and keys every open question on, and a pid is meaningful only to this kernel and is issued
    // again once its numbers wrap.
    //
    // A ledger left by the last run keeps the name it already had ONLY while a question is still
    // open under it — that is the whole reason to keep it, because those buttons are on his phone
    // under that name and the hub retires everything belonging to a name it has not heard before.
    // With nothing open the old name means nothing to anybody, and holding it anyway is how a box
    // that ran this adapter before the pid came off the wire would go on saying a pid for ever:
    // the state file outlives every restart, so the migration would never reach a deployed box.
    this.instance = past.asks.length && past.instance
      ? past.instance
      : `${randomBytes(8).toString('hex')}-${Date.now()}`
    for (const [k, n] of past.numbers) this.numbers.set(k, n)
    for (const [ns, a] of past.asks) this.open.set(ns, a)
    this.count = Math.max(past.count, ...[...this.numbers.values(), 0])
  }

  /**
   * Read back what the last run left, or start clean.
   *
   * Nothing here is trusted for anything but routing this relay's own conversation back to its own
   * producers: it was written by this process, beside a socket only this user can reach. A file
   * that will not parse is treated as no file at all rather than as a reason to refuse to start —
   * refusing would leave the lane with no voice at all over a bookkeeping detail.
   */
  private readBack(): { instance: string | null; count: number; numbers: [string, number][]; asks: [string, OpenAsk][] } {
    const empty = { instance: null, count: 0, numbers: [], asks: [] }
    try {
      const raw = JSON.parse(readFileSync(this.o.file, 'utf8'))
      if (!raw || typeof raw.instance !== 'string' || !raw.instance.length) return empty
      const fresh = Date.now() - SHELF_MS
      return {
        instance: raw.instance,
        count: Number(raw.count) || 0,
        numbers: Array.isArray(raw.numbers) ? raw.numbers : [],
        // A question past the hub's own shelf life cannot be tapped by anybody, so restoring it
        // would only mean withdrawing buttons that are already gone.
        asks: (Array.isArray(raw.asks) ? raw.asks : []).filter(
          (e: any) => Array.isArray(e) && e[1] && Number(e[1].at) > fresh,
        ),
      }
    } catch {
      return empty
    }
  }

  /**
   * Write down what the next run will need, soon but not on every frame.
   *
   * Debounced because a producer under a hub outage can open questions faster than a disk wants to
   * be rewritten, and losing the last quarter-second costs at worst one withdrawal that the grace
   * timer performs anyway. A shutdown writes it out at once.
   */
  private remember(): void {
    if (this.saving) return
    this.saving = setTimeout(() => this.rememberNow(), 250)
  }

  rememberNow(): void {
    if (this.saving) clearTimeout(this.saving)
    this.saving = null
    try {
      // Only the keys that still matter: one with no open question and nothing attached will never
      // be looked up again, and keeping it is how a file that is only appended to becomes a leak.
      const live = new Set([...this.open.values()].map(a => a.key))
      for (const k of this.numbers.keys()) if (this.o.isAttached(k)) live.add(k)
      writeFileSync(
        this.o.file,
        JSON.stringify({
          instance: this.instance,
          count: this.count,
          numbers: [...this.numbers].filter(([k]) => live.has(k)),
          asks: [...this.open],
        }),
        { mode: 0o600 },
      )
    } catch (e) {
      // Not fatal: the lane still has a voice. What is lost is the next restart's ability to route a
      // tap, and the grace timer takes those buttons off rather than leaving them dead.
      this.o.note(`could not write down what a restart would need: ${(e as Error)?.message ?? e}`)
    }
  }

  /**
   * The small number this producer's questions are namespaced under.
   *
   * Reused when the key has been seen before — including in a previous run of this process — so a
   * producer that reconnects keeps the namespace its still-open questions were asked under. A new
   * key always gets a number above every one ever handed out, so two producers can never collide.
   */
  numberFor(key: string): number {
    const known = this.numbers.get(key)
    if (known !== undefined) return known
    this.numbers.set(key, ++this.count)
    this.remember()
    return this.count
  }

  /** A producer is here. Whatever it left open is its own again, and stays open. */
  attached(key: string): boolean {
    const waiting = this.leaving.get(key)
    if (!waiting) return false
    clearTimeout(waiting)
    this.leaving.delete(key)
    return true
  }

  /** Write down a question, so a tap on it can be found its way home. */
  opened(ns: string, key: string, askId: string): void {
    this.forgetWhatCannotBeTapped()
    this.open.set(ns, { key, askId, at: Date.now() })
    this.remember()
  }

  /** A question has closed — tapped, withdrawn by its agent, or never delivered at all. */
  closed(ns: string): void {
    if (this.open.delete(ns)) this.remember()
  }

  /** Who asked this, if anybody here did. */
  who(ns: string): OpenAsk | undefined {
    return this.open.get(ns)
  }

  /** Every producer with something still open — asked at startup, of what the last run left. */
  waitingOn(): string[] {
    return [...new Set([...this.open.values()].map(a => a.key))]
  }

  get openCount(): number {
    return this.open.size
  }

  /**
   * Drop the routing for questions no tap can arrive for any more, oldest first.
   *
   * The map is in insertion order and every entry is stamped when it was made, so this stops at the
   * first one still inside the window rather than walking the whole map on every ask.
   */
  private forgetWhatCannotBeTapped(): void {
    const dead = Date.now() - SHELF_MS
    for (const [ns, a] of this.open) {
      if (a.at > dead) break
      this.open.delete(ns)
    }
    while (this.open.size > MAX_OPEN) {
      const oldest = this.open.keys().next()
      if (oldest.done) break
      // Loud, because this one really is a live question losing its way back: nothing but a hub
      // that has stopped resolving anything for two days can reach here.
      this.o.note(`more than ${MAX_OPEN} questions are open at once; the oldest (${oldest.value}) can no longer be answered`)
      this.open.delete(oldest.value)
    }
  }

  /**
   * Take the buttons off a producer's open questions, unless it comes home first.
   *
   * The two facts here are the same two the hub insists on, in the only form this process can have
   * them: its socket is gone, AND nothing has come back under its name within a window a reconnect
   * comfortably beats.
   *
   */
  withdrawWhatItLeaves(key: string): void {
    if (!this.mine(key).length) return
    const already = this.leaving.get(key)
    if (already) clearTimeout(already)
    this.leaving.set(
      key,
      setTimeout(() => {
        this.leaving.delete(key)
        if (this.o.isAttached(key)) return
        const mine = this.mine(key)
        if (!mine.length) return
        this.o.note(`a producer did not come back; taking the buttons off ${mine.length} question(s) it left open`)
        for (const ns of mine) {
          this.open.delete(ns)
          const d = this.o.sendUp({ t: 'ask_resolved', ask_id: ns, how: 'withdrawn' }, `taking the buttons off ${ns}`, key)
          if (!d.ok) this.o.note(`the buttons on ${ns} could not be taken off: ${d.why}`)
        }
        this.rememberNow()
      }, this.o.graceMs),
    )
  }

  /**
   * Take the buttons off EVERY open question, now, with a reason he can read.
   *
   * For the moment the engine behind the door exits under `--run`. The grace above exists because
   * a producer that went away may be a tool server restarting in place; an engine that has exited
   * is a different fact — nothing behind the door can answer any more, and this process is about to
   * exit with it, which is what kills the grace timer before it ever fires. That left a question
   * with live buttons on his phone and nothing behind them, and his tap went to a lane that no
   * longer existed.
   *
   * A withdrawal the link would not take stays open and is written down, so the next run of this
   * door takes those buttons off at start rather than forgetting them — and so does one the link
   * took but has not yet written, because this process exits before a queue gets a second chance
   * and a question forgotten here with its buttons still on his phone is forgotten for good. A
   * withdrawal sent twice costs the hub nothing; a keyboard nobody withdraws stays until it ages
   * out. Returns how many were handed to the link.
   */
  withdrawEverythingNow(outcome: string): number {
    for (const t of this.leaving.values()) clearTimeout(t)
    this.leaving.clear()
    let sent = 0
    for (const [ns, a] of [...this.open]) {
      const d = this.o.sendUp({ t: 'ask_resolved', ask_id: ns, how: 'withdrawn', outcome }, `taking the buttons off ${ns}`, a.key)
      if (!d.ok) {
        this.o.note(`the buttons on ${ns} could not be taken off: ${d.why}`)
        continue
      }
      sent++
      if (d.delivered) this.open.delete(ns)
      else this.o.note(`the withdrawal of ${ns} is still waiting to go out; it stays written down for the next run`)
    }
    this.rememberNow()
    return sent
  }

  private mine(key: string): string[] {
    return [...this.open].filter(([, a]) => a.key === key).map(([ns]) => ns)
  }
}
