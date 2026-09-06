/**
 * The door — one slot at the hub, served by one or more local producers.
 *
 * This is the relay `adapters/fanin/` used to be a process of its own. It is a module now, because
 * `kickoff-hub-attach` is one command that holds the claim, opens this door, watches an opencode
 * server and can be a wall's entrypoint — and the relay's logic is the product, where its process
 * was only a process. `main.ts` starts it; `opencode.ts` attaches to it AS A PRODUCER, in the same
 * process, over this same door (see §13.2 of `docs/ATTACHING.md`). The hub still sees one `hello`,
 * one pid, one claim — it never learns there were several.
 *
 * The hub admits exactly ONE live connection per addressable thing, and on opencode two things want
 * to speak for one lane:
 *
 *   * the MCP tool server, carrying what the agent CHOSE to say — `reply`, `ask`, `done`;
 *   * the event watcher, carrying the permission and question prompts the agent did NOT choose.
 *
 * Both dialling the hub means the second is refused with `already_claimed`, so the joining happens
 * HERE, on the adapter side of the seam. `docs/INTERFACES.md` keeps a closed list of six hub
 * capabilities, and a seventh is a decision rather than a refactor.
 *
 * # It is not a pipe, and these are the reasons why
 *
 *   1. **It answers `hello` itself**, with the `welcome` it is holding — including the `lane` echo,
 *      so a producer's refuse-rather-than-impersonate check keeps working unmodified.
 *   2. **It rewrites envelope ids.** Two producers both mint `b1`, and an `ack` that named the
 *      wrong frame would tell the wrong agent its message was lost.
 *   3. **It namespaces `ask_id`.** Two producers can both mint `a1`, and the hub resolves a tap by
 *      ask id against a written record — so without this, a tap on one agent's question would be
 *      delivered to the other agent, which is the worst thing in this file.
 *   4. **It answers the hub's `ping` itself** — in `hub-link.ts`, never forwarded — so a producer
 *      that has wedged cannot cost the lane its liveness and therefore its claim.
 *   5. **It shares the queue out among the producers actually here.** One chatty producer must not
 *      fill the line another producer's question needs, and a fixed cap is not a share: two at 28
 *      hold 56 of 64 and the third is refused before it has said a word.
 *   6. **It ends a producer's connection when its OWN link drops.** A producer's link is up
 *      whenever this process is running, so nothing else could tell it the hub had gone — and it
 *      went on being told "said" with nothing behind the relay at all. The wire has no way to
 *      un-welcome anybody; the honest signal is the one the direct path gives, which is the
 *      connection ending.
 *   7. **It keeps a producer's lifecycle**, which standing in front of the hub took away from the
 *      hub — who a producer is, what it is waiting on, and what has to survive a restart of this
 *      process. That is `ledger.ts`, and the argument for every part of it is written there.
 */

import { mkdirSync, statSync, unlinkSync } from 'fs'
import { dirname } from 'path'

import type { Facts, Project } from '../../plugins/kickoff-channel/where.ts'
import { HubLink, MAX_FRAME_BYTES, PROTOCOL_VERSION, type Outbound, type Unanswered } from '../../plugins/kickoff-channel/hub-link.ts'
import { Ledger } from './ledger.ts'

/** Everything the door needs, worked out once by `main.ts` from the namespace it read. */
export type RelayConfig = {
  /** The directory this relay speaks for — for the one message that has to name a folder to enrol. */
  projectDir: string
  /** What git says about that directory; `mainTop` names the folder to enrol. */
  facts: Facts
  /** The conversation this relay holds, or null when it speaks for the project itself. */
  address: string | null
  /** The door this relay listens on. */
  listen: string
  /** The hub's own socket. */
  hubSocket: string
  /** How long a departed producer has to come home before its questions come off the phone. */
  graceMs: number
  /**
   * The `instance` of the one producer that carries the operator's typed words — the watcher
   * `--opencode` starts — or null when there is none. Named so that when every producer refuses
   * his words, the reason forwarded is the carrier's: a tool server's "this engine cannot" is
   * true and useless beside "the worker has no session open".
   */
  carrier: string | null
  /**
   * The enrolled project, resolved AFRESH on every attempt — never once, because the operator may
   * `herdr-tg enroll` while this is running and that is the documented recovery from unknown_project.
   */
  secretOf: () => Project | null
  /** Say something in this process's own transcript. The operator cannot see it; a developer can. */
  note: (msg: string) => void
  /** Refuse to start and exit 2, naming what has to change. */
  die: (msg: string) => never
}

/** A running door. `main.ts` binds it, starts it, and says goodbye through it. */
export type Relay = {
  /** Bind the door, refusing (exit 2) to displace a live relay for this address. */
  bind(): Promise<void>
  /** Start dialling the hub. */
  start(): void
  /** Write the ledger, say `bye`, unlink the door. Best effort, time-boxed. */
  goodbye(): void
  /** Where the door is — for the first-line output and for `--run` to hand to the child. */
  readonly listenPath: string
}

export function createRelay(cfg: RelayConfig): Relay {
  const { projectDir: PROJECT_DIR, facts: FACTS, address: OUR_LANE, listen: LISTEN, note, die } = cfg

  /** What this relay knows that must outlive it: who it is, and which questions are still open. */
  const REMEMBERED = `${LISTEN}.state`

  const GRACE_MS = cfg.graceMs

  // ─────────────────────────────────────────────────────────────────────────────────────────────
  // Writing to a producer.

  /**
   * One producer's side of the socket, with the same whole-frame rule the hub link keeps.
   *
   * A short write is normal on a socket under load. Bun's `Socket.write` returns how many bytes the
   * kernel took and DROPS the rest, so treating a short write as sent leaves a headless tail that
   * swallows the next healthy frame into one unparseable line. Local sockets are not exempt.
   */
  class Down {
    private q: Uint8Array[] = []
    private unsent: { bytes: Uint8Array; sent: number } | null = null
    private seq = 0
    /** Close as soon as the last byte is out, never before. */
    private closing = false

    constructor(private readonly s: import('bun').Socket) {}

    /** THE ONLY WRITER for this producer. The newline is appended here and nowhere else. */
    write(payload: Record<string, unknown>): void {
      const id = `d${++this.seq}`
      this.q.push(Buffer.from(JSON.stringify({ v: PROTOCOL_VERSION, id, ...payload }) + '\n', 'utf8'))
      this.flush()
    }

    /** Relay a frame the hub minted, keeping its own envelope id: it is opaque to the producer. */
    relay(frame: Record<string, unknown>): void {
      this.q.push(Buffer.from(JSON.stringify(frame) + '\n', 'utf8'))
      this.flush()
    }

    /**
     * Say goodbye once what is already queued has gone.
     *
     * Ending the socket the instant a refusal is queued truncates it whenever the kernel would not
     * take the whole frame — and a producer that never reads its refusal waits for a link that is
     * never coming, which is the one failure mode every sentence in this system exists to prevent.
     */
    endAfterFlush(): void {
      this.closing = true
      this.flush()
    }

    flush(): void {
      const put = (b: Uint8Array): number => {
        try {
          return this.s.write(b)
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
            return
          }
          this.unsent = null
        }
        const next = this.q.shift()
        if (!next) {
          if (this.closing) this.s.end()
          return
        }
        const n = put(next)
        if (n < next.length) {
          this.unsent = { bytes: next, sent: Math.max(n, 0) }
          return
        }
      }
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────────────────────
  // Producers.

  type Producer = {
    /**
     * Who this producer IS, taken from the `instance` in its own `hello`.
     *
     * Not its socket. A producer that reconnects is the same agent, still waiting on the same
     * questions, and routing by socket handed its tap to a corpse. A producer that RESTARTED mints
     * a new instance and is correctly a new voice.
     */
    key: string
    /**
     * Small and stable for this key, so a namespaced ask id survives the producer reconnecting.
     *
     * The key itself would do the job, and would put a whole instance string in every ask id for no
     * reader's benefit; this is the same thing short enough to read in a log.
     */
    n: number
    s: import('bun').Socket
    down: Down
    buf: string
    /** Its `hello` was checked against the secret and the lane this relay holds. */
    checked: boolean
    /** It has been given a `welcome`, so its own queue has been allowed to drain. */
    greeted: boolean
  }

  const producers = new Set<Producer>()

  /** Who is attached right now, by key. A frame from the hub is routed through this and nothing else. */
  const attached = new Map<string, Producer>()

  /** hub frame id → whose it was and the id IT used. An `ack` names the frame the producer sent. */
  const byHubFrame = new Map<string, { key: string; ref: string; askUp?: string }>()

  /** Frames this relay sent for itself, so their `ack` is not read as one nobody is waiting on. */
  const ourOwn = new Set<string>()

  /**
   * His typed words, handed to several producers, and who has yet to answer for them — keyed by
   * the hub's own id for the `message`, which is what a producer's `ack` names.
   *
   * The hub keeps the FIRST answer it hears for a message and no other, so the door cannot forward
   * every producer's answer as it comes. On opencode the tool server refuses at once — nothing on
   * that engine reads a channel message, and saying so is no more than a lookup — while the
   * watcher goes to the server and back, so forwarded as they came the operator read "this engine
   * cannot" on the days the watcher could have said why, and "accepted" was never the first when
   * both spoke. So: one `accepted` from anybody goes up the moment it arrives, and a `refused`
   * goes up only once every producer the words were handed to has refused or gone — carrying the
   * carrier's reason when the carrier is among them (`cfg.carrier`). Bounded, oldest first out,
   * because a producer that never answers must not turn this into a leak.
   */
  type TypedWords = {
    waiting: Set<string>
    refusals: { key: string; reason: string }[]
    /** How many entries his message carried in `files`. Zero for ordinary typed words. */
    files: number
    /** The most any one producer has said it handed on. */
    handedOn: number
    /** Whether anybody took the words at all. */
    accepted: boolean
  }
  const typedWords = new Map<string, TypedWords>()
  const TYPED_WORDS_KEPT = 64

  /**
   * Who the producers are and what they are waiting on — including across a restart of this process.
   *
   * `sendUp` is a closure rather than the link itself because the ledger has no business knowing
   * what a socket is: it decides WHAT must be said and this file decides how it goes out.
   */
  const ledger = new Ledger({
    file: REMEMBERED,
    note,
    graceMs: GRACE_MS,
    isAttached: key => attached.has(key),
    sendUp: (payload, _what, owner) => {
      const d = link.send(payload, _what, undefined, owner)
      if (d.delivered || !d.permanent) {
        rememberOurOwn(d.id)
        return { ok: true }
      }
      return { ok: false, why: d.why }
    },
  })

  /**
   * A frame this door sent for itself is one whose `ack` nobody upstairs is waiting on.
   *
   * Bounded for the same reason every other map here is: a hub that stopped acking must not turn
   * a set nobody reads any more into a slow leak.
   */
  function rememberOurOwn(id: string): void {
    ourOwn.add(id)
    while (ourOwn.size > 256) ourOwn.delete(ourOwn.values().next().value!)
  }

  /** Say something to the hub on this door's OWN behalf — an answer for a frame it received. */
  function answerHub(payload: Record<string, unknown>, what: string): void {
    const d = link.send(payload, what)
    if (d.delivered || !d.permanent) rememberOurOwn(d.id)
    else note(`the hub link would not take ${what}: ${d.why}`)
  }

  /** The one place a producer's ask id becomes the hub's, so the two can never drift apart. */
  const namespaced = (p: Producer, askId: string) => `${p.n}~${askId}`

  /**
   * Tell a producer one of its frames did not reach the operator.
   *
   * `too-fast` is the hub's own word for a frame shed because too much was offered at once, and it
   * is the truth here: this producer's share of the line was full. The producer turns it into the
   * sentence its agent reads, which already says the message will not be tried again — so the agent
   * stops waiting instead of believing something is still on its way.
   */
  function refuseFrame(p: Producer, ref: string, why?: string): void {
    p.down.write({ t: 'ack', ref, delivered: 'no', ...(why ? { why } : {}) })
  }

  // ─────────────────────────────────────────────────────────────────────────────────────────────
  // The hub link.

  /** The `welcome` the hub gave us, held so every producer can be answered with the same one. */
  let welcomeFrame: Record<string, any> | null = null

  /** The secret this relay authenticated with, so a producer's own token can be compared to it. */
  let secretHeld: string | null = null

  /**
   * When the current RUN of `already_claimed` refusals began, or null outside one.
   *
   * The bridge counted three refusals in a row and called that a squatter; the relay never had the
   * rule and would wait on a squatter for ever, saying only "the hub would not take this relay". But
   * three dials with the link's backoff is a claim held for THREE SECONDS — shorter than the ten a
   * predecessor attach gets to stop (§13.5), and shorter than the operator's own Claude session
   * takes to get out of the way — so an ordinary restart tripped it and took the whole conversation
   * off the phone for good. A squatter is a claim held LONGER than anything legitimate holds one,
   * so the rule is elapsed time, not a count. It is a run and not a lifetime tally — cleared on
   * every `welcome` — because two ordinary restarts months apart must not add up.
   */
  let heldSince: number | null = null

  /**
   * Longer than a predecessor's stop (ten seconds, then a kill, then its `bye`) with room to spare.
   * With the link's backoff the refusals land at 0, 1, 3, 7, 15 and 31 seconds, so in practice this
   * is the sixth.
   */
  const STUCK_AFTER_MS = 30_000

  const link = new HubLink({
    framePrefix: 'r',
    note,
    whenUnreachable: 'The hub is not running, so nothing can reach his phone until it is back.',
    identify() {
      const project = cfg.secretOf()
      if (!project) {
        return {
          refuse: {
            permanent: true,
            // The main working tree when git says there is one, and the directory this relay was
            // pointed at when it does not — a container has no git, and "enroll undefined" is an
            // instruction nobody can carry out.
            why: `This project is not enrolled, so the hub has no way to know which project it is. Run:  herdr-tg enroll ${FACTS.mainTop ?? PROJECT_DIR}`,
            note: `no secret under ${FACTS.mainTop ?? PROJECT_DIR}. Run:  herdr-tg enroll ${FACTS.mainTop ?? PROJECT_DIR}`,
            // Retried, because the operator may enrol the project while this is running and that is
            // the recovery the message prescribes.
            retryMs: 30_000,
          },
        }
      }
      secretHeld = project.token
      return {
        socket: cfg.hubSocket,
        hello: {
          t: 'hello',
          project_id: `unknown-until-the-hub-says`,
          token: project.token,
          // Held across a hub reconnect AND across a restart of this process — see `ledger.ts`.
          instance: ledger.instance,
          repo: project.repo,
          // This process's OWN pid, never a producer's: the hub evicts a claim whose pid has gone,
          // and a claim held under a pid that is not the one holding the socket is a claim the
          // eviction rule cannot reason about.
          pid: process.pid,
          ...(OUR_LANE ? { lane: OUR_LANE } : {}),
        },
      }
    },
    onFrame: fromHub,
    /**
     * Stand in for the hub honestly, in both directions.
     *
     * A producer's own link is up whenever this relay is running, so nothing else could tell it the
     * hub had gone: it went on being told "said", "sent", "asked" with nothing behind the relay at
     * all — the exact sentence the agent then repeats to the operator as "your phone buzzed". The
     * wire has no way to un-welcome anybody, so the honest signal is the one the direct path already
     * gives: the connection ends. A producer's own link then drops, its queue goes back to waiting,
     * it says "not said yet", and it redials and is welcomed again when there is something to be
     * welcomed to.
     */
    onState(up: boolean) {
      if (up) {
        for (const p of producers) greet(p)
        return
      }
      welcomeFrame = null
      for (const p of producers) {
        if (!p.greeted) continue
        p.greeted = false
        // What it handed this door that never left. The door's own line is not the hub's: a
        // frame still waiting in it when the hub connection ended went nowhere, and the socket
        // about to be ended is the one its producer's close-time report runs on — which counts
        // everything handed over and not heard back on, these included. The agent was told they
        // had gone out with their fate unknown and would not be sent again, and this door then
        // sent them on the next connection. Off the line and answered `no` BEFORE the socket
        // ends, "never got it" is the truth and nothing the agent was told to forget is sent.
        // The routing for a question that never went is dead weight, as it is for one the hub
        // said no to.
        for (const o of link.dropQueuedFor(p.key)) {
          const m = byHubFrame.get(o.id)
          byHubFrame.delete(o.id)
          if (m?.askUp) ledger.closed(m.askUp)
          refuseFrame(p, m?.ref ?? o.id)
        }
        p.down.endAfterFlush()
      }
    },
    onLost(lost: Outbound[], why: string) {
      // Each of these was reported to its producer's agent as waiting in line and certain to go out.
      // Saying nothing would leave that agent waiting for an answer to a question that has been let
      // go — the failure this whole vocabulary exists to prevent, one process further away.
      note(`the hub link gave up holding ${lost.length} frame(s): ${why}`)
      for (const o of lost) {
        const m = byHubFrame.get(o.id)
        if (!m) continue
        byHubFrame.delete(o.id)
        const p = attached.get(m.key)
        if (!p) {
          note(`a frame was let go for a producer that is no longer here, so nothing could be told: ${o.what}`)
          continue
        }
        refuseFrame(p, m.ref)
      }
    },
    /**
     * Frames the hub took and the connection ended before it answered for them.
     *
     * The door keeps the wire's promise to its producers the way the hub keeps it to the door: one
     * ack per frame, and never a socket ended with frames unanswered. `unseen` is the truth here —
     * the hub may have delivered them and lost the ack with the socket, or destroyed them, and from
     * this side the two are one — and a producer already knows what to do with it: say so to its
     * agent, and never send the frame again. Written BEFORE the link is marked down, because marking
     * it down ends every greeted producer's socket, and an ack written after that is written into a
     * socket that is closing. The ask's routing record is kept: a question that DID land has live
     * buttons, and this relay's instance survives the reconnect, so his tap can still arrive.
     */
    onUnanswered(gone: Unanswered[], why: string) {
      note(`the hub connection ended with ${gone.length} frame(s) unanswered for: ${why}`)
      for (const o of gone) {
        const m = byHubFrame.get(o.id)
        if (!m) continue
        byHubFrame.delete(o.id)
        const p = attached.get(m.key)
        if (!p) {
          note(`nobody knows what became of a frame whose producer is no longer here: ${o.what}`)
          continue
        }
        p.down.write({ t: 'ack', ref: m.ref, delivered: 'unseen' })
      }
    },
  })

  function fromHub(f: Record<string, any>): void {
    switch (f.t) {
      case 'welcome': {
        // The same refuse-rather-than-impersonate check a producer makes, made once here on behalf
        // of all of them. An old hub ignores the unknown `lane` field and admits this worktree AS
        // THE PROJECT — taking the project's claim and its topic while the project's own session is
        // turned away.
        if (OUR_LANE && f.lane !== OUR_LANE) {
          note('the hub did not confirm this worktree; it is older than this relay')
          // The producers are told BEFORE the link is marked down, because marking it down ends
          // every greeted producer's socket — and a refusal written into a socket that is already
          // closing is a producer left waiting for a link that is never coming.
          // `bad_lane` is the closest thing the closed refusal set has to "the hub will not address
          // this worktree", and it is permanent, which is the fact a producer must act on. Widening
          // the set is a change to `hub-proto`, and that is out of this slice.
          for (const p of producers) refuse(p, 'bad_lane')
          link.markDown(
            true,
            `The hub on this machine is older than this relay and cannot give a worktree a place of its own (${OUR_LANE}).`,
          )
          link.end()
          break
        }
        welcomeFrame = f
        // A connection that succeeded is the end of any run of refusals.
        heldSince = null
        // The hub's title for the conversation already names the address when there is one (the
        // registry composes "<project> · <address>", clipped from the left); appending it again
        // printed a lane's name twice, once with an ellipsis, which reads as a bug.
        note(`connected as "${f.project}"`)
        // Held first, then `markUp` — which is what greets the producers, through `onState`.
        // Greeting them here as well would be one more place that has to stay in step with the
        // link's state.
        link.markUp()
        break
      }
      case 'refused': {
        const reason = String(f.reason)
        // The same split the tool server makes: will waiting help? An unknown reason is temporary on
        // purpose — a hub shipped after this build may refuse for something recoverable.
        const forGood = ['unknown_project', 'bad_token', 'not_enabled', 'version_skew', 'bad_lane']
        // A squatting claim is not permanent, so it is waited out — but a run of them that has
        // outlasted anything legitimate is a process that outlived its session, and waiting for
        // ever is the wrong answer then.
        if (reason === 'already_claimed') heldSince ??= Date.now()
        else heldSince = null
        const stuck = heldSince !== null && Date.now() - heldSince >= STUCK_AFTER_MS
        note(`the hub refused this relay (${reason})`)
        const permanent = stuck || forGood.includes(reason)
        const why = stuck
          ? `Another connection has held this conversation across several attempts and is not letting go. If no worker for it is running, a stray process is squatting the claim (${reason}).`
          : `The hub would not take this relay (${reason}).`
        if (permanent && link.isUp) {
          // A refusal on a LIVE link: `not_enabled`, since 5 September, when the project is switched
          // off at a terminal. The hub says the reason first, then finishes answering for what it
          // had already read, and only then closes — so the door does the same. Greeted producers
          // hear the reason NOW and stop promising their agents anything; every ack that still
          // arrives is relayed; and the close the wire promises is what ends them, with whatever is
          // then unanswered said to be `unseen` as at any close. Marking the link down here (the
          // old order, written when a refusal only ever answered a `hello`) ended those sockets
          // before the reason was written into them, so a tool server saw nothing but a dropped
          // link, and its agent read "the hub went away" for a project the operator had turned off.
          // A producer that has not said hello yet is turned away at once, as it would be after.
          for (const p of producers) {
            if (p.greeted) p.down.write({ t: 'refused', reason })
            else refuse(p, reason)
          }
        } else if (permanent) {
          // Marked down FIRST. A permanent refusal lets go of every frame the link was holding, and
          // `onLost` answers each with an `ack` saying no — which has to reach a socket that is still
          // open. Ending the producers first (the old order) wrote that "no" into sockets already
          // closing, so the agent kept reading "waiting in line" for a frame nothing would ever
          // carry. No greeted producer is ended by this: the link is down, so every greeted
          // producer was already ended when it went down.
          link.markDown(true, why)
          for (const p of producers) refuse(p, reason)
        } else {
          // Relayed rather than swallowed: the producer owns the words its agent reads, and it
          // already has a sentence for every one of these reasons. Written BEFORE the link is
          // marked down, because marking it down is what closes the socket this refusal travels on.
          for (const p of producers) refuse(p, reason)
          link.markDown(false, why)
        }
        break
      }
      case 'ack': {
        // A frame this relay sent on its own behalf — a withdrawal for a producer that never came
        // back. Nobody is waiting on it, and saying "no producer is waiting on this" below would be
        // a false alarm in the one log a developer reads to find the real ones.
        if (ourOwn.delete(String(f.ref))) break
        const m = byHubFrame.get(String(f.ref))
        if (!m) {
          // A lookup that comes up empty is a failure, not a silent continue. Nothing can be told
          // what became of this frame, so say so where a developer will see it.
          note(`an ack named ${f.ref}, which no producer is waiting on`)
          break
        }
        byHubFrame.delete(String(f.ref))
        // A question the hub never delivered can never be tapped, so its routing record is dead
        // weight — and keeping it would let a much later ask id collide with a stale entry.
        if (m.askUp && f.delivered === 'no') ledger.closed(m.askUp)
        const p = attached.get(m.key)
        if (!p) {
          note(`the hub answered for a frame whose producer has gone; nothing was told (${f.ref})`)
          break
        }
        p.down.relay({ ...f, ref: m.ref })
        break
      }
      case 'choice': {
        const id = String(f.ask_id)
        const a = ledger.who(id)
        if (!a) {
          note(`a tap arrived for ${id}, which no producer here asked; it was not delivered`)
          break
        }
        // Answered once and for all, at the hub, the moment he tapped — so this routing record has
        // done its job whichever way the next line goes.
        ledger.closed(id)
        const p = attached.get(a.key)
        if (!p) {
          // The lookup SUCCEEDED and still found nobody, which is the shape this used to fail in:
          // the frame went into a closed socket, the write error was swallowed, and no agent
          // anywhere ever learned the answer the operator had already given. It cannot be delivered
          // now — his tap is spent — so the one thing left is to say so where a developer can see it.
          note(`a tap on ${id} arrived after its producer had gone; the answer reached nobody`)
          break
        }
        // To exactly one producer, and named with the id THAT producer minted. Handing it to the
        // wrong one would answer a question a different agent is still waiting on.
        p.down.relay({ ...f, ask_id: a.askId })
        break
      }
      case 'message': {
        // The operator's typed words are not addressed to a particular producer — the hub has no
        // idea there is more than one — so every attached producer gets them and each decides what
        // it can do. The tool server hands them to its agent; the watcher hands them to the session
        // as a prompt, and answers the hub with an `ack` saying whether it could.
        //
        // Over the ATTACHED producers, not every open socket: a producer that has reconnected has an
        // older socket still draining its last frame, and his words would go into the one that is
        // about to close rather than the one its agent is reading.
        //
        // `in_reply_to_ask` names the ask id the HUB knows, which is this door's namespaced one.
        // The producer that asked gets its own id back; every other producer gets no field at
        // all, because an id from another producer's namespace names nothing it ever minted.
        const asked = f.in_reply_to_ask === undefined || f.in_reply_to_ask === null ? undefined : ledger.who(String(f.in_reply_to_ask))
        const { in_reply_to_ask: _theirs, ...plain } = f
        const handedTo = new Set<string>()
        for (const p of attached.values()) {
          if (!p.greeted) continue
          handedTo.add(p.key)
          p.down.relay(asked && asked.key === p.key ? { ...plain, in_reply_to_ask: asked.askId } : plain)
        }
        if (!handedTo.size) {
          // Refused out loud, never dropped in silence. Dropped, the hub goes on believing his
          // words were read; answered `refused`, it puts a line in the topic he typed in. The wire
          // has always had this ack, and until now nothing ever sent it. This is the door's own
          // answer because there is nobody else to give one: an engine's tool server that has not
          // attached yet, or a worker whose engine has gone.
          note('the operator typed something and nothing is attached to take it; the hub was told')
          answerHub(
            { t: 'ack', ref: String(f.id), status: 'refused', reason: 'nothing is attached to the worker yet that can take typed words' },
            'an answer about typed words',
          )
          break
        }
        // Everyone it was handed to has to answer, or go, before the hub hears a refusal — and,
        // when his message carried a file, before it hears a count that is short.
        typedWords.set(String(f.id), {
          waiting: handedTo,
          refusals: [],
          files: Array.isArray(f.files) ? f.files.length : 0,
          handedOn: 0,
          accepted: false,
        })
        while (typedWords.size > TYPED_WORDS_KEPT) {
          const oldest = typedWords.keys().next()
          if (oldest.done) break
          typedWords.delete(oldest.value)
        }
        break
      }
      default:
        break
    }
  }

  /**
   * Give one producer the welcome this relay is holding, so its own queue may drain.
   *
   * `link.isUp` is checked as well as the welcome being held, and the two are not the same fact: the
   * welcome is a frame from a connection that may since have died, and greeting a producer out of a
   * dead one is telling it the operator is reachable when nothing is behind this process at all.
   */
  function greet(p: Producer): void {
    if (!p.checked || p.greeted || !welcomeFrame || !link.isUp) return
    p.greeted = true
    p.down.relay(welcomeFrame)
  }

  /** Turn a producer away, exactly as the hub would: one frame, then the connection closes. */
  function refuse(p: Producer, reason: string): void {
    p.down.write({ t: 'refused', reason })
    p.down.endAfterFlush()
  }

  // ─────────────────────────────────────────────────────────────────────────────────────────────
  // Producers → the hub.

  function fromProducer(p: Producer, line: string): void {
    let f: Record<string, any>
    try {
      f = JSON.parse(line)
    } catch {
      note('a frame from a producer could not be read; ignoring it')
      return
    }
    if (f.t === 'hello') {
      if (p.checked) {
        note('a producer said hello twice; the second was ignored')
        return
      }
      // Defence in depth over the directory's permissions, and it costs nothing because the frame
      // already carries the secret. A producer that cannot prove the same project cannot borrow
      // this relay's claim to reach a conversation it has no secret for.
      if (!secretHeld || String(f.token) !== secretHeld) {
        note('a producer presented a secret this relay does not hold; it was turned away')
        refuse(p, 'bad_token')
        return
      }
      const theirs = f.lane === undefined || f.lane === null ? null : String(f.lane)
      if (theirs !== OUR_LANE) {
        // Fail closed. This relay holds ONE address; a producer naming another is asking to be
        // relayed into a conversation this connection does not hold, and quietly relaying it anyway
        // would put its words in somebody else's topic.
        note(`a producer named ${theirs ?? 'no lane'} and this relay holds ${OUR_LANE ?? 'no lane'}; it was turned away`)
        refuse(p, 'bad_lane')
        return
      }
      // `instance` is a required field of `hello` on this wire, and it is the only thing that says
      // this producer is the same one that asked the questions still open under its name. A producer
      // with no name of its own would be a new voice on every reconnect, so its taps would go to a
      // socket that is already closed — fail closed and say so instead.
      const key = f.instance === undefined || f.instance === null ? '' : String(f.instance)
      if (!key.length) {
        note('a producer said hello without naming which run of itself it is; it was turned away')
        refuse(p, 'version_skew')
        return
      }
      p.checked = true
      p.key = key
      // Reused when this key has been seen before, so a producer that reconnects keeps the namespace
      // its still-open questions were asked under.
      p.n = ledger.numberFor(key)
      if (ledger.attached(key)) note('a producer came back; its open questions stay open')
      const older = attached.get(key)
      attached.set(key, p)
      if (older && older !== p) {
        // One process, two sockets. The newest is the live one — the older is a connection it has
        // already given up on — and letting the older one stay would leave taps going to whichever
        // the map happened to hold.
        note('a producer said hello on a second socket; the first was let go')
        older.down.endAfterFlush()
      }
      greet(p)
      return
    }
    if (!p.checked) {
      note(`a producer sent ${f.t} before saying hello; it was dropped`)
      return
    }
    switch (f.t) {
      case 'pong':
        // Never forwarded. This relay answers the hub's pings itself, so a producer's pong is an
        // answer to a ping nobody sent it.
        return
      case 'bye':
        // One producer leaving is not this lane leaving. Ending the hub connection here would take
        // the whole conversation off the operator's phone because one of two voices restarted.
        note('a producer said goodbye')
        detach(p)
        p.down.endAfterFlush()
        return
      case 'ack': {
        // An answer about his typed words is folded into the one answer the hub will hear; any
        // other ack goes up as the frame it is.
        const about = typedWords.get(String(f.ref))
        if (about) {
          answerForTypedWords(String(f.ref), about, p, f)
          return
        }
        relayUp(p, f)
        return
      }
      case 'say':
      case 'ask':
      case 'done':
      case 'ask_resolved':
      case 'beat':
        relayUp(p, f)
        return
      default:
        // A kind this build does not know is dropped rather than relayed: passing an unread frame to
        // the hub would put this relay's name on something it cannot vouch for.
        note(`a producer sent an unknown frame kind (${f.t}); it was dropped`)
        return
    }
  }

  function relayUp(p: Producer, f: Record<string, any>): void {
    const ref = String(f.id)
    // A share worked out from how many producers are actually here, not a constant tuned for two.
    //
    // A fixed cap is not a reservation: at 28 each, two producers hold 56 of a 64-slot queue and the
    // THIRD is refused permanently before it has said one word — for a backlog that is not its own,
    // which is the exact failure a share exists to prevent. And three is not exotic: opencode starts
    // one MCP child PER DIRECTORY, so a session in the repo root, one in a subfolder and the event
    // watcher are three voices on one relay. The floor is there because a share of one is no voice
    // at all; past sixteen producers the queue is genuinely too small and everyone shares the shortage.
    const share = Math.max(4, Math.floor(link.capacity / Math.max(2, attached.size)))
    const held = link.queuedFor(p.key)
    if (held >= share) {
      note(`producer ${p.n} is holding ${held} of the line; this one was shed rather than crowding out the others`)
      refuseFrame(p, ref, 'too-fast')
      return
    }
    // The envelope is this relay's to mint — `v` and `id` are per connection, and two producers both
    // counting from 1 would make an `ack` ambiguous. Everything else travels untouched.
    const { v: _v, id: _id, ...payload } = f
    let askUp: string | undefined
    if (f.t === 'ask' || f.t === 'ask_resolved') {
      askUp = namespaced(p, String(f.ask_id))
      payload.ask_id = askUp
    }
    const d = link.send(payload, `${f.t} from producer ${p.n}`, undefined, p.key)
    if (!d.delivered && d.permanent) {
      // The link will never carry this one. Telling the producer now is the only chance its agent
      // has to stop waiting, because no `ack` is coming for a frame that was never queued.
      note(`the hub link would not take a ${f.t} from producer ${p.n}: ${d.why}`)
      refuseFrame(p, ref)
      return
    }
    byHubFrame.set(d.id, { key: p.key, ref, ...(askUp && f.t === 'ask' ? { askUp } : {}) })
    // Insertion order is the right eviction here and the wrong one for the ask map: an `ack` the hub
    // has not sent by its 512th frame later is one it is never going to send.
    while (byHubFrame.size > 512) {
      const oldest = byHubFrame.keys().next()
      if (oldest.done) break
      byHubFrame.delete(oldest.value)
    }
    if (!askUp) return
    if (f.t === 'ask') ledger.opened(askUp, p.key, String(f.ask_id))
    // The agent has closed the question itself, so nothing can be routed for it any more. Kept, it
    // would sit in the map until it aged out — and the map is only ever added to otherwise.
    else ledger.closed(askUp)
  }

  /** One producer's answer about his typed words, folded into the one the hub will hear. */
  function answerForTypedWords(ref: string, about: TypedWords, p: Producer, f: Record<string, any>): void {
    // A second answer from one producer, or one from a producer the words were never handed to,
    // says nothing the first did not.
    if (!about.waiting.delete(p.key)) return
    if (f.status === 'accepted') {
      about.accepted = true
      // The count of his files this producer handed on — the one frame this door rebuilds rather
      // than forwards, and so the one place a field the hub reads could be dropped on the way.
      // The hub reads a short count as a worker too old to take files and says so in his topic,
      // which this door must never make a producer look like.
      if (typeof f.files === 'number') about.handedOn = Math.max(about.handedOn, f.files)
      // As soon as it cannot be improved on, and not before. For ordinary typed words that is the
      // first accepted answer, exactly as it always was, so his thumb still arrives at once. For a
      // message carrying a file it is the first producer that took ALL of them — and when the one
      // that answers first took none, waiting for the rest is the only way the number is a fact
      // rather than a race between processes, which is what the refusal path has always done.
      if (about.handedOn >= about.files || !about.waiting.size) acceptTypedWords(ref, about)
      return
    }
    about.refusals.push({ key: p.key, reason: typeof f.reason === 'string' ? f.reason : '' })
    settleTypedWords(ref, about)
  }

  /** Somebody took his words: one accepted answer goes up, with the count nobody can better. */
  function acceptTypedWords(ref: string, about: TypedWords): void {
    typedWords.delete(ref)
    answerHub(
      { t: 'ack', ref, status: 'accepted', ...(about.files > 0 ? { files: about.handedOn } : {}) },
      'an answer about typed words',
    )
  }

  /** Nobody left to answer: one answer goes up, with the reason that matters most if it is no. */
  function settleTypedWords(ref: string, about: TypedWords): void {
    if (about.waiting.size) return
    // Somebody did take them; a later producer's refusal does not unsay that.
    if (about.accepted) {
      acceptTypedWords(ref, about)
      return
    }
    typedWords.delete(ref)
    const said = about.refusals.find(r => r.key === cfg.carrier) ?? about.refusals[0]
    const reason = said?.reason || 'everything attached to the worker went away before taking it'
    answerHub({ t: 'ack', ref, status: 'refused', reason }, 'an answer about typed words')
  }

  function detach(p: Producer): void {
    if (!producers.delete(p)) return
    note(`a producer went away; ${producers.size} left`)
    if (!p.checked || attached.get(p.key) !== p) return
    attached.delete(p.key)
    // Whatever of his typed words it had not answered for, it never will now.
    for (const [ref, about] of typedWords) {
      if (about.waiting.delete(p.key)) settleTypedWords(ref, about)
    }
    ledger.rememberNow()
    // The other half of the same proof the hub makes: its socket is gone, and if nothing comes back
    // under its name, the questions it left cannot be answered by anybody and their buttons come off.
    ledger.withdrawWhatItLeaves(p.key)
  }

  // ─────────────────────────────────────────────────────────────────────────────────────────────
  // The door.

  /**
   * Bind the address for this conversation, refusing to displace a relay that is already live on it.
   *
   * A leftover socket FILE from a process that was killed must be removed or nothing can ever bind
   * again. A socket somebody is still ANSWERING on is a second relay for this address, and two of
   * those hold two hub claims and race — the exact thing this process exists to prevent. The two
   * look identical on disk, so they are told apart by dialling it.
   */
  async function bind(): Promise<void> {
    mkdirSync(dirname(LISTEN), { recursive: true, mode: 0o700 })
    let there = false
    try {
      statSync(LISTEN)
      there = true
    } catch {
      there = false
    }
    if (there) {
      const live = await new Promise<boolean>(resolve => {
        Bun.connect({
          unix: LISTEN,
          socket: { open: s => { s.end(); resolve(true) }, data() {}, close() {}, error() {} },
        }).catch(() => resolve(false))
      })
      if (live) die(`another attach is already holding ${LISTEN}; this one is not needed`)
      unlinkSync(LISTEN)
    }
    Bun.listen({
      unix: LISTEN,
      socket: {
        open(s) {
          // Nameless and unnumbered until its `hello` says which producer this is. A number handed
          // out per SOCKET is a new namespace on every reconnect, and a question asked before one
          // would then be a question nothing could route an answer back to.
          const p: Producer = {
            key: '',
            n: 0,
            s,
            down: new Down(s),
            buf: '',
            checked: false,
            greeted: false,
          }
          ;(s as any).data = p
          producers.add(p)
          note(`a producer attached (${producers.size} now)`)
        },
        data(s, chunk) {
          const p = (s as any).data as Producer | undefined
          if (!p) return
          p.buf += chunk.toString()
          // Read exactly one line at a time, and never to EOF: a reset after a good frame would lose
          // the frame with it.
          for (;;) {
            const nl = p.buf.indexOf('\n')
            if (nl < 0) break
            const line = p.buf.slice(0, nl)
            p.buf = p.buf.slice(nl + 1)
            if (line.trim()) fromProducer(p, line)
          }
          if (p.buf.length > MAX_FRAME_BYTES) {
            note('a producer sent a line past the ceiling; dropping it')
            p.buf = ''
            s.end()
          }
        },
        drain(s) {
          const p = (s as any).data as Producer | undefined
          p?.down.flush()
        },
        close(s) {
          const p = (s as any).data as Producer | undefined
          if (p) detach(p)
        },
        error(_s, e) {
          note(`producer socket error: ${(e as Error)?.message ?? e}`)
        },
      },
    })
    note(`listening on ${LISTEN} for ${FACTS.mainTop ?? PROJECT_DIR}${OUR_LANE ? ` · ${OUR_LANE}` : ''}`)

    // Anything the last run left open belongs to a producer that has not come home yet. The same
    // grace every living producer gets: come back and keep your questions, stay away and their
    // buttons come off rather than sitting on his phone with nobody behind them.
    if (ledger.openCount) {
      note(`${ledger.openCount} question(s) were still open when this attach last stopped`)
      for (const key of ledger.waitingOn()) ledger.withdrawWhatItLeaves(key)
    }
  }

  function goodbye(): void {
    // First, and outside the try below: the questions open at this instant are exactly what the next
    // run has to route taps for, a quarter of a second of debounce is long enough to lose the last
    // one asked, and it must not be skipped because saying goodbye threw. It swallows its own errors.
    ledger.rememberNow()
    try {
      if (link.isUp) link.sendControl({ t: 'bye', reason: 'stopping' })
      // The door goes; what was written down beside it stays. That file is how the relay comes back
      // as the same voice the hub already has open questions under.
      unlinkSync(LISTEN)
    } catch {
      /* going away regardless */
    }
  }

  return {
    bind,
    start: () => link.start(),
    goodbye,
    listenPath: LISTEN,
  }
}
