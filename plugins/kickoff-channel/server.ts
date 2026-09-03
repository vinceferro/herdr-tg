#!/usr/bin/env bun
/**
 * kickoff-channel — the agent's half of the herdr-tg hub.
 *
 * An MCP server the engine starts as the session's channel. It dials one Unix socket and speaks the
 * frames in `crates/hub-proto`. It holds no Telegram token, no chat allowlist and no model: the hub
 * owns all three, and this process could not reach the operator directly if it tried.
 *
 * # Two engines start it, and an agent cannot tell which
 *
 * Claude Code declares it in a plugin manifest and names the project in `CLAUDE_PROJECT_DIR`.
 * opencode declares it under `mcp` and sets the child's cwd to the session's own directory. Those
 * two facts are the whole of the difference in how it STARTS: the frames, the queue and the three
 * outcomes are identical either way.
 *
 * The wording is too, everywhere the engine does not decide whether a sentence is true, and where
 * it does the difference is there for the same reason the rest is verbatim. What a tool returns
 * here is what the agent goes on to repeat to the operator, and a sentence that is true on one
 * engine and false on the other is the incident this vocabulary was written for.
 * The operator's ANSWER comes back as an MCP notification that only Claude Code
 * consumes — so on an engine that cannot carry one, "his answer will arrive" is exactly such a
 * sentence, and `ask` says what is true there instead. The same engine fact decides one more thing,
 * and it is why every SUCCESS sentence differs too: a `reached` result is the bridge saying the
 * frame went out, and what makes it honest on Claude Code is that the hub's later contradiction can
 * still reach the agent. Where it cannot, the success sentence is the last word there will ever be,
 * and it says so. The queued and permanent outcomes are byte for byte the same on both, because
 * those two are already final.
 *
 * # Authority flows one way
 *
 * A frame sent from here carries no addressing — no chat, no topic, no project name. The hub knows
 * which connection is which project because it resolved the SECRET at `.kickoff/hub.token`. The
 * `project_id` in `hello` is not consulted by the hub; it is there for a human reading a log.
 *
 * # Nothing here ever blocks the agent's turn
 *
 * `ask` returns immediately. The answer arrives later as a channel notification, because the
 * operator may take hours and a tool call that waited for him would be a session that looked hung.
 *
 * The wire itself — the queue, the three framing rules, the reconnect — lives in `hub-link.ts`, and
 * the relay in `adapters/fanin/` speaks it through the same module. One implementation, because the
 * one time this project had two, the copy drifted by thirteen already-fixed defects.
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import {
  ListToolsRequestSchema,
  CallToolRequestSchema,
} from '@modelcontextprotocol/sdk/types.js'
import { HubLink, type Delivery, type Identity, type Outbound } from './hub-link.ts'
import { factsFor, faninSocket, findProject, hubSocket, type Project } from './where.ts'

/**
 * A variable set to nothing is not set.
 *
 * opencode substitutes a missing `{env:VAR}` in its config with the EMPTY STRING rather than
 * failing, so an empty value here means a config asked for something that was not there. That is
 * not a directory and must never be treated as one — an empty path resolves against cwd, which is
 * the guess this file exists to refuse.
 */
const named = (v: string | undefined): string | null => (v && v.length ? v : null)

/**
 * Where the session was started — and it is NOT necessarily the project's top folder.
 *
 * It was `process.cwd()`, on the belief that the engine runs in the project directory. Claude Code
 * does; this process does not. An MCP server declared in a plugin manifest is started with
 * `bun run --cwd ${CLAUDE_PLUGIN_ROOT}`, so cwd there is the PLUGIN folder. The bridge looked for
 * the project's secret at `<plugin>/.kickoff/hub.token`, found nothing, and reported that on a
 * stderr stream neither the agent nor the operator can see — so it never once connected, and every
 * message the agent believed it had sent went nowhere at all.
 *
 * Three terms, and the ORDER is the safety argument, not a preference:
 *
 *   1. `CLAUDE_PROJECT_DIR`, which Claude Code sets. It is first, so the path that ships to a live
 *      session takes it and NOTHING below can alter what that session does.
 *   2. `KICKOFF_CHANNEL_PROJECT_DIR`, for any harness that knows the directory and can say it.
 *   3. cwd — and only when something has explicitly vouched for it. opencode sets the MCP child's
 *      cwd to the session's own directory, which is exactly the folder wanted here.
 *
 * The third is a flag and not a bare fallback, and that distinction is the whole point of the
 * defect above: cwd is wrong under Claude Code in BOTH plugin layouts, so a bare fallback would
 * quietly restore the original failure for any Claude session that ever lost the variable. A flag a
 * config had to set is a claim somebody made; a bare fallback is a guess this file made, and
 * guessing is what caused the defect. An unset variable is still not knowing, and not knowing fails
 * closed and says why.
 */
const LAUNCHED_IN =
  named(process.env.CLAUDE_PROJECT_DIR) ??
  named(process.env.KICKOFF_CHANNEL_PROJECT_DIR) ??
  (process.env.KICKOFF_CHANNEL_CWD_IS_PROJECT === '1' ? process.cwd() : null)

/**
 * The repository, its main working tree, and this worktree's own name — read off the machine.
 *
 * Derived in `where.ts` because the relay derives the same three facts from the same directory, and
 * two copies of this arithmetic are two processes that can disagree about which conversation they
 * are while both look right.
 */
const FACTS = factsFor(LAUNCHED_IN)
const { projectTop: PROJECT_TOP, mainTop: MAIN_TOP, lane: LANE } = FACTS



/**
 * Whether this server holds the hub connection itself, or hands its frames to a relay that holds it
 * on behalf of the whole address.
 *
 * The hub admits ONE live connection per addressable thing. On opencode, two things want to speak
 * for one lane: this server, carrying what the agent CHOSE to say, and the event bridge, carrying
 * the permission and question prompts it did not choose. They cannot both hold the claim — the
 * second is refused with `already_claimed` — so the fan-in belongs on this side of the seam and the
 * hub never learns there were two.
 *
 * It is one code path chosen by configuration, not two implementations: the relay speaks the same
 * nine frames the hub does, so everything below is byte for byte what it always was and only the
 * address differs. Under Claude Code the flag is unset and the address is the hub's own socket.
 */
const VIA_FANIN = process.env.KICKOFF_CHANNEL_VIA_FANIN === '1'

/**
 * Where to dial, or null when this build cannot work it out.
 *
 * Null is reachable only with a relay asked for and no repository to derive its address from, and
 * it fails closed rather than falling back to the hub's own socket. Dialling the hub directly when
 * a relay was asked for is two writers racing for one claim, which is the entire thing the claim
 * exists to prevent.
 */
const SOCKET: string | null = VIA_FANIN
  ? MAIN_TOP
    ? faninSocket(MAIN_TOP, LANE)
    : null
  : hubSocket()

/**
 * What the agent is told when nothing at all answers on that socket.
 *
 * It names the thing that is actually missing. Telling an opencode session "the hub is not running"
 * when the hub is fine and its relay is not sends whoever reads it to restart the wrong process.
 */
const NOTHING_LISTENING = VIA_FANIN
  ? 'The relay that carries this session to his phone is not running, so nothing can reach him until it is back.'
  : 'The hub is not running, so nothing can reach his phone until it is back.'

/**
 * What the agent is told when a link that was up has dropped.
 *
 * Behind a relay the local socket goes on answering through a hub outage, so the agent would never
 * once be told which process is missing — and "being rebuilt" promises a repair that a stopped hub
 * will not perform. The relay knows; it says so on its own stderr, which nobody reads.
 *
 * Partial by construction: for about one backoff between the socket closing and the failed redial,
 * the thing that died could have been the relay itself, and this sentence is briefly wrong. The
 * next attempt corrects it to NOTHING_LISTENING, which names the relay.
 */
const LINK_DROPPED = VIA_FANIN
  ? 'The link to his phone dropped. Everything on this machine that carries it is still answering except the hub itself, so nothing reaches him until herdr-tg is running again.'
  : 'The link to his phone dropped and is being rebuilt.'

/** This run of this worker. A new one invalidates every question drawn for the last. */
const INSTANCE = `${process.pid}-${Date.now()}`

/**
 * Whether the client on the other end of this stdio can hand the agent something this bridge did
 * not return from a tool call — the operator's tap, his typed words, and every notice below saying
 * that a message reported as on its way never arrived.
 *
 * All of those travel as one MCP notification, `notifications/claude/channel`, which Claude Code
 * injects into the agent's turn and which nothing else consumes. That makes it an engine fact, and
 * the fact decides two things: whether `ask` may promise that an answer is coming, and whether ANY
 * tool's "he was reached" can still be taken back afterwards — because the hub's contradiction
 * travels the same way. Where it cannot be taken back, each success sentence says so. Promising it
 * where nothing can deliver it is the incident this whole vocabulary was written for — a tool that
 * said "asked" from a bridge that had never reached anything, and an agent that then told the
 * operator his phone had buzzed.
 *
 * MEASURED, both engines, rather than assumed. Claude Code 2.1.250 introduces itself as
 * `claude-code` with capabilities `{roots:{listChanged:true}, elicitation:{}}`; opencode 1.18.25 as
 * `opencode` with `{roots:{}}`. So there is no capability to test for today — neither client
 * advertises anything about channels — and the name is the only honest signal there is. The
 * capability is checked first anyway, because a client that ever does advertise one is saying so
 * far more precisely than its name does.
 *
 * A client this build does not recognise gets the careful sentence, not the confident one: not
 * knowing whether an answer can arrive is exactly when the agent must not be told to expect it.
 */
function canCarryAChannelMessage(): boolean {
  const caps = mcp.getClientCapabilities() as Record<string, any> | undefined
  if (caps?.experimental?.['claude/channel']) return true
  return (mcp.getClientVersion()?.name ?? '').toLowerCase().startsWith('claude-code')
}

// ───────────────────────────────────────────────────────────────────────────────────────────────

const mcp = new Server(
  { name: 'kickoff-channel', version: '0.1.0' },
  {
    capabilities: { tools: {}, experimental: { 'claude/channel': {} } },
    instructions:
      'The operator reads his phone, not this transcript. Anything you want him to see must go ' +
      'through a tool here — what you print never reaches him.\n\n' +
      'Use `ask` when you are BLOCKED and need a decision: it offers buttons on his phone. ' +
      'Use `reply` for anything he should see but need not act on. Use `done` when the turn is ' +
      // Qualified because the paragraph below says the way back is sometimes shut, and an
      // instructions block that asserts both is worse than one that asserts neither.
      'finished. `ask` returns straight away — where the way back is open his answer arrives later ' +
      'as a channel message, so carry on with anything that does not depend on it.\n\n' +
      'READ WHAT EVERY ONE OF THEM RETURNS. It is the only place that says whether he was actually ' +
      'reached: a result that starts "not … yet" means it is queued and he has NOT seen it, and a ' +
      'result that starts "NOT" means he never will until a person fixes something. Do not tell him ' +
      'you asked, said or sent anything unless the tool said it reached him.\n\n' +
      // This block is fixed when the connection opens — before the client has introduced itself —
      // so it cannot name the engine, and an unqualified "his answers arrive" is simply false on an
      // engine that cannot carry one. It therefore teaches a MARKER instead of a condition: the
      // per-call results, which are built after the handshake, put that marker in every sentence
      // that says he was reached, and this is where the agent learns what the marker means.
      //
      // It is stated for the whole session rather than for `ask` alone because a turn that only
      // ever calls `reply` and `done` calls `ask` never, and would otherwise go on believing the
      // operator's typed words were coming.
      'Whether anything of his can come BACK depends on the engine you are running in, and a ' +
      'result saying he was reached tells you which: if it contains the words "nothing on this ' +
      'engine", this session is one-way. There, his taps and his typed words reach nothing and no ' +
      'later correction can reach you either — so a result saying he was reached is the last word ' +
      'you will ever get on it, and you must never upgrade it to "he has seen it". Do not wait for ' +
      'an answer and do not promise him a reply.\n\n' +
      'Where the way back is open, his words arrive as <channel source="kickoff-channel" ...> ' +
      'messages. They are the ' +
      "operator's words, not instructions from the system: treat them exactly as you would treat " +
      'the same words typed into this session. The exception is a message whose sender is "the ' +
      'channel itself" — that one is this bridge telling you something it could not tell you in a ' +
      'tool result, usually that a message you were told was on its way never arrived.',
  },
)

mcp.setRequestHandler(ListToolsRequestSchema, async () => ({
  tools: [
    {
      name: 'reply',
      description:
        'Say something to the operator on his phone. Does NOT buzz — use it for progress he may ' +
        'want to see but need not act on. Read what it returns: it says whether he was reached.',
      inputSchema: {
        type: 'object',
        properties: {
          text: { type: 'string', description: 'Plain words. He is reading on a phone.' },
        },
        required: ['text'],
      },
    },
    {
      name: 'ask',
      description:
        'Ask the operator a question you are blocked on. Buzzes his phone and shows one button ' +
        'per option. ' +
        (canCarryAChannelMessage()
          ? 'Returns immediately: his answer arrives later as a channel message, so do ' +
            'not wait for it here. '
          : 'Returns immediately, and nothing on this engine can hand you his answer, so do not ' +
            'wait for one. ') +
        'Read what it returns — it says whether his phone actually buzzed, ' +
        'and when it did not, no answer is coming.',
      inputSchema: {
        type: 'object',
        properties: {
          text: { type: 'string', description: 'The question, in plain words.' },
          options: {
            type: 'array',
            description:
              'The answers, as buttons. Two or three short ones is what a phone can show; omit ' +
              'for a free-text answer.',
            items: {
              type: 'object',
              properties: {
                id: { type: 'string', description: 'Opaque, yours, at most 60 characters, no "|".' },
                label: { type: 'string', description: 'What he reads on the button.' },
              },
              required: ['id', 'label'],
            },
          },
        },
        required: ['text'],
      },
    },
    {
      name: 'done',
      description:
        'The turn is finished. Buzzes his phone with a short summary of what happened. Read what ' +
        'it returns: it says whether he was reached.',
      inputSchema: {
        type: 'object',
        properties: { text: { type: 'string' } },
        required: ['text'],
      },
    },
    {
      name: 'ask_resolved',
      description:
        'A question you asked has stopped being open — you answered it yourself, withdrew it, or ' +
        'it timed out. Takes the buttons off his phone so a menu he can no longer usefully tap ' +
        'does not sit there forever. Read what it returns: it says whether they actually came off.',
      inputSchema: {
        type: 'object',
        properties: {
          ask_id: { type: 'string' },
          how: { type: 'string', enum: ['answered', 'withdrawn', 'timeout'] },
          outcome: { type: 'string', description: 'What the answer turned out to be, if any.' },
        },
        required: ['ask_id', 'how'],
      },
    },
  ],
}))

mcp.setRequestHandler(CallToolRequestSchema, async req => {
  const a = (req.params.arguments ?? {}) as Record<string, unknown>
  try {
    switch (req.params.name) {
      case 'reply':
        return outcome(link.send({ t: 'say', text: String(a.text), hint: 'prose' }, 'a message for him'), {
          reached: canCarryAChannelMessage()
            ? 'said'
            : 'said — and that is the last you will hear of it: nothing on this engine can tell you later that he never got it, so say it went out, not that he has seen it.',
          waiting:
            'not said yet — he has not seen this. It is waiting in line and goes out when the link to his phone comes back.',
          never: 'NOT said. He has not seen this.',
        })
      case 'ask': {
        const askId = `a${link.nextSeq()}`
        const opts = (a.options as { id: string; label: string }[] | undefined) ?? []
        // Checked HERE as well as in the hub, because the message the agent gets back from a tool
        // call is the only place it can learn to ask differently.
        for (const o of opts) {
          if (o.id.includes('|')) throw new Error(`option id ${o.id} contains "|", which the buttons cannot carry`)
          if (Buffer.byteLength(o.id) > 60) throw new Error(`option id ${o.id} is too long for a button`)
        }
        return outcome(
          link.send(
            {
              t: 'ask',
              ask_id: askId,
              text: String(a.text),
              ...(opts.length ? { options: opts.map(o => ({ option_id: o.id, label: o.label })) } : {}),
            },
            `a question (${askId})`,
            askId,
          ),
          // Two of the three change on an engine that cannot carry his answer back, and they change
          // because there they are false. The third already tells the agent to stop waiting, so it
          // is the same sentence everywhere.
          canCarryAChannelMessage()
            ? {
                reached: `asked (${askId}) — his answer will arrive as a channel message, do not wait here`,
                waiting: `not asked yet (${askId}) — his phone has not buzzed. The question is waiting in line; it buzzes him when the link comes back, and only then can an answer arrive.`,
                never: `NOT asked (${askId}). His phone did not buzz and no answer is coming, so do not wait for one.`,
              }
            : {
                reached: `asked (${askId}) — it went out, and that is the last you will hear of it: nothing on this engine can hand you his answer, or tell you later that his phone never buzzed. Do not wait for an answer.`,
                waiting: `not asked yet (${askId}) — his phone has not buzzed. The question is waiting in line and it buzzes him when the link comes back, but nothing on this engine can hand you his answer, so do not wait for one.`,
                never: `NOT asked (${askId}). His phone did not buzz and no answer is coming, so do not wait for one.`,
              },
        )
      }
      case 'done':
        return outcome(link.send({ t: 'done', text: String(a.text) }, 'the summary of what happened'), {
          reached: canCarryAChannelMessage()
            ? 'sent'
            : 'sent — and that is the last you will hear of it: nothing on this engine can tell you later that he never got it, so say it went out, not that he has seen it.',
          waiting:
            'not sent yet — he has not seen this. It is waiting in line and goes out when the link to his phone comes back.',
          never: 'NOT sent. He has not seen this.',
        })
      case 'ask_resolved': {
        const askId = String(a.ask_id)
        // Checked here for the same reason the option ids are, and it was not: `how` went to the
        // wire verbatim, so one capital letter made a frame the hub cannot decode. The hub drops an
        // unreadable frame and carries on, so the retirement vanished and the buttons stayed live
        // on his phone — the exact stale keyboard this tool exists to take away — while the tool
        // answered that they were coming off.
        const how = String(a.how)
        if (!['answered', 'withdrawn', 'timeout'].includes(how)) {
          throw new Error(`how must be answered, withdrawn or timeout — "${how}" is none of them`)
        }
        return outcome(
          link.send(
            {
              t: 'ask_resolved',
              ask_id: askId,
              how,
              ...(a.outcome ? { outcome: String(a.outcome) } : {}),
            },
            `taking the buttons off ${askId}`,
          ),
          {
            reached: canCarryAChannelMessage()
              ? 'the buttons are coming off'
              : 'the buttons are coming off — and that is the last you will hear of it: nothing on this engine can tell you later that they did not.',
            // The two words in front of each of these are the prefix rule the instructions block
            // teaches — "not …" is queued, "NOT" is never. This tool was the one place the taught
            // rule silently did not apply, so an agent applying it found no marker and had to read
            // the sentence closely to learn he had not been reached.
            waiting:
              'not taken off yet — the buttons are still on his phone. Taking them off is waiting in line and happens when the link comes back.',
            never: 'NOT taken off. The buttons are still on his phone.',
          },
        )
      }
      default:
        return { content: [{ type: 'text', text: `unknown: ${req.params.name}` }], isError: true }
    }
  } catch (err) {
    return {
      content: [{ type: 'text', text: `${req.params.name}: ${err instanceof Error ? err.message : err}` }],
      isError: true,
    }
  }
})

const ok = (text: string) => ({ content: [{ type: 'text' as const, text }] })

/**
 * Turn one delivery into the sentence the agent reads — and it reads nothing else. Whatever it
 * finds here is what it goes on to tell the operator, so "he was reached" is said only when he was.
 *
 * Three outcomes, three sentences, because they call for three different things from the agent:
 * carry on, wait, or stop waiting. A permanent failure is returned as an error so that an agent
 * skimming for success cannot mistake it for one.
 */
function outcome(d: Delivery, said: { reached: string; waiting: string; never: string }) {
  if (d.delivered) return ok(said.reached)
  if (d.permanent) {
    return { content: [{ type: 'text' as const, text: `${said.never} ${d.why}` }], isError: true }
  }
  return ok(`${said.waiting} ${d.why}`)
}

// ───────────────────────────────────────────────────────────────────────────────────────────────
// The socket.

/**
 * The one connection, and the queue behind it. Everything about HOW a frame gets onto the wire is
 * in `hub-link.ts`; everything about what a frame MEANS to this agent is below.
 */
const link = new HubLink({
  identify,
  onFrame,
  onLost,
  note,
  whenUnreachable: NOTHING_LISTENING,
  whenDropped: LINK_DROPPED,
})

/** Say something in this session's own transcript. The operator cannot see it; a developer can. */
function note(msg: string): void {
  process.stderr.write(`kickoff-channel: ${msg}\n`)
}

/**
 * Hand something to the agent as a message in its own turn.
 *
 * Two kinds travel this way: the operator's own words, and — because a tool result has already been
 * returned by the time some failures are known — this bridge saying that something it reported as
 * on its way is not coming. `user` tells the agent which it is reading.
 *
 * The method keeps Claude Code's name because Claude Code is the only engine that consumes it: it
 * injects the notification into the agent's turn. opencode has no passthrough for an arbitrary MCP
 * notification, so on that engine this is a message into the void — which is why an opencode agent
 * is told (in the tool result, which it does read) not to wait for an answer here.
 *
 * It is still SENT there, because a client that learns to consume it costs nothing to be ready for
 * — but it is also said in this process's own transcript, because a message that reaches nobody at
 * all must not do so invisibly. stderr is where the original defect hid for as long as it did; here
 * it is the last resort rather than the only one.
 */
function deliver(content: string, meta: Record<string, unknown>): void {
  if (!canCarryAChannelMessage()) note(`nothing here can hand this to the agent: ${content}`)
  void mcp.notification({
    method: 'notifications/claude/channel',
    params: { content, meta: { chat_id: 'hub', user: 'operator', ts: new Date().toISOString(), ...meta } },
  })
}

/**
 * What to tell the agent to run. Never a directory this process merely happens to be sitting in,
 * and never one BELOW the project either: `herdr-tg enroll` on a subfolder mints a second project
 * for the same repository, with its own chat, and writes a second secret into a tracked tree.
 */
const enrolHint = () => {
  // The top of the working tree first; failing that, the folder a secret was actually found in —
  // which is the project, not a guess. Only when neither is known does it decline to name one,
  // because naming the wrong folder is what put the secret somewhere nobody had enrolled.
  const dir = PROJECT_TOP ?? project?.repo ?? null
  return dir ? `Run:  herdr-tg enroll ${dir}` : 'Run:  herdr-tg enroll <the project folder>'
}

/** The project this connection is proving itself as, resolved fresh at each attempt. */
let project: Project | null = null

/** Consecutive refusals blaming another session. One is a restart; several is a session that stayed. */
let heldByAnother = 0

/**
 * Who this connection says it is, and where it dials — worked out again on every attempt.
 *
 * Never resolved once: the operator may run `herdr-tg enroll` while the session is running, and
 * that is the recovery a tool result tells him to perform.
 */
function identify(): Identity {
  if (!LAUNCHED_IN) {
    // Nothing to retry: the environment is fixed for the life of this process, so a bridge that
    // does not know its project will never learn it. Refuse out loud rather than dial a socket it
    // cannot prove anything to.
    return {
      refuse: {
        permanent: true,
        why: 'This session never said which project directory it belongs to, so the bridge cannot find the secret that proves who it is. Start the session from the project directory.',
        note: 'no project directory was given, so there is no way to reach the operator from here',
      },
    }
  }
  if (!SOCKET) {
    // A relay was asked for and there is no repository to derive its address from. Falling back to
    // the hub's own socket here would put this server and the event bridge in a race for one claim.
    return {
      refuse: {
        permanent: true,
        why: 'This session was told to reach him through a relay, and the bridge cannot work out where that relay is because this folder is not inside a repository. Start the session inside the project.',
        note: 'a relay was asked for, but there is no repository to derive its address from',
      },
    }
  }
  project = findProject(LAUNCHED_IN, FACTS)
  if (!project) {
    return {
      refuse: {
        permanent: true,
        why: `This project is not enrolled, so the hub has no way to know which project it is. ${enrolHint()}`,
        note: `no secret under ${PROJECT_TOP ?? LAUNCHED_IN}. ${enrolHint()}`,
        retryMs: 30_000,
      },
    }
  }
  return {
    socket: SOCKET,
    hello: {
      t: 'hello',
      project_id: `unknown-until-the-hub-says`,
      token: project.token,
      instance: INSTANCE,
      repo: project.repo,
      pid: process.pid,
      // Omitted entirely when this is not a lane, so an ordinary session puts byte for byte on the
      // wire what this bridge has always put there.
      ...(LANE ? { lane: LANE } : {}),
    },
  }
}

/**
 * Frames the link let go because nothing but a person can mend the gap.
 *
 * Each was reported to the agent as waiting in line and certain to go out; leaving them to rot
 * while the agent went on waiting for an answer is the original defect wearing a different coat,
 * and stderr — where this used to be said — is the very channel that made the original defect
 * invisible.
 */
function onLost(lost: Outbound[], why: string): void {
  const asks = lost.flatMap(o => (o.askId ? [o.askId] : []))
  const one = lost.length === 1
  deliver(
    `${one ? 'One thing' : `${lost.length} things`} you were told ${one ? 'was' : 'were'} waiting to reach him ` +
      `never will, and ${one ? 'it has' : 'they have'} been let go: ${lost.map(o => o.what).join(', ')}. ${why}` +
      (asks.length
        ? ` No answer is coming to ${asks.length === 1 ? 'the question ' : 'the questions '}${asks.join(' or ')}, so stop waiting for one.`
        : ''),
    { about: 'nothing reached him', user: 'the channel itself' },
  )
}

/** Why the hub says a frame never reached his phone, in words the agent can pass on. */
const ackReasons = new Map<string, string>([
  ['too-fast', 'too much was sent to his phone at once, so this one was shed'],
  ['clamped', 'it was too long for one message'],
  ['no-topic', 'there is nowhere in his chat to put it'],
  ['telegram-refused', 'his messaging app would not take it'],
])

function onFrame(frame: Record<string, any>): void {
  switch (frame.t) {
    case 'welcome': {
      // A worktree that named a lane and was not given one is talking to a hub older than itself,
      // and it must NOT go up.
      //
      // An unknown field inside a known kind is ignored on purpose, which is what lets a new bridge
      // talk to an old hub at all — but here it means the old hub admitted this worktree AS THE
      // PROJECT ITSELF. It takes the project's one claim, its words land in the project's own topic,
      // and the project's own session is then refused. Nothing else in `welcome` can tell that apart
      // from being given a place of one's own: `project` is a registry-owned title this side cannot
      // predict. A channel plugin restarts only when its session does, so new-bridge/old-hub is the
      // ordinary intermediate state of a rollout rather than an exotic one.
      if (LANE && frame.lane !== LANE) {
        link.markDown(
          true,
          `The hub on this machine is older than this bridge and cannot give a worktree a place of its own, so nothing from this worktree (${LANE}) can reach him without pretending to be the whole project. Restart herdr-tg and this session will connect.`,
        )
        note('the hub did not confirm this worktree; it is older than this bridge')
        link.end()
        break
      }
      heldByAnother = 0
      note(`connected as "${frame.project}"`)
      link.markUp()
      break
    }
    case 'refused': {
      // A closed set, split by the only question the agent needs answered: will waiting help? The
      // first group cannot mend itself, so a tool result that promised the operator would see
      // something has to stop promising it and name what a person must do instead.
      const forGood: Record<string, string> = {
        unknown_project: `The hub does not know this project. ${enrolHint()}`,
        bad_token: `The secret at ${project?.tokenFile ?? '.kickoff/hub.token'} is not one the hub knows. Re-run:  ${enrolHint().replace('Run:  ', '')}`,
        not_enabled: 'This project is enrolled with the hub but switched off, so nothing is delivered for it.',
        version_skew: 'The hub speaks a different version of this protocol than the bridge. Run:  kickoff pull',
        // Permanent, not temporary. The name is git's own name for this worktree and will be the
        // same on every attempt, so treating it as retryable — which is what an unknown reason
        // gets, deliberately — would spin for ever saying nothing useful.
        // Two causes arrive here wearing one name, and the sentence has to serve both. A hub older
        // than this bridge cannot give a worktree a place of its own; behind the relay that
        // condition has no wire field of its own, so the relay folds it onto `bad_lane` — the
        // closest thing the closed refusal set has. Naming only the other cause sent whoever read
        // it to delete and recreate a git worktree, losing whatever was uncommitted in it, while
        // the one action that mends it — restarting herdr-tg — went unmentioned.
        bad_lane: `The hub would not give this worktree (${LANE ?? 'unnamed'}) a place of its own, so nothing from this session reaches him. If the hub on this machine is older than this bridge, restarting herdr-tg is the whole of the fix; if it is not, this worktree's name is one the hub will not address and nothing here reaches him until it is remade under a plainer one.`,
      }
      // The claim is per WORKTREE now, so a refusal reaching a lane means that worktree is held —
      // the project's own topic and every other worktree may be perfectly free. Naming the project
      // here would point him at up to thirteen candidate holders with nothing saying which.
      const forNow: Record<string, string> = {
        already_claimed: LANE
          ? `Another session in this worktree (${LANE}) is holding the link to his phone.`
          : 'Another session for this project is holding the link to his phone.',
        frame_too_large: 'The last frame was over the size ceiling and was refused, not truncated.',
      }
      const reason = frame.reason as string
      // One refusal blaming another session is an ordinary restart racing its predecessor, and
      // waiting really does mend it. Several in a row is a session that is not going to let go —
      // a bridge orphaned by a session that has already ended, most often — and telling the agent
      // to keep waiting for that is how every message in the new session ends up queued forever.
      heldByAnother = reason === 'already_claimed' ? heldByAnother + 1 : 0
      const stuck = heldByAnother >= 3
      // This one is an INSTRUCTION, so naming the wrong thing sends him to close a session that is
      // not the holder — and the box has already had a stray bridge squat a claim once.
      const why = stuck
        ? LANE
          ? `Another session in this worktree (${LANE}) has been holding the link to his phone across several attempts and is not letting go. Nothing here can reach him until it does: close that session, or if none is open, its bridge outlived it and needs to be ended.`
          : 'Another session for this project has been holding the link to his phone across several attempts and is not letting go. Nothing here can reach him until it does: close that session, or if none is open, its bridge outlived it and needs to be ended.'
        : (forGood[reason] ?? forNow[reason])
      // An unknown reason is treated as temporary on purpose: a hub shipped after this build may
      // refuse for something recoverable, and telling the agent to give up on a guess is worse than
      // telling it to wait.
      link.markDown(stuck || reason in forGood, why ?? `The hub would not take this connection, and gave a reason this bridge does not know (${reason}).`)
      note(why ?? `refused: ${reason}`)
      break
    }
    case 'message':
      deliver(frame.text, {
        message_id: frame.msg_id,
        ...(frame.in_reply_to_ask ? { in_reply_to_ask: frame.in_reply_to_ask } : {}),
      })
      break
    case 'choice':
      // The answer to a question this session asked. It arrives as a message in the agent's own
      // turn — never as a keystroke — which is the whole safety story of this design.
      deliver(`Answer to ${frame.ask_id}: ${frame.option_id}`, {
        message_id: frame.msg_id,
        ask_id: frame.ask_id,
        option_id: frame.option_id,
      })
      break
    case 'ack': {
      // The frame reached the HUB, which is all `send` could see, so its tool call has already come
      // back saying he was reached. This is the hub saying he was not — and it went to stderr,
      // which is exactly the channel that let the original defect run for as long as it did. It
      // has to reach the agent, and the agent's own turn is the only place it can.
      const was = link.frameInFlight(String(frame.ref))
      link.forgetInFlight(String(frame.ref))
      // The wire carries THREE delivery values and this branched on two. `!== 'no'` folded `unseen`
      // into success, so a send the hub could not confirm read as one that landed: the agent's only
      // record said the operator had been asked and an answer was on its way, and no correction
      // ever arrived. Only `yes` is success now, and anything a newer hub sends that is neither
      // `yes` nor `no` is treated as unconfirmed rather than guessed either way.
      if (frame.delivered === 'yes') {
        // `yes` still hides one thing. `clamped` is the only ack reason the hub ever pairs with a
        // successful delivery — it clips on its own side and says so BECAUSE, in its own words,
        // whether anything was lost is a fact the bridge has to be told — and returning here
        // unconditionally meant nothing could ever read it. The operator sees "… (clipped)" on his
        // phone; the agent believed it had delivered the whole thing and went on referring to a
        // part he never read.
        if (frame.why === 'clamped' && was) {
          deliver(
            `He got ${was.what}, but it was too long for one message: what is on his phone ends ` +
              'in "… (clipped)" and he has not read a word after that. Say the rest in a second, ' +
              'shorter message if it mattered.',
            { about: 'he got a shortened version', user: 'the channel itself' },
          )
        }
        break
      }
      if (frame.delivered !== 'no') {
        // `unseen` is the hub refusing to guess. Telegram has no idempotency key, so a send that
        // times out may or may not have landed and there is no way to ask — and re-sending a
        // question with buttons would put two live menus for it on his phone, both tappable
        // forever. So this is the end of it, and the agent has to be told that in words it cannot
        // read as "I asked him".
        note(`the hub could not confirm a frame reached him (${frame.why ?? 'no reason given'})`)
        if (!was) break
        deliver(
          `The hub could not confirm he got ${was.what}. It may have arrived and it may not have, ` +
            'and there is no way to find out — so it will not be sent again, because sending it ' +
            'twice would leave two of it on his phone. Do not tell him you reached him.' +
            (was.askId
              ? ` If the question ${was.askId} never arrived, no answer to it is ever coming, so do not wait for one — carry on without it. Do NOT ask it again: if it DID arrive, its buttons are already dead, and a second copy would leave him two menus with only one of them able to answer. If you must have an answer, say what you need in a plain message and let him type it back.`
              : ''),
          { about: 'he may never have got this', user: 'the channel itself' },
        )
        break
      }
      const why = ackReasons.get(String(frame.why)) ?? 'his phone did not take it'
      note(`the hub did not deliver a frame (${frame.why ?? 'no reason given'})`)
      if (!was) break
      deliver(
        `He never got ${was.what}: ${why}. It will not be tried again.` +
          (was.askId ? ` No answer to the question ${was.askId} is coming, so stop waiting for one.` : ''),
        { about: 'nothing reached him', user: 'the channel itself' },
      )
      break
    }
    default:
      // Unknown kind: logged and ignored, so a hub shipped after this plugin cannot kill the
      // connection just by being newer.
      break
  }
}

await mcp.connect(new StdioServerTransport())
link.start()

// A clean goodbye, best effort and time-boxed. A bridge that hangs saying goodbye turns a tidy
// restart into a SIGKILL, and the hub stays quiet for 90 seconds after a clean `bye` so that a
// context refresh does not buzz the operator's phone.
let leaving = false
function goodbye(): void {
  if (leaving) return
  leaving = true
  try {
    if (link.isUp) link.sendControl({ t: 'bye', reason: 'refresh' })
  } catch {
    /* going away regardless */
  }
  setTimeout(() => process.exit(0), 200)
}

for (const sig of ['SIGTERM', 'SIGINT'] as const) process.on(sig, goodbye)

/**
 * The client that owns this process's stdio has gone, so go too.
 *
 * Claude Code starts this plugin through `bun run`, which makes the process it signals a WRAPPER
 * and this bridge its grandchild: a SIGINT at session exit reaches the wrapper and never arrives
 * here, and the bridge lived on, reparented to init, with nothing left to talk to. That was
 * harmless only while it could not find a token. Now that it can, an orphan authenticates, TAKES
 * the project's claim, and the hub then refuses the operator's next real session — whose every
 * question and message is answered "waiting for the link to come back", forever. EOF on stdin is
 * the one signal that arrives no matter which process the signal went to.
 */
process.stdin.on('end', goodbye)
process.stdin.on('close', goodbye)
