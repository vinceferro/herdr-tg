/**
 * The watcher — seam ② for opencode, folded into the one command.
 *
 * It watches an opencode server's `/event` stream and turns the two things opencode already knows
 * how to say — "I am asking this, and here are the options" and "I need permission to do this" —
 * into the hub's `ask`, then posts the operator's tap back as a reply. And it carries the other
 * half of a phone: what the operator TYPES at the conversation goes to the session as a prompt,
 * verbatim, the way a Claude session gets it in its own turn (`carry`, below).
 *
 * # It is a PRODUCER at attach's own door, in the same process
 *
 * This used to be a process of its own (`adapters/opencode-bridge/bridge.ts`) that dialled the
 * relay with two variables and not a line of it changing. Collapsing the process boundary changes
 * nothing on the wire: the watcher holds a `HubLink` whose socket is attach's OWN door, and whose
 * `hello` carries the secret, the address and an instance of its own. `relay.ts` greets it, numbers
 * it, namespaces its ask ids, shares the queue with it, and — when the hub link drops — ends its
 * connection exactly as it ends every other producer's. One routing path, one greeting path, one
 * ledger; the machinery that exists for two producers is not duplicated for a third that happens to
 * live in-process. `docs/ATTACHING.md` §13.2 is the argument in full.
 *
 * # Inbound content selects; it never names
 *
 * Every button this watcher mints comes from a list opencode published. A tap comes back as an
 * `option_id` this watcher wrote down, and it is looked up in that record — never used to address
 * anything. The operator picks one of the answers the machine already offered; he cannot name a
 * tool, a file or a command. His typed words are the same rule from the other side: they travel
 * only as the text of a prompt, and WHICH session gets them is the server's answer to a question
 * this watcher asks about the project directory — nothing in the text can pick a session, a path
 * or a URL.
 *
 * # The wire is not written here
 *
 * The queue, the framing, the reconnect and the three-outcome vocabulary are `hub-link.ts`, the
 * same module the door and the tool server use. A fork of it drifted by twelve fixed defects once;
 * a test fails if any adapter starts writing its own again.
 */

import { pathToFileURL } from 'node:url'

import type { Project } from '../../plugins/kickoff-channel/where.ts'
import { readBindingFile, sameDirectory, type SessionBinding } from './plan.ts'
import { HubLink, MAX_FRAME_BYTES, type Delivery, type Outbound, type Unanswered } from '../../plugins/kickoff-channel/hub-link.ts'

/** What the watcher needs, worked out by `main.ts` from the namespace and the door it opened. */
export type WatcherConfig = {
  /** attach's own door — the watcher dials it as a producer, exactly as the old bridge dialled a relay. */
  door: string
  /** The conversation attach holds, echoed in `hello` and checked in `welcome`. */
  address: string | null
  /**
   * This run of the watcher, as its `hello` names it. Minted by `main.ts` rather than here because
   * the door has to know it: of the producers behind the door this is the one that carries the
   * operator's typed words, and the door forwards its answer about them over a tool server's.
   */
  instance: string
  /** The opencode server to watch. Seam ②, and deliberately outside the namespace. */
  opencodeUrl: string
  /**
   * The file whatever starts the engine writes this worker's OWN binding into, or null.
   *
   * With it, the operator's words go to the session it names and to no other, and only that
   * session's questions reach him. Without it, the watcher takes the most recently active root
   * session the server lists for the project directory — a guess, and a wrong one on any wall
   * running more than one session for a directory: measured on this box, a steering room's only
   * root session was an unrestricted coordinator and not the session the room steers at all.
   */
  bindingFile: string | null
  /**
   * Which conversation attach is attached AS, asked afresh — null when nothing on this box says.
   *
   * A thunk and not a value because the operator may enrol or grant while the wall is running, and
   * because the binding's own claim to a conversation is checked on every line, like the rest of it.
   */
  attachedAs: () => string | null
  /**
   * The path the wall was pointed at its secret by, or null when it was not pointed at one.
   *
   * Only ever read to choose the WAY OUT a refusal offers: a wall started this way cannot also be
   * told which conversation it is, so the sentence that sends every other wall to
   * `KICKOFF_HUB_CONVERSATION` would send this one to a start attach refuses outright. Not a thunk
   * because, unlike the secret and the conversation, this is what the wall was STARTED with and
   * nothing on the box can change it under a running process.
   */
  tokenFile: string | null
  /**
   * The oldest binding this run may obey, or null when the wall named none.
   *
   * The floor of the fence, and the only part of it that survives this process being restarted.
   */
  bindingGeneration: number | null
  /**
   * The enrolled project, resolved AFRESH on every attempt — the watcher presents the same secret
   * the door authenticated with, so the door's defence-in-depth token check passes.
   */
  secretOf: () => Project | null
  /** What to say when `secretOf` finds nothing: a verb, never a path. */
  whenNotEnrolled: { why: string; note: string }
  /** The directory the opencode server lists sessions for, and names in a developer's note. */
  projectDir: string
  /** Say something in this process's own transcript. The operator cannot see it; a developer can. */
  note: (msg: string) => void
}

/** One open question, as it was published and as it was drawn. */
type Open = {
  /** `que_…` for a question, `per_…` for a permission request. */
  requestID: string
  sessionID: string
  kind: 'question' | 'permission'
  /**
   * Which family of event published it. opencode carries two, and they take the same body at
   * different URLs — the v2 endpoint is under `/api` and names the session, the v1 one does not.
   * Guessing here posts an answer at a path that 404s, and the question stays open on his phone
   * with the agent still waiting.
   */
  v2: boolean
  /** option_id → the label opencode published. A tap is looked up here and nowhere else. */
  labels: Map<string, string>
  /**
   * The agent the note bound this session to when the keyboard went up, or null when it bound none.
   *
   * Held per QUESTION and not per watcher, because the one place it is used pairs it with the
   * session that ASKED — an older one after a rollover. One value for the whole process was the
   * agent of whichever note was last obeyed, so a reply typed under an old question went into the
   * old session naming the new session's agent: a turn under an agent nothing ever bound to it,
   * which is the failure the binding exists to refuse.
   */
  agent: string | null
}

export function startWatcher(cfg: WatcherConfig): void {
  const { note } = cfg
  const OPENCODE = cfg.opencodeUrl.replace(/\/$/, '')
  const ADDRESS = cfg.address

  /** This run. A new instance invalidates every question the last one left open. */
  const INSTANCE = cfg.instance

  /**
   * The file naming this worker's own session — the whole of the binding, and null when the wall
   * did not give one. Read AFRESH every time it is needed and never cached: a launcher that starts
   * a new session rewrites it, and the next line the operator types must go to the new session
   * with nothing restarted. It is a path, not a session: what is in it may change under this
   * process at any moment, and may not be there at all while the engine is still booting.
   */
  const BINDING_FILE = cfg.bindingFile

  /**
   * The oldest binding this run may ever obey — the wall said it on the command line, and nothing
   * this process reads can lower it.
   *
   * The monotonic rule below is memory, and a restart is what destroys memory: a watcher that comes
   * back after a crash, a redeploy or a systemd restart has never seen the binding it was obeying a
   * second earlier, so a stale launcher's file from before the last rollover is the newest thing it
   * has ever seen and it obeys it. That is precisely the rollback the fence exists to refuse, and
   * only a number carried INTO the process can refuse it. Whatever starts the wall is the same
   * party that numbers the bindings, so it is the party that can say the floor.
   */
  const FLOOR = cfg.bindingGeneration

  /**
   * The generation of the newest note this process has ACTED ON, and the session it named.
   *
   * A launcher that numbers its writings is saying which one is newer. Two walls that both think
   * they own this conversation would otherwise take turns retargeting it — the loser's write is the
   * last one, so the loser wins — and the operator's words would land in whichever session lost the
   * race. Once a numbered note has been obeyed, a note carrying a smaller number, or the same
   * number with a different session in it, is refused rather than obeyed: going backwards cannot be
   * told apart from a stale wall writing over a newer binding, so it fails closed.
   *
   * Two things the fence deliberately does NOT do, each of which bricked a wall for the life of the
   * process while telling the operator about a note he has never been told exists:
   *
   *   - A note carrying NO number is not "older" — unless the wall named a FLOOR, which is a wall
   *     saying its launcher numbers every writing, and an unnumbered note there is one this cannot
   *     place. Where no floor was named the fence only orders two numbered writers against each
   *     other, and a note that names no generation has made no claim to be newer; reading it as
   *     older meant that one numbered note, ever, muted a launcher that had gone back to writing
   *     plain ones.
   *   - The fence closes only behind a note the SERVER confirmed. A launcher that writes the note
   *     a beat before its session is listed sees every line refused, and has no reason to raise the
   *     number for the correction — nothing ever took the first one. Advancing on a note that then
   *     failed every check made that correction permanently unreachable.
   */
  let fencedAt = -1
  let fencedTo: string | null = null

  /**
   * Read the note and apply the fence. The sentences are the operator's, not a developer's.
   *
   * A refusal says WHICH KIND it is. `couldNotFindOut` means the note did not say anything — it is
   * not there yet, it could not be read, it is written in something this cannot make sense of — as
   * against the note saying plainly that this is not the session (the fence, another project,
   * another agent). The two need different handling in two places: a question is kept and offered
   * again when nothing was found out, and a reply under a question this watcher itself drew still
   * reaches the session that asked it. Neither may EVER happen for a note that has moved on, which
   * is the rollback the fence exists to refuse.
   */
  function bindingNow(): { binding: SessionBinding } | { refused: string; couldNotFindOut?: true } {
    const read = readBindingFile(BINDING_FILE!)
    if ('refused' in read) {
      // The half of it only whoever runs the wall can act on. The operator's sentence can name no
      // path, no owner and no mode — he has never been told the file exists — so the actionable
      // words go where the person who can chmod it looks.
      if (read.why) note(`the note naming the worker's session was not read: ${read.why}`)
      return { refused: read.refused, couldNotFindOut: true }
    }
    const { conversation, generation, sessionID } = read.binding
    // WHOSE note this is, before anything else it says is weighed.
    //
    // A launcher that points this wall at a sibling room's session writes a note that is right in
    // every other particular: the id resolves, the directory is a room tree that may well match,
    // the agent is the one this worker expects. The conversation is the only thing that tells the
    // two apart, and steering another room's worker with this operator's words — while telling him
    // they were delivered — is the failure this whole binding exists to refuse.
    //
    // Refused, never ignored, when this side cannot say which conversation it is: a claim nobody
    // can check is not a check, and a wall started so that it cannot be checked is a wall to start
    // differently. The half only whoever runs it can act on goes to the journal, as ever.
    if (conversation !== null) {
      const here = cfg.attachedAs()
      if (here === null) {
        // The way out has to be one this wall could actually take. A wall pointed at its secret BY
        // PATH cannot also be told which conversation it is — attach refuses the two variables
        // together — so sending that one to KICKOFF_HUB_CONVERSATION was advice this same program
        // refuses to start on, and the only advice it could act on is the other half: the note must
        // not make a claim the wall it was written for has no way to check.
        note(
          cfg.tokenFile !== null
            ? 'the note names the conversation the worker\'s session belongs to, and this wall was pointed at ' +
              'its secret by path (KICKOFF_HUB_TOKEN_FILE), which does not say which conversation that secret ' +
              'is for; start the wall with KICKOFF_HUB_CONVERSATION in place of the path, or whatever writes ' +
              'the note must leave the conversation out'
            : 'the note names the conversation the worker\'s session belongs to, and nothing told this ' +
              'wall which conversation it is; start it with KICKOFF_HUB_CONVERSATION so the two can be ' +
              'compared, or whatever writes the note must leave the conversation out',
        )
        // Nothing was FOUND OUT — as against the note saying plainly that this is not the session.
        // Both halves of the way out land while the wall is running: the operator grants, or the
        // launcher rewrites the note, which is why this is re-asked on every line rather than
        // decided once at boot. Deciding it here spent an agent's whole turn on a keyboard that was
        // never drawn, and told the operator so once for however many questions were lost.
        return { refused: CANNOT_TELL_WHOSE_CONVERSATION_IT_IS, couldNotFindOut: true }
      }
      if (conversation !== here) {
        note(
          `the note is written for conversation ${conversation}, and this wall speaks for ${here}; ` +
            'whatever writes the note must write it for the conversation the worker was started for',
        )
        return { refused: BELONGS_TO_ANOTHER_CONVERSATION }
      }
    }
    // The floor next, because it is the only half of the fence a restart cannot forget. A binding
    // under it is refused for the life of this process however many times it is rewritten.
    if (FLOOR !== null) {
      if (generation === null) {
        note(
          `this watcher was started for binding ${FLOOR}, and the note names no generation at all; ` +
            'a wall started with a number must have every writing of the note numbered',
        )
        return { refused: DOES_NOT_SAY_HOW_NEW_IT_IS }
      }
      if (generation < FLOOR) {
        note(
          `the note names a session at generation ${generation}, and this watcher was started for ${FLOOR}; ` +
            'a binding older than the one the wall was started for is never obeyed',
        )
        return { refused: OLDER_THAN_THE_ONE_IN_USE }
      }
    }
    if (fencedAt >= 0 && generation !== null) {
      const olderNumber = generation < fencedAt
      const sameNumberOtherSession = generation === fencedAt && sessionID !== fencedTo
      if (olderNumber || sameNumberOtherSession) {
        // The one sentence he can be given is about a note he does not know exists, and the fix is
        // in a file only whoever wrote the launcher can touch. So the actionable half is said
        // here, where that person looks, rather than left implied on his phone.
        note(
          `the note names a session at generation ${generation}, and ${fencedAt} has already been acted on; ` +
            'whatever writes the note must raise the number on every rewrite',
        )
        return { refused: OLDER_THAN_THE_ONE_IN_USE }
      }
    }
    return read
  }

  /**
   * Close the fence behind a binding that passed every check, including the server's.
   *
   * Called at the one place a note is actually obeyed, so a note that was read and refused leaves
   * the fence exactly where it was.
   */
  function fenceBehind(b: SessionBinding): void {
    if (b.generation === null) return
    fencedAt = b.generation
    fencedTo = b.sessionID
  }

  /** The session this conversation is bound to right now, or null when the note cannot say. */
  function boundSessionNow(): string | null {
    const b = bindingNow()
    return 'refused' in b ? null : b.binding.sessionID
  }

  /**
   * Reasons a question could not be shown that he has already been told about.
   *
   * One line, not one per question: a note that cannot be read stays unreadable for as long as it
   * takes somebody to mend it, and a wall that asks a question a second every second would
   * otherwise spend the project's whole send ceiling saying the same sentence. Cleared the moment a
   * question does get through, so the next spell of not knowing is said again.
   */
  const alreadySaid = new Set<string>()
  function tellHimOnce(key: string, text: string): void {
    if (alreadySaid.has(key)) return
    alreadySaid.add(key)
    say({ t: 'say', text, hint: 'prose' }, 'a word about a question that could not be shown')
  }

  /**
   * Whether a question opencode published is THIS conversation's to ask.
   *
   * One server can be running sessions for several walls, and a question from a session this
   * conversation is not bound to is another worker's: relaying it would put another worker's
   * keyboard on the operator's phone under this project's name, and his tap would then answer into
   * a session nobody bound. Fails closed — a note that cannot be read relays nothing — because a
   * question shown under the wrong name is worse than a question that waits.
   *
   * The SAME question the typed-words path asks, against the server, and for the same reason: two
   * rules meant the note could be good enough to draw a session's keyboard on his phone and post
   * his tap into it, while the very next line he typed at that session was refused. A note naming
   * another project's directory, or a session running an agent it should not be, is exactly how a
   * launcher reaches that by accident.
   *
   * And nothing here is ever dropped in silence. A question refused for a reason ABOUT THE NOTE is
   * a question nobody has been shown while an agent waits on it for ever, so it is said once in the
   * topic; a question from a session that simply is not this one belongs to another wall's
   * conversation and is not his business.
   */
  /**
   * Questions the machine could not decide on, kept to be offered again — the event as it came,
   * against the moment this stops waiting for it.
   *
   * A question is dropped when the NOTE says it is not this conversation's. It must not be dropped
   * when nothing was found out: a server that stalled once, a server that answered 500, a note the
   * launcher has not written yet because the engine booted a beat before it. Dropped there, the
   * agent that asked waits on a keyboard that never appears, no record exists to retire it, and
   * nothing tries again when the server is well a second later. Before the note existed a question
   * was drawn with no round trip at all, so that failure is new and it is this adapter's own worst
   * shape — a dead keyboard — with the keyboard missing instead of stale.
   *
   * Bounded three ways, because a wall whose server never comes back must not grow a queue: how
   * many are kept, how long each is kept for, and — the one that matters most — ONE is offered
   * again per pass. Every question here costs a request with a ten-second deadline, and the events
   * are handled strictly one after another, so offering eight of them at once against a stalled
   * server would park every live event behind a minute and a half of timeouts.
   */
  const keptQuestions = new Map<Record<string, any>, number>()
  const KEEP_A_QUESTION_FOR_MS = 60_000
  const MOST_QUESTIONS_KEPT = 8
  /** Long enough that a stalled server's ten-second deadline is not the whole of every window. */
  const TRY_A_KEPT_QUESTION_AGAIN_MS = 5_000
  /** One went through, so the server is answering: the rest need not wait out the slow interval. */
  const OFFER_THE_NEXT_ONE_AFTER_MS = 250
  let tryingAgain: ReturnType<typeof setTimeout> | null = null

  /** Keep this question to be offered again, or false when it has waited as long as it may. */
  function keepForLater(ev: Record<string, any>): boolean {
    const until = keptQuestions.get(ev)
    if (until !== undefined) {
      if (Date.now() < until) {
        offerAKeptQuestionAgainIn(TRY_A_KEPT_QUESTION_AGAIN_MS)
        return true
      }
      keptQuestions.delete(ev)
      return false
    }
    // Full: this one is given up on now rather than pushing an older one out. The oldest has been
    // waited on longest and is closest to being answered or given up on either way.
    if (keptQuestions.size >= MOST_QUESTIONS_KEPT) return false
    keptQuestions.set(ev, Date.now() + KEEP_A_QUESTION_FOR_MS)
    note('a question could not be decided on yet; it is being kept and will be offered again')
    offerAKeptQuestionAgainIn(TRY_A_KEPT_QUESTION_AGAIN_MS)
    return true
  }

  /** Put the oldest kept question back through the ordinary path, one at a time, later. */
  function offerAKeptQuestionAgainIn(ms: number): void {
    if (tryingAgain !== null || keptQuestions.size === 0) return
    tryingAgain = setTimeout(() => {
      tryingAgain = null
      const next = keptQuestions.keys().next()
      if (next.done) return
      const ev = next.value
      onOpencodeEvent(ev)
      // Only once it has been decided on, so two passes can never be in flight at once. Caught,
      // because this chain is the one every later event queues behind: a rejection left on it
      // would be the last event this watcher ever handled.
      handledInOrder = handledInOrder
        .then(() =>
          offerAKeptQuestionAgainIn(keptQuestions.has(ev) ? TRY_A_KEPT_QUESTION_AGAIN_MS : OFFER_THE_NEXT_ONE_AFTER_MS),
        )
        .catch(e => note(`a kept question could not be offered again: ${(e as Error)?.message ?? e}`))
    }, ms)
    // Never a reason for this process to stay alive on its own.
    ;(tryingAgain as any)?.unref?.()
  }

  /** A question answered somewhere else while it waited is no longer one to offer. */
  function stopKeeping(requestID: string): void {
    if (!requestID) return
    for (const ev of [...keptQuestions.keys()]) {
      const d = ev?.properties ?? ev?.data ?? {}
      if (String(d.id ?? '') === requestID) keptQuestions.delete(ev)
    }
  }

  async function fromTheBoundSession(
    sessionID: string,
    ev: Record<string, any>,
  ): Promise<{ agent: string | null } | null> {
    // No note, so nothing here has ever been told which agent this worker is, and a question drawn
    // without one carries none.
    if (!BINDING_FILE) return { agent: null }
    if (!sessionID) {
      // An older event shape that names no session cannot be matched against the note at all. It is
      // still not shown — nothing can prove it is this conversation's — but the agent that asked is
      // blocked on it, and the journal is not somewhere he looks.
      note('an event that named no session was not passed on: nothing about it can be matched against the note')
      tellHimOnce(
        'named no session',
        'The worker asked something that did not say which of its sessions it came from, so it is not being shown here.',
      )
      return null
    }
    const bound = await theSessionTheNoteNames(null)
    if ('refused' in bound) {
      // Nothing was found out, so nothing has been decided: kept, and offered again. He is told
      // only once this has stopped waiting, because a sentence about a question that then appears
      // a second later is a sentence he can do nothing with.
      if (bound.couldNotFindOut && keepForLater(ev)) return null
      keptQuestions.delete(ev)
      note(`a question from ${sessionID} was not passed on: ${bound.refused}`)
      tellHimOnce(bound.refused, `The worker asked something and it cannot be shown here — ${bound.refused}.`)
      return null
    }
    keptQuestions.delete(ev)
    if (bound.sessionID !== sessionID) {
      note(`a question from ${sessionID} was not passed on: this conversation is bound to ${bound.sessionID}`)
      return null
    }
    // And the same rule on the way back, or it is a rule in one direction only. The note's agent is
    // proved against the SESSION above; a turn inside that session can still run under another
    // agent, because the prompt endpoint resolves one when the body names none and a tree's own
    // `opencode.json` can name the org coordinator as its default. A turn that ran under somebody
    // else's agent is somebody else's turn, and its question belongs on no phone under this
    // project's name — so it is withheld, exactly as a question from another session is.
    if (bound.agent !== null) {
      const running = await theAgentAnswering(sessionID, ev?.properties ?? ev?.data ?? {})
      if (running !== null && running !== bound.agent) {
        note(`a question from ${sessionID} was not passed on: the turn ran under ${running}, not ${bound.agent}`)
        // Withheld is not the same as handled. Nothing else answers this request, so the agent that
        // asked would block on a keyboard nobody will ever draw — for ever, on a wall whose tree
        // names another agent by default, which is the configuration the fence was written for.
        // Turning it down is this watcher's to do here and nowhere else: the session was already
        // proved to be this conversation's own, and a question from somebody else's session is
        // left alone precisely because it is not ours to answer.
        await turnDown(sessionID, ev)
        tellHimOnce(
          A_DIFFERENT_AGENT_IS_ANSWERING,
          `The worker asked something and it cannot be shown here — ${A_DIFFERENT_AGENT_IS_ANSWERING}. ` +
            'It has been turned down, so the worker is not left waiting on it.',
        )
        return null
      }
    }
    alreadySaid.clear()
    return { agent: bound.agent }
  }

  /**
   * Tell opencode nobody is going to answer this request, so the turn that asked can move on.
   *
   * Both shapes, measured against a real opencode 1.18.25 on 7 September: a question has its own
   * `reject` endpoint and takes no body at all, and a permission is rejected through the reply it
   * already has, with the one word opencode's own closed set uses. Best effort — a server that
   * will not take it leaves the worker exactly where withholding alone would have left it, and
   * there is nothing better to do about that than say so where a developer looks.
   */
  async function turnDown(sessionID: string, ev: Record<string, any>): Promise<void> {
    const data = ev?.properties ?? ev?.data ?? {}
    const requestID = String(data.id ?? '')
    if (!requestID) return
    const type = String(ev?.type ?? '')
    const kind: Open['kind'] = type.startsWith('question') ? 'question' : 'permission'
    const v2 = type.includes('.v2.')
    const sid = encodeURIComponent(sessionID)
    const rid = encodeURIComponent(requestID)
    const url =
      kind === 'question'
        ? v2
          ? `${OPENCODE}/api/session/${sid}/question/${rid}/reject`
          : `${OPENCODE}/question/${rid}/reject`
        : replyUrl({ kind, v2, sessionID, requestID })
    try {
      const r = await fetch(url, {
        method: 'POST',
        ...(kind === 'question'
          ? {}
          : { headers: { 'content-type': 'application/json' }, body: JSON.stringify({ reply: 'reject' }) }),
        signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS),
      })
      if (!r.ok) note(`opencode would not take the refusal of ${requestID} (${r.status})`)
      else note(`${requestID} was turned down so the worker is not left waiting on it`)
    } catch (e) {
      note(`could not tell opencode nobody will answer ${requestID}: ${(e as Error)?.message ?? e}`)
    }
  }

  /**
   * The agent a turn ran under, or null when this server will not say.
   *
   * The asked events carry no agent — measured against the 1.18.25 OpenAPI at `/doc`, where
   * `QuestionV2Asked.data` and `PermissionV2Asked.data` have no such field — but each names the
   * message the tool call belongs to, and `AssistantMessage` carries `agent`. So the agent of a
   * turn is one request away, on the same deadline every request here shares.
   *
   * Null on anything but a plain answer, and null is NOT a refusal: a build that names no message,
   * a server that will not serve it, a message with no agent on it. Withholding on a fact nobody
   * could observe would silence every question a server like that ever asks, while the note's own
   * session check still stands. What is withheld is a turn OBSERVED to be somebody else's.
   */
  async function theAgentAnswering(sessionID: string, data: Record<string, any>): Promise<string | null> {
    const named = data?.tool?.messageID ?? data?.source?.messageID
    if (typeof named !== 'string' || named.length === 0) return null
    try {
      const r = await fetch(
        `${OPENCODE}/session/${encodeURIComponent(sessionID)}/message/${encodeURIComponent(named)}`,
        { signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS) },
      )
      if (!r.ok) {
        note(`opencode would not say which agent is answering (${r.status})`)
        return null
      }
      const m: any = await r.json()
      const running = m?.info?.agent
      return typeof running === 'string' && running.length > 0 ? running : null
    } catch (e) {
      note(`could not ask opencode which agent is answering: ${(e as Error)?.message ?? e}`)
      return null
    }
  }

  /**
   * How long one request to the server may take before it is given up on.
   *
   * Bun's `fetch` waits for ever by default, and typed lines are carried one after another so
   * their order is kept — so one request the server accepted and never answered parked that line
   * AND every line typed after it, none of them acked: the hub went on believing each was read,
   * and the operator went on typing at a wall that took none of it. Measured: two messages,
   * twelve seconds, no ack for either. The measured round trip is 4–24 ms; a server that has not
   * answered in ten seconds is not going to.
   */
  const OPENCODE_ANSWERS_WITHIN_MS = 10_000

  const open = new Map<string, Open>()

  /**
   * Sessions his typed words were handed to and that have not finished a turn since.
   *
   * 204 from `prompt_async` means opencode wrote the words down; the agent has not run. When it
   * then cannot — the gateway down, a provider key expired, the context overflowed — opencode says
   * so as `session.error`, and until this existed nothing here listened: the hub had posted
   * nothing, and the only record was a stack trace in a stream nobody watches. Cleared by the
   * session's next `session.idle`, which is a turn that ran. Bounded, oldest first, for a server
   * that never goes idle.
   */
  const prompted = new Map<string, number>()
  const MAX_PROMPTED = 100
  function rememberPrompted(sessionID: string): void {
    prompted.delete(sessionID)
    prompted.set(sessionID, Date.now())
    while (prompted.size > MAX_PROMPTED) {
      const oldest = prompted.keys().next()
      if (oldest.done) break
      prompted.delete(oldest.value)
    }
  }

  /**
   * Bounded, because a server that never resolves its questions would otherwise make this a leak.
   * The oldest goes first: a question nobody answered in the last hundred is not being waited on.
   */
  const MAX_OPEN = 100
  function remember(askId: string, o: Open): void {
    open.set(askId, o)
    while (open.size > MAX_OPEN) {
      const oldest = open.keys().next()
      if (oldest.done) break
      open.delete(oldest.value)
    }
  }

  /**
   * Forget a question nothing can ever answer, and say so where a developer will see it.
   *
   * A record kept for a question that never reached a phone is worse than no record: it stays
   * answerable HERE for the next hundred questions, while no keyboard for it has ever existed. The
   * agent is meanwhile blocked, and there is nobody this watcher can tell — opencode has no channel
   * back into a turn — so the loudest thing available is this line.
   */
  function giveUpOn(askId: string, why: string): void {
    if (!open.delete(askId)) return
    note(`nothing can answer ${askId} any more: ${why}`)
  }

  const link = new HubLink({
    framePrefix: 'f',
    note,
    // The watcher dials attach's own door, which is up whenever this process is running. So the
    // sentence that names a missing relay can only be briefly true, between the door closing on a
    // hub outage and the watcher's redial finding it again.
    whenUnreachable: 'The relay that carries this project to his phone is not running.',
    identify() {
      const project = cfg.secretOf()
      if (!project) {
        // Retried: the documented recovery from "not enrolled" is to open or enrol the project
        // while the adapter is running. A verb, never a path: `attach.ts` says why.
        return { refuse: { permanent: true, ...cfg.whenNotEnrolled, retryMs: 30_000 } }
      }
      return {
        socket: cfg.door,
        hello: {
          t: 'hello',
          project_id: 'unknown-until-the-hub-says',
          token: project.token,
          instance: INSTANCE,
          repo: project.repo,
          pid: process.pid,
          // Every tap this watcher is handed comes back answered, once opencode has spoken. The
          // door folds these into the one answer the hub hears, and answers nothing at all for a
          // producer that promised nothing — so a watcher that stopped saying this would have its
          // taps read as unconfirmed however well they went.
          confirms: ['choice'],
          // Omitted entirely when there is no address. Never `"lane": null`.
          ...(ADDRESS ? { lane: ADDRESS } : {}),
        },
      }
    },
    onFrame: fromHub,
    onLost,
    onUnanswered,
  })

  /**
   * Frames the link let go because nothing but a person can mend the gap.
   *
   * Every one of them was a question or a permission prompt an agent is still blocked on.
   */
  function onLost(lost: Outbound[], why: string): void {
    for (const o of lost) if (o.askId) giveUpOn(o.askId, why)
    note(`${lost.length} frame(s) will never go out: ${why}`)
  }

  /**
   * Frames the door took and the connection ended before it answered for them.
   *
   * The same fact the hub's own `unseen` carries, learned one hop earlier, and handled the same way:
   * a question nobody can confirm was asked is given up on here, never asked again — a second copy
   * would leave two live keyboards for one answer.
   */
  function onUnanswered(gone: Unanswered[], why: string): void {
    for (const o of gone) if (o.askId) giveUpOn(o.askId, why)
    note(`${gone.length} frame(s) went out and nobody knows what became of them: ${why}`)
  }

  /** Send, and act on what actually happened to it. */
  function say(payload: Record<string, unknown>, what: string, askId?: string): Delivery {
    const d = link.send(payload, what, askId)
    if (!d.delivered && d.permanent) {
      note(`not sent (${what}): ${d.why}`)
      if (askId) giveUpOn(askId, d.why)
    }
    return d
  }

  function fromHub(f: Record<string, any>): void {
    switch (f.t) {
      case 'welcome': {
        // A watcher that named an address and was not given it back is talking to something older
        // than itself, and it must NOT go up. Against attach's own door this cannot happen — the
        // door always echoes the lane it holds — but the check is kept as the same defence the tool
        // server keeps, so the watcher never impersonates a project.
        if (ADDRESS && f.lane !== ADDRESS) {
          link.markDown(
            true,
            `The door did not confirm ${ADDRESS} — it is holding a different conversation.`,
          )
          note(`the door did not confirm ${ADDRESS}`)
          link.end()
          return
        }
        note(`the watcher is attached${ADDRESS ? ` · ${ADDRESS}` : ''}`)
        link.markUp()
        return
      }
      case 'refused': {
        const why = String(f.reason)
        const forGood: Record<string, string> = {
          unknown_project: `the hub does not know ${cfg.projectDir}`,
          bad_token: `the secret for ${cfg.projectDir} is not one the hub knows`,
          not_enabled: 'this project is enrolled but switched off',
          version_skew: 'the watcher and the door do not speak the same version',
          bad_lane: `the door will not address ${ADDRESS ?? 'this conversation'}`,
        }
        const forNow: Record<string, string> = {
          // Temporary, always. The bridge this came from counted three of these and gave up for
          // good — on its own asks, the prompts an agent is blocked on. Here every `already_claimed`
          // is the DOOR relaying the hub's refusal, at the hub link's own cadence, so three of them
          // is three seconds: shorter than the ten a predecessor attach gets to stop. The door
          // decides when a claim is stuck (`relay.ts`, by elapsed time), and when it does, it
          // answers each of this watcher's queued frames with an `ack` saying no — which the `ack`
          // branch below turns into giving up on that one question. A second rule here would only
          // ever be the wrong one.
          already_claimed: 'another voice already holds this conversation at the door; waiting for it to go',
          frame_too_large: 'a frame was refused for being too large',
        }
        const said = forGood[why] ?? forNow[why]
        link.markDown(why in forGood, said ?? `refused for a reason this watcher does not know (${why})`)
        note(said ?? `refused: ${why}`)
        return
      }
      case 'ack': {
        // `unseen` is NOT success: it means the send went out and could not be confirmed, and it is
        // never retried, because Telegram has no idempotency key and a second copy of a question
        // would leave two live keyboards for it.
        const was = link.frameInFlight(String(f.ref))
        link.forgetInFlight(String(f.ref))
        if (f.delivered === 'yes') {
          if (f.why === 'clamped' && was) note(`${was.what} arrived on his phone clipped short`)
          return
        }
        const why = f.delivered === 'no' ? String(f.why ?? 'no reason given') : 'the hub could not confirm it arrived'
        note(`the hub did not deliver ${was?.what ?? 'a frame'} (${why})`)
        if (was?.askId) giveUpOn(was.askId, why)
        return
      }
      case 'choice':
        // The hub's own id for the tap goes with it: the answer this watcher owes for it names
        // that id, and the hub edits the receipt it has already put under his thumb with what it
        // hears back.
        void answer(String(f.ask_id), String(f.option_id), String(f.id))
        return
      case 'message':
        // The operator typed at this conversation. Carried one at a time, in the order the lines
        // arrived: two lines typed a second apart are one thought, and two lookups racing could
        // put the second in front of the agent before the first.
        carrying = carrying
          .then(() => carry(f))
          .catch(e => {
            // The line must never be poisoned: a rejected promise here would skip every later
            // `carry`, and every line typed after it would be neither posted nor acked — the
            // silence this whole path exists to end. `carry` catches its own failures; this is
            // for the one it cannot foresee, and the hub is still answered.
            note(`could not carry the operator's words: ${(e as Error)?.message ?? e}`)
            answerFor(String(f.id), 'the worker could not take it')
          })
        return
      default:
        // Unknown kind ignored, so something shipped after this cannot kill the link by being newer.
        // `ping` never arrives here — `hub-link.ts` answers it — so liveness never waits on this switch.
        return
    }
  }

  // ─────────────────────────────────────────────────────────────────────────────────────────────
  // Typed steering.

  /** The line of typed messages, carried one after another — see `case 'message'`. */
  let carrying: Promise<void> = Promise.resolve()

  /**
   * The operator typed at this conversation, and his words become a PROMPT to the session —
   * verbatim, the way a Claude session gets them in its own turn. Nothing is put in front of them
   * and nothing in them is read: the session is the machine's answer (`sessionForTypedWords`), the
   * URL is built from that answer and the `--opencode` flag, and the words travel only as the text
   * of the body. `from` is not shown to the agent, exactly as the Claude adapter does not show it.
   *
   * The endpoint and the body were captured from opencode 1.18.25 on 5 September, not guessed —
   * the last time this adapter guessed an opencode shape every test agreed with the guess.
   * `POST /session/{id}/prompt_async` with `{parts: [{type: 'text', text}]}` answers 204 at once
   * and runs the agent with the session's own model and agent. The v2 `/api/session/{id}/prompt`,
   * the one a reader of the spec reaches for first, admitted the prompt, emitted two events, and
   * ran nothing. A prompt to a session blocked on its own question is taken (204), written down,
   * and run once the question is answered.
   *
   * Every `message` is answered on the wire with `ack{ref, status, reason?}`. `refused` carries a
   * reason in the operator's own register, because the hub puts it in the topic he typed in — the
   * one place he can learn that a line he wrote reached nobody. Until this existed his words
   * reached a line on stderr saying it was not built, and nothing at all reached him.
   */
  async function carry(f: Record<string, any>): Promise<void> {
    const ref = String(f.id)
    if (typeof f.text !== 'string') {
      answerFor(ref, 'the message arrived without any words in it')
      return
    }
    const replyTo = f.in_reply_to_ask === undefined || f.in_reply_to_ask === null ? null : String(f.in_reply_to_ask)
    const target = await sessionForTypedWords(replyTo)
    if ('refused' in target) {
      note(`the operator's words were not handed on: ${target.refused}`)
      answerFor(ref, target.refused)
      return
    }
    const sid = encodeURIComponent(target.sessionID)
    // His words, then one file part per file the hub fetched, then — inside the words — one line
    // per file that did not come through. A file with no caption is a prompt with no text part.
    const about = filesOn(f)
    const text = [f.text, ...about.lines].filter(s => s.length > 0).join('\n')
    const parts: Record<string, unknown>[] = [...(text.length > 0 ? [{ type: 'text', text }] : []), ...about.parts]
    if (parts.length === 0) {
      answerFor(ref, 'the message arrived without any words in it')
      return
    }
    // The agent the note binds has to be one this server can actually resolve, and that is asked
    // BEFORE his words go anywhere. Measured on 1.18.25 on 7 September: a prompt naming an agent
    // the server does not know is answered 204 and writes no message at all — his words are gone —
    // so the ack below would have said they were taken and his line would have carried the thumb,
    // with the truth arriving seconds later as a `session.error`. One of the two was always false.
    //
    // The session-level check cannot see this: a session goes on naming an agent that has been
    // renamed or dropped from the tree's own configuration while it lives.
    if (target.agent !== null) {
      const known = await thisServerResolves(target.agent)
      if (known === false) {
        note(`opencode does not resolve the agent the note binds (${target.agent})`)
        answerFor(ref, THE_SERVER_DOES_NOT_KNOW_THE_AGENT)
        return
      }
    }
    try {
      const r = await fetch(`${OPENCODE}/session/${sid}/prompt_async`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        // The agent the note binds, NAMED, when it names one. `agent` is an optional top-level
        // string on this endpoint, read off the running 1.18.25 server's own OpenAPI at `/doc`
        // rather than guessed. Without it the server resolves an agent of its own — and the room
        // tree measured on 6 September ships an `opencode.json` whose default is the org
        // coordinator, so a session bound to the room's agent ran its turn under the coordinator
        // and the note's check had proved nothing about the turn his words actually reached.
        body: JSON.stringify({ parts, ...(target.agent !== null ? { agent: target.agent } : {}) }),
        signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS),
      })
      if (!r.ok) {
        // The status is for a developer, here. The reason goes verbatim into his topic, where a
        // number is jargon; and it is "it", because the hub's sentence is about "what you typed".
        note(`opencode would not take the operator's words (${r.status})`)
        answerFor(ref, "the worker's server would not take it")
        return
      }
      note(`the operator's words went to ${target.how} as a prompt`)
      rememberPrompted(target.sessionID)
      answerFor(ref, undefined, about.count)
    } catch (e) {
      note(`could not reach opencode with the operator's words: ${(e as Error)?.message ?? e}`)
      answerFor(ref, unreached(e))
    }
  }

  /**
   * The agent names this server can resolve, as it last answered — `GET /agent`, measured on
   * 1.18.25: 200 and a JSON array of `{name, mode, native, …}`, the whole server's set rather than
   * one session's.
   *
   * Asked once for a run and remembered, so the ordinary typed line costs nothing: the objection to
   * a pre-flight was a request per line, and the answer is not per line. A name that is NOT in the
   * remembered set is asked about again before anything is refused on it — a set from ten minutes
   * ago is not evidence that an agent added since does not exist — so the extra request happens
   * only while the name really is unknown.
   *
   * `null` means this side could not find out: an older server with no such route, or one that
   * would not answer. Nothing is refused on that. Withholding his words on a fact nobody could
   * observe would silence every line typed at such a server, and the failure it guards against is
   * still caught after the fact by `session.error`.
   */
  let agentsKnown: Set<string> | null = null
  async function thisServerResolves(name: string): Promise<boolean | null> {
    if (agentsKnown?.has(name)) return true
    const fresh = await agentsThisServerKnows()
    if (fresh === null) return agentsKnown === null ? null : agentsKnown.has(name)
    agentsKnown = fresh
    return fresh.has(name)
  }

  async function agentsThisServerKnows(): Promise<Set<string> | null> {
    try {
      const r = await fetch(`${OPENCODE}/agent`, { signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS) })
      if (!r.ok) {
        note(`opencode would not say which agents it knows (${r.status})`)
        return null
      }
      const list = await r.json()
      if (!Array.isArray(list)) {
        note('opencode answered something other than a list of agents')
        return null
      }
      return new Set(
        list.filter((a: any) => a && typeof a.name === 'string' && a.name.length > 0).map((a: any) => String(a.name)),
      )
    } catch (e) {
      note(`could not ask opencode which agents it knows: ${(e as Error)?.message ?? e}`)
      return null
    }
  }

  /** What he is told when a request to the server ended without an answer. */
  function unreached(e: unknown): string {
    return (e as Error)?.name === 'TimeoutError'
      ? "the worker's server did not answer in time"
      : "the worker's server could not be reached"
  }

  /**
   * Tell the hub what became of one `message`, on the wire. The hub acks this like any frame.
   *
   * `files` is how many entries of the message's `files` went into the prompt, sent only when the
   * message carried any: the hub reads its absence as "an adapter older than files took the words
   * and dropped the picture", and an ack about words alone stays byte for byte what it always was.
   */
  function answerFor(ref: string, refused?: string, files?: number): void {
    say(
      refused
        ? { t: 'ack', ref, status: 'refused', reason: refused }
        : { t: 'ack', ref, status: 'accepted', ...(files !== undefined ? { files } : {}) },
      'an answer about typed words',
    )
  }

  /**
   * The file parts for one of his messages, and the lines for the files that did not come.
   *
   * `FilePartInput` as captured off `/doc` of opencode 1.18.25, not guessed: `type`, `mime` and
   * `url` required, `filename` optional, nothing else allowed (`additionalProperties: false`, so a
   * stray key is a refused prompt). The URL is the hub's own path as a `file:` URL; the SERVER reads
   * it from disk and hands the model a data URL, so the same-path mount rule binds the server
   * process and nothing here reads a byte. The name his phone reported goes in `filename`, as
   * data, and is never part of the URL. A mime the frame does not carry is sent as the type that
   * declares nothing.
   */
  function filesOn(f: Record<string, any>): { parts: Record<string, unknown>[]; lines: string[]; count?: number } {
    if (!Array.isArray(f.files)) return { parts: [], lines: [] }
    const parts: Record<string, unknown>[] = []
    const lines: string[] = []
    for (const file of f.files as any[]) {
      const kind = typeof file?.kind === 'string' ? file.kind : 'file'
      const filename = typeof file?.filename === 'string' ? file.filename : undefined
      if (typeof file?.path === 'string') {
        parts.push({
          type: 'file',
          mime: typeof file.mime === 'string' ? file.mime : 'application/octet-stream',
          url: pathToFileURL(file.path).href,
          ...(filename !== undefined ? { filename } : {}),
        })
        continue
      }
      // The last arm is not decoration: this word travels DOWN from a hub that may be newer than
      // this watcher, and a reason it has never heard of has to read as one it cannot explain
      // rather than as nothing at all.
      const why =
        file?.why === 'too-big'
          ? 'it is larger than the 20 MB the bot may fetch from Telegram, and it will not be fetched later'
          : file?.why === 'download-failed'
            ? 'the download from Telegram failed, and he is being asked to send it again'
            : file?.why === 'not-stored'
              ? 'this machine had nowhere to store it, so nothing was downloaded — sending it again will not help, and whoever looks after the machine has the reason'
              : 'the hub did not say why'
      // Never "he has been told": what the hub does about telling him is an ordinary send against
      // a ceiling every project shares, and it can be shed with only the journal knowing.
      lines.push(`[${kind}]${filename !== undefined ? ` ${JSON.stringify(filename)}` : ''} did not come through: ${why}. The hub is saying so under his message too, though it cannot confirm that landed.`)
    }
    return { parts, lines, count: f.files.length }
  }

  // ── what he is told when the note cannot be obeyed ────────────────────────────────────────────
  //
  // Every one of these goes VERBATIM into the topic he typed in, under the line it refuses — the
  // hub only strips control characters and clips — so none of them carries a path, an id, a status
  // code or a word he has never been told exists. Each says the thing he could act on, and none of
  // them is ever followed by the words going somewhere else instead.
  const OLDER_THAN_THE_ONE_IN_USE = "the note naming this worker's session is older than the one already in use"
  const DOES_NOT_SAY_HOW_NEW_IT_IS = "the note naming this worker's session does not say how new it is"
  const BELONGS_TO_ANOTHER_PROJECT = 'the session named for this worker belongs to a different project'
  const BELONGS_TO_ANOTHER_CONVERSATION = 'the session named for this worker belongs to a different conversation'
  const CANNOT_TELL_WHOSE_CONVERSATION_IT_IS =
    "the note naming this worker's session says which conversation it belongs to, and this worker cannot tell whether that is this one"
  const HAS_BEEN_ARCHIVED = 'the session named for this worker has been archived'
  const IS_A_HELPERS_SESSION = "the session named for this worker is a helper's session, not the one to speak to"
  const IS_NOT_OPEN = 'the session named for this worker is not open on its server'
  const SAYS_NO_AGENT = 'the session named for this worker does not say which agent it is running'
  const RUNS_ANOTHER_AGENT = 'the session named for this worker is running a different agent from the one it should be'
  const A_DIFFERENT_AGENT_IS_ANSWERING = 'the worker is answering under a different agent from the one it should be'
  const THE_SERVER_DOES_NOT_KNOW_THE_AGENT = 'the worker is set to run as an agent its server does not know'
  const THE_QUESTION_HAS_MOVED_ON = 'the question you replied to was asked by a session this worker no longer speaks to'
  const MOVED_WHILE_ON_ITS_WAY = 'the worker moved to another session while that was on its way, so it was not delivered; send it again'

  /**
   * Which session his words go to. The MACHINE's answer, in this order, and never the text's:
   *
   *   0. When the wall gave a note naming this worker's own session (`--opencode-binding-file`),
   *      that session and no other — `theSessionTheNoteNames`, below. This is the only rule that
   *      can be RIGHT on a server running sessions for more than one wall, and where it is set,
   *      nothing here ever falls back to a guess: a line that cannot be delivered to the named
   *      session is refused out loud, with a reason the hub puts under the line he typed.
   *   1. Otherwise, typed under a question this watcher asked and still holds open: the session
   *      that asked it. It is the one time the operator has said which session he means. The words
   *      are still a prompt and not an answer to the question — its answers are the buttons
   *      opencode published, and a permission takes three words and no others — so the question
   *      stays open for his tap, and opencode runs the words once it is answered (measured,
   *      5 September).
   *   2. Otherwise the session this wall's server is running for the project directory attach
   *      speaks for: `GET /session?directory=<it>&roots=true`, which the server answers most
   *      recently updated first. That is a guess, and it is only ever taken when nobody said
   *      otherwise. Root sessions only, because a subagent's session is not the conversation on his
   *      phone; and nothing archived. Measured: the directory match takes a trailing slash and a
   *      symlink, and excludes a subfolder.
   *   3. None: refused, with a reason the hub can put in front of him. Never a guess — a guess is
   *      his words in a session he was not talking to.
   *
   * `GET /api/session/active` was measured and is NOT used: it sees only the v2 drains, and stayed
   * empty for the whole of a session driven through the v1 endpoint this watcher uses.
   */
  async function sessionForTypedWords(
    inReplyTo: string | null,
  ): Promise<{ sessionID: string; how: string; agent: string | null } | { refused: string }> {
    if (BINDING_FILE) return await theSessionTheNoteNames(inReplyTo)
    if (inReplyTo) {
      const asked = open.get(inReplyTo)
      // No note, so nothing here has ever been told which agent this worker is. The prompt names
      // none and the server resolves its own, which is exactly what happened before a note existed.
      if (asked) return { sessionID: asked.sessionID, how: `the session that asked ${inReplyTo}`, agent: null }
    }
    const listed = await rootSessionsHere()
    if ('refused' in listed) return listed
    const candidates = listed.list
      .filter((s: any) => s && typeof s.id === 'string' && s.id.startsWith('ses') && !s.parentID && !s.time?.archived)
      .sort((a: any, b: any) => Number(b.time?.updated ?? 0) - Number(a.time?.updated ?? 0))
    if (!candidates.length) {
      return { refused: 'the worker has no session open, so there was nothing to hand it to' }
    }
    return {
      sessionID: String(candidates[0].id),
      how: candidates.length === 1 ? 'the one session open' : `the most recently active of ${candidates.length} sessions`,
      agent: null,
    }
  }

  /**
   * The session the note names, proved to be the one it claims to be, on every line.
   *
   * Everything here is checked at DELIVERY time against the server's own answer, because each of
   * these has a way of being true when the note was written and false a minute later: the session
   * can be closed, archived, or replaced; the note can be a leftover from a previous run of the
   * wall; and an id a launcher wrote for another project's worker resolves perfectly well on a
   * server shared by several. The one measured on this box on 6 September is the last of them: the
   * only root session in a steering room's directory was an unrestricted `coordinator`, and the
   * session the room actually steers was a different one — so a note that says which AGENT it
   * expects refuses the coordinator instead of handing it the operator's words.
   *
   * The same measured listing decides all of it — one request, `directory` and `roots=true`, the
   * two rules captured from 1.18.25 — so a session that is not in it is not this project's root
   * session, whatever else it may be. A second, unfiltered request is made only to choose the
   * SENTENCE, never to widen what is accepted.
   */
  async function theSessionTheNoteNames(
    inReplyTo: string | null,
  ): Promise<{ sessionID: string; how: string; agent: string | null } | { refused: string; couldNotFindOut?: true }> {
    const b = bindingNow()
    if ('refused' in b) {
      // A question THIS watcher drew had its session proved against the note when the keyboard went
      // up, and a reply typed under it is the one time the operator has said which session he
      // means. While the note simply cannot be read — a launcher mid-rewrite, a file not put back
      // yet — refusing his reply told him to try again in a moment about a thing no moment of his
      // will mend, and left the question he was answering open in front of him. A note that has
      // MOVED ON is a different fact and is refused below, where it always was.
      if (b.couldNotFindOut && inReplyTo) {
        const asked = open.get(inReplyTo)
        // The agent THIS question was drawn under, because the note itself cannot be read at this
        // instant and a prompt that names none lets the server resolve its own — which on a tree
        // whose `opencode.json` names another agent by default is the very failure the binding
        // exists to refuse. It is the question's own agent and not the watcher's last one: the
        // session here is the one that ASKED, and after a rollover the two are different sessions
        // running different agents.
        if (asked) return { sessionID: asked.sessionID, how: `the session that asked ${inReplyTo}`, agent: asked.agent }
      }
      return b
    }
    const want = b.binding
    // The launcher's own claim about which project the session is for, checked against the project
    // attach speaks for before anything is asked of the server: a note written for another worker
    // must not steer this conversation even if that session is somehow listed here.
    if (want.canonicalProjectDir !== null && !sameDirectory(want.canonicalProjectDir, cfg.projectDir)) {
      return { refused: BELONGS_TO_ANOTHER_PROJECT }
    }
    const listed = await rootSessionsHere()
    if ('refused' in listed) return listed
    // The note again, now that the server has been waited on. Reading it once and delivering on
    // what it said before a network round trip is a window — ten seconds wide on a loaded server —
    // in which a launcher's rollover lands the operator's words in the session he has stopped
    // talking to, acked as though it went where he meant. Refused rather than retargeted: the
    // checks above were made about the old session, and nothing has proved them of the new one.
    const still = bindingNow()
    if ('refused' in still) return still
    if (still.binding.sessionID !== want.sessionID) return { refused: MOVED_WHILE_ON_ITS_WAY }
    const found = listed.list.find((s: any) => s && String(s.id) === want.sessionID)
    if (!found) return { refused: await whyItIsNotListed(want.sessionID) }
    if (found.time?.archived) return { refused: HAS_BEEN_ARCHIVED }
    // `roots=true` already drops these, and it is checked again because the cost of being wrong is
    // the operator steering a subagent that nobody is reading.
    if (found.parentID) return { refused: IS_A_HELPERS_SESSION }
    // The same reason, on the session's OWN word rather than on the query having been obeyed. The
    // sentence-picking listing below already applies this rule; acceptance skipping it meant a
    // server that ignored `directory=` — a proxy, a version that drops an unknown parameter —
    // would have handed this project's operator another project's worker.
    if (typeof found.directory === 'string' && !sameDirectory(found.directory, cfg.projectDir)) {
      return { refused: BELONGS_TO_ANOTHER_PROJECT }
    }
    if (want.agent !== null) {
      const running = typeof found.agent === 'string' && found.agent.length > 0 ? found.agent : null
      if (running === null) return { refused: SAYS_NO_AGENT }
      if (running !== want.agent) return { refused: RUNS_ANOTHER_AGENT }
    }
    if (inReplyTo) {
      const asked = open.get(inReplyTo)
      // A question asked before the note was rewritten belongs to a session this conversation has
      // stopped speaking to. Carrying his reply into it would steer a worker nobody is bound to,
      // and carrying it into the NEW session would answer a question that session never asked.
      if (asked && asked.sessionID !== want.sessionID) return { refused: THE_QUESTION_HAS_MOVED_ON }
      if (asked) {
        fenceBehind(want)
        return { sessionID: want.sessionID, how: `the session that asked ${inReplyTo}, which is the one the note names`, agent: want.agent }
      }
    }
    fenceBehind(want)
    return { sessionID: want.sessionID, how: 'the session the note names', agent: want.agent }
  }


  /** The root sessions this server lists for the project directory — the one measured listing. */
  async function rootSessionsHere(): Promise<{ list: any[] } | { refused: string; couldNotFindOut?: true }> {
    let list: unknown
    try {
      const r = await fetch(`${OPENCODE}/session?directory=${encodeURIComponent(cfg.projectDir)}&roots=true`, {
        signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS),
      })
      if (!r.ok) {
        note(`opencode would not say which session is open (${r.status})`)
        return { refused: "the worker's server would not say which session is open", couldNotFindOut: true }
      }
      list = await r.json()
    } catch (e) {
      note(`could not ask opencode which session is open: ${(e as Error)?.message ?? e}`)
      return { refused: unreached(e), couldNotFindOut: true }
    }
    if (!Array.isArray(list)) return { refused: "the worker's server gave an answer that could not be read", couldNotFindOut: true }
    return { list }
  }

  /**
   * Why the named session is not among this project's root sessions — the SENTENCE only.
   *
   * Nothing this answers can make a refused line deliverable; it exists so the operator is told
   * which of four different things went wrong, because the fix is different for each and "not
   * open" sent somebody looking for a dead server once. One unfiltered listing, on the same
   * deadline every request here has, and a server that will not answer it simply gets the plainest
   * of the four.
   */
  async function whyItIsNotListed(sessionID: string): Promise<string> {
    let list: unknown
    try {
      const r = await fetch(`${OPENCODE}/session`, { signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS) })
      if (!r.ok) return IS_NOT_OPEN
      list = await r.json()
    } catch (e) {
      note(`could not ask opencode about the session the note names: ${(e as Error)?.message ?? e}`)
      return IS_NOT_OPEN
    }
    if (!Array.isArray(list)) return IS_NOT_OPEN
    const anywhere = list.find((s: any) => s && String(s.id) === sessionID)
    if (!anywhere) return IS_NOT_OPEN
    if (anywhere.time?.archived) return HAS_BEEN_ARCHIVED
    if (anywhere.parentID) return IS_A_HELPERS_SESSION
    if (typeof anywhere.directory === 'string' && !sameDirectory(anywhere.directory, cfg.projectDir)) {
      return BELONGS_TO_ANOTHER_PROJECT
    }
    return IS_NOT_OPEN
  }

  /**
   * Where an answer goes. Four endpoints, and the spec is the only authority on which is which:
   * the v2 pair live under `/api` and name the session in the path, the v1 pair do not.
   */
  function replyUrl(o: Pick<Open, 'kind' | 'v2' | 'sessionID' | 'requestID'>): string {
    const sid = encodeURIComponent(o.sessionID)
    const rid = encodeURIComponent(o.requestID)
    if (o.kind === 'question') {
      return o.v2
        ? `${OPENCODE}/api/session/${sid}/question/${rid}/reply`
        : `${OPENCODE}/question/${rid}/reply`
    }
    return o.v2
      ? `${OPENCODE}/api/session/${sid}/permission/${rid}/reply`
      : `${OPENCODE}/permission/${rid}/reply`
  }

  /**
   * Post the operator's tap back to opencode, as the label it published — and say what became of it.
   *
   * The hub has already put "Sent: <label>" under his thumb and is waiting to edit that line. So
   * every tap is answered, exactly once, and NEVER before opencode has spoken: an `accepted` sent
   * when the POST was merely started is the dead keyboard this adapter exists to end, wearing a
   * thumbs-up. `refused` carries a reason in his own register, because that reason goes verbatim
   * into the topic under the receipt.
   */
  async function answer(askId: string, optionId: string, ref: string): Promise<void> {
    const o = open.get(askId)
    if (!o) {
      note(`a tap arrived for ${askId}, which this watcher has no record of; nothing was answered`)
      answerForTap(ref, 'the worker no longer has that question open')
      return
    }
    const label = o.labels.get(optionId)
    if (label === undefined) {
      note(`a tap named an option ${askId} never offered; nothing was answered`)
      answerForTap(ref, 'that is not one of the answers the worker offered')
      return
    }
    // A tap for a question this conversation can no longer speak to the asker of. Proved the same
    // way a typed line is — against the server, not against the note's raw text — because the two
    // directions being different rules is how his tap came to be posted into a session the next
    // line he typed was refused for.
    //
    // The record is KEPT rather than forgotten: nothing was answered, and if the note names that
    // session again the tap's own question is still the one it belongs to.
    //
    // And he is told. The hub has already put "Sent: X" under his tap; leaving him with that and
    // nothing else is the dead keyboard this adapter exists to end.
    if (BINDING_FILE) {
      // Named with the ask, not asked in the abstract: this is his answer to a question this
      // watcher drew, so the same rule that carries a REPLY typed under it carries the tap — a note
      // that cannot be read at this instant does not unsay which session asked, while a note that
      // has moved on refuses both.
      const to = await theSessionTheNoteNames(askId)
      const why = 'refused' in to ? to.refused : to.sessionID === o.sessionID ? null : THE_QUESTION_HAS_MOVED_ON
      if (why !== null) {
        note(`a tap arrived for ${askId}, whose session this conversation cannot speak to; nothing was answered`)
        say(
          { t: 'say', text: `Your answer did not reach the worker — ${why}. Nothing was sent to it.`, hint: 'prose' },
          'a word about an answer that reached nobody',
        )
        // And on the wire too, so a hub that reads a tap's answer edits the receipt under his thumb
        // rather than leaving it "Sent". The line above stays because it is the only thing that
        // reaches him on a hub from before a tap was answered for; on a hub that reads one he gets
        // the same fact twice, which is noise rather than a contradiction, and cheaper than his
        // only notice disappearing on the older half of the fleet.
        answerForTap(ref, why)
        return
      }
    }
    open.delete(askId)
    const url = replyUrl(o)
    // Both question endpoints take the same body — the answers to each question in order, and each
    // answer is the list of labels chosen for it. One question, one label.
    const body = o.kind === 'question' ? { answers: [[label]] } : { reply: label }
    try {
      const r = await fetch(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
        // Not carried one after another as typed words are, so one stuck tap takes no other with
        // it — but a request that never answers is still a promise this process holds for ever.
        signal: AbortSignal.timeout(OPENCODE_ANSWERS_WITHIN_MS),
      })
      if (!r.ok) {
        // The status is for a developer, here. The reason goes verbatim into his topic, where a
        // number is jargon — the same split the typed-words path makes, in the same words.
        note(`opencode refused the answer to ${askId} (${r.status})`)
        answerForTap(ref, "the worker's server would not take it")
        return
      }
      // No `ask_resolved` here. The hub already took the buttons off when it resolved the tap, and
      // said so in his own words. A second retirement from this side would overwrite that.
      note(`opencode took the answer to ${askId}`)
      answerForTap(ref)
    } catch (e) {
      note(`could not reach opencode to answer ${askId}: ${(e as Error)?.message ?? e}`)
      answerForTap(ref, unreached(e))
    }
  }

  /**
   * Tell the hub what became of one tap, on the wire.
   *
   * The same two shapes `answerFor` uses for his typed words, and deliberately the same words: a
   * server that would not take a line and one that would not take a tap are one fact to him.
   */
  function answerForTap(ref: string, refused?: string): void {
    say(
      refused ? { t: 'ack', ref, status: 'refused', reason: refused } : { t: 'ack', ref, status: 'accepted' },
      'an answer about a tap',
    )
  }

  /**
   * Events, handled strictly one after another.
   *
   * Deciding whether a question is this conversation's now asks the server, so drawing a keyboard
   * is no longer instant — and two things go wrong the moment events overlap. Two questions can be
   * drawn in the wrong order, so the phone's second keyboard belongs to the first question; and the
   * event that RETIRES a question can run before the question was drawn, which leaves a live
   * keyboard on his phone for something already answered. Ordering them costs a queue and settles
   * both. One slow answer from the server holds the rest for at most the deadline every request
   * here shares.
   */
  let handledInOrder: Promise<unknown> = Promise.resolve()
  function onOpencodeEvent(ev: Record<string, any>): void {
    handledInOrder = handledInOrder
      .then(() => handleOpencodeEvent(ev))
      .catch(e => note(`an event could not be handled: ${(e as Error)?.message ?? e}`))
  }

  /** One opencode event, mapped onto the hub's vocabulary. Exported shape kept identical to before. */
  async function handleOpencodeEvent(ev: Record<string, any>): Promise<void> {
    const type = String(ev?.type ?? '')
    // opencode carries the same payload under two names. `/event` nests it in `properties`; the
    // durable per-session stream nests it in `data`. Reading only one is not a parse error — every
    // field simply comes back undefined, and the first real permission request reached the phone
    // fine and then answered nothing, because its ask id was the string "pundefined".
    const data = ev?.properties ?? ev?.data ?? {}
    switch (type) {
      case 'question.v2.asked':
      case 'question.asked': {
        const boundTo = await fromTheBoundSession(String(data.sessionID ?? ''), ev)
        if (!boundTo) return
        const questions: any[] = Array.isArray(data.questions) ? data.questions : []
        // opencode can publish several questions in one request. Only the first is drawn: the reply
        // shape answers them in order, and a phone that shows two keyboards for one request cannot
        // say which one an answer belonged to.
        const q = questions[0]
        if (!q || !Array.isArray(q.options) || q.options.length === 0) {
          note('a question arrived with no options; it needs a keyboard this watcher cannot draw')
          return
        }
        const askId = `q${data.id}`
        const labels = new Map<string, string>()
        const options = q.options.slice(0, 3).map((o: any, i: number) => {
          const id = `o${i}`
          labels.set(id, String(o.label))
          return { option_id: id, label: String(o.label) }
        })
        remember(askId, {
          requestID: String(data.id),
          sessionID: String(data.sessionID),
          kind: 'question',
          v2: type.includes('.v2.'),
          labels,
          agent: boundTo.agent,
        })
        const more =
          questions.length > 1 ? `\n\n(it asked ${questions.length} things at once; this is the first)` : ''
        const trimmed = q.options.length > options.length ? `\n\n(showing ${options.length} of ${q.options.length} choices)` : ''
        say({ t: 'ask', ask_id: askId, text: `${q.question}${more}${trimmed}`, options }, `a question (${askId})`, askId)
        return
      }
      case 'permission.v2.asked':
      case 'permission.asked': {
        const boundTo = await fromTheBoundSession(String(data.sessionID ?? ''), ev)
        if (!boundTo) return
        // The reply set is opencode's own and it is closed: once, always, reject. The operator picks
        // one of three; he never names the action, and this watcher never invents a fourth.
        const askId = `p${data.id}`
        const labels = new Map<string, string>([
          ['once', 'once'],
          ['always', 'always'],
          ['reject', 'reject'],
        ])
        remember(askId, {
          requestID: String(data.id),
          sessionID: String(data.sessionID),
          kind: 'permission',
          v2: type.includes('.v2.'),
          labels,
          agent: boundTo.agent,
        })
        const what = Array.isArray(data.resources) && data.resources.length
          ? `${data.action}: ${data.resources.join(', ')}`
          : String(data.action ?? 'something')
        say(
          {
            t: 'ask',
            ask_id: askId,
            text: `It wants to ${what}`,
            options: [
              { option_id: 'once', label: 'Just this once' },
              { option_id: 'always', label: 'Always' },
              { option_id: 'reject', label: 'No' },
            ],
          },
          `a permission request (${askId})`,
          askId,
        )
        return
      }
      case 'question.v2.replied':
      case 'question.v2.rejected':
      case 'permission.v2.replied': {
        // Answered somewhere else — at the keyboard, or by a saved rule. The buttons come off, so a
        // stale keyboard cannot be tapped an hour later. This is the frame no screen could produce.
        const id = String(data.requestID ?? data.id ?? '')
        const askId = (type.startsWith('question') ? 'q' : 'p') + id
        // Before the record is looked for: a question still waiting to be decided on has no record
        // yet, and offering it after it has been answered at the keyboard puts a live keyboard on
        // his phone for something already settled.
        stopKeeping(id)
        if (!open.has(askId)) return
        open.delete(askId)
        say(
          { t: 'ask_resolved', ask_id: askId, how: type.endsWith('rejected') ? 'withdrawn' : 'answered' },
          `retiring ${askId}`,
        )
        return
      }
      case 'session.error': {
        // The agent could not run on what he typed. opencode says so twice for one failure — the
        // second carrying a stack trace — and then `session.idle`; the first is answered, once,
        // and the session is forgotten so the second says nothing. Only for a session his words
        // went to and that has not finished a turn since: an error in a session nobody typed at
        // is the agent's own business, in its own terminal. The `ack` for his words was spent
        // when opencode wrote them down, so this is a line in the topic, in his words. The first
        // line of the message, control characters out: opencode's second event is a stack.
        const sid = String(data.sessionID ?? '')
        if (!sid || !prompted.delete(sid)) return
        const err = data.error ?? {}
        const raw = firstLine(err?.data?.message) || firstLine(err?.name) || ''
        // One of these failures is this watcher's own doing. Measured against a real opencode
        // 1.18.25 on 7 September: a prompt naming an agent the server cannot resolve is answered
        // 204, NO user message is written — his words are gone — and the failure arrives here as
        // `Agent not found: "<name>". Available agents: build, explore, general, plan`. Forwarded
        // word for word that is a quoted identifier and an internal roster in a message on his
        // phone. The prompt names the agent because the note binds one, so this side knows what
        // that sentence means and says it in his register instead. Reachable without anybody
        // touching the note: an agent renamed or dropped from the tree's own configuration while
        // a session lives passes every check made against the SESSION and fails at the prompt.
        const said = /^Agent not found:/.test(raw) ? THE_SERVER_DOES_NOT_KNOW_THE_AGENT : raw || 'it did not say why'
        say(
          { t: 'say', text: `The agent could not act on what you typed: ${said}`, hint: 'prose' },
          'a word about typed words the agent could not act on',
        )
        return
      }
      case 'session.idle': {
        // A turn ended. Whatever he typed at this session has been read, so a later error in it
        // is not about his words.
        const sid = String(data.sessionID ?? '')
        if (sid) prompted.delete(sid)
        // A heartbeat speaks for THIS conversation's worker, and on a server shared by two walls a
        // stranger's turn ending says nothing about this one — it would have the hub reading a
        // worker as settled between turns while it is still mid-turn.
        if (BINDING_FILE && boundSessionNow() !== sid) return
        say({ t: 'beat', state: 'idle' }, 'a heartbeat')
        return
      }
      default:
        return
    }
  }

  /** The first line of a message, fit to put on a phone: no control characters, and not a stack. */
  function firstLine(s: unknown): string {
    if (typeof s !== 'string') return ''
    return s
      .split(/\r?\n/)[0]
      .replace(/[\u0000-\u001f\u007f]/g, ' ')
      .trim()
      .slice(0, 300)
  }

  /**
   * Follow the server's event stream, reconnecting for as long as this process lives.
   *
   * A stream that ends is normal — the server restarts, a proxy times it out — so the end of one is
   * not an error and never stops the loop.
   */
  async function watch(): Promise<void> {
    for (;;) {
      try {
        const r = await fetch(`${OPENCODE}/event`, { headers: { accept: 'text/event-stream' } })
        if (!r.ok || !r.body) throw new Error(`opencode answered ${r.status}`)
        note(`watching ${OPENCODE}`)
        const reader = r.body.getReader()
        const dec = new TextDecoder()
        let buf = ''
        for (;;) {
          const { done, value } = await reader.read()
          if (done) break
          buf += dec.decode(value, { stream: true })
          for (;;) {
            const nl = buf.indexOf('\n')
            if (nl < 0) break
            const line = buf.slice(0, nl).trim()
            buf = buf.slice(nl + 1)
            if (!line.startsWith('data:')) continue
            try {
              onOpencodeEvent(JSON.parse(line.slice(5).trim()))
            } catch {
              // One unreadable event never ends the stream.
            }
          }
          if (buf.length > MAX_FRAME_BYTES) buf = ''
        }
      } catch (e) {
        note(`lost the opencode stream: ${(e as Error)?.message ?? e}`)
      }
      await new Promise(r => setTimeout(r, 2000))
    }
  }

  link.start()
  void watch()
}
