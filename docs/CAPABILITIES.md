<!-- SURFACE, v14, 6 September 2026. v14 is v13 attacked, and three sentences of REQUIRES 1
     change: the switch by repo turns off the project's seed AND every room of it (a room's id
     turns off that room alone) — the first build narrowed "off" to the seed in silence; a title
     is read once per run of the hub, the first time it composes the conversation's name, and
     never re-read while the hub runs; and a rotation writes the repo's copy only where one
     already lives, so an opened project rotates without a token returning to its tree. Also:
     `open` refuses a stale channel copy naming `adopt-secrets --apply` rather than saying
     "already open", a grant whose list cannot be saved takes its slots and secrets back, and a
     room is hidden from every list by its id and its topic rather than by a flag an older hub
     could drop.
     v13 changes REQUIRES 1, which is a promise change and is
     said here rather than left to a diff: enrolment is per CONVERSATION, at a terminal, and the
     secret lives where the channel keeps it — under the hub's own state directory, outside every
     repo — so there is nothing in a working tree for git to commit. `herdr-tg open <repo>` is the
     door that writes nothing into the repo; `herdr-tg grant <repo> --rooms N` mints a seed's
     rooms, complete conversations a dispatcher takes by id; `herdr-tg adopt-secrets` copies the
     secrets of projects enrolled the older way; `enroll` still works and still writes the repo's
     copy, which `remove-repo-secret` takes away per project at the operator's hand. Nothing on
     the wire moved. `docs/CONVERSATIONS.md` is the design and `docs/ATTACHING.md` §5 the ladder.
     v12 is offer 9 attacked. Nothing new is offered and four
     things are said more honestly. Two words join closed sets a reader must treat as open:
     `not-stored` on a file he sent, where nothing was downloaded because the machine had nowhere
     to put the bytes and "send it again" would be a loop with no end; and `no-file-unsaid` on an
     ack, where the file did not go AND nothing could be said in his topic either — the one case
     an adapter must not report as a reason waiting on his phone, and the one that mends itself.
     An outbox file must also be the only name its bytes have, because a hard link passed every
     other check. And the hub's line for a worker that took only his words no longer names a
     cause it cannot see: a short count means the tool server is old or the relay in front of it
     is, and the hub cannot tell.
     v11 BUILDS the up half of offer 9: a file an agent
     attaches to a `say` or a `done` is read from that conversation's outbox — opened following
     no link, checked as what it is and not what it is called — and reaches his phone as a picture
     or a document through the same send accounting as words; when it does not, the words go with
     one line saying so and the agent reads `no-file`. Offer 9 is whole.
     v10 BUILDS the down half of offer 9: what he sends into
     a topic reaches the agent as a path, the bridge's ack counts what it handed on, and an
     adapter older than files earns him one line saying the picture did not follow. The up half
     — an agent attaching a file to a `say` or a `done` — is still the design.
     v9 adds offer 9, files both ways — DESIGNED, not built.
     Bytes never cross the wire; they cross by a bind mount at the same path on both sides, one
     pair of directories per conversation, and the hub mints every path it writes and trusts
     nothing an adapter names. `docs/ATTACHING.md` §14 is the design. Nothing else moved.
     v8 adds the second gate, WHO may speak, beside WHERE the bot
     listens: REQUIRES 5 is new, REFUSES 3 names it, and offer 8 gains a ninth field,
     `allowed_users` — the people a project has let into its own conversations at the terminal,
     never the people who may speak anywhere. Until v8 the only gate was the chat allowlist, so
     anyone who could post in the allowed forum was relayed into an agent's turn and anyone who
     could see a keyboard could tap it; that was safe while the forum held one person.
     v7 is v6 attacked. Offer 8 gains an eighth field,
     `connected_lanes`: `connected` is the project's OWN voice, and a project whose sessions are all
     dispatched into worktrees never has one — so it read `false` while its agent was live, and the
     document had no field that could carry the difference. The switch's bound is said honestly:
     the claim drops within a second and everything queued is refused unsent, but a message already
     mid-send is finished before the close. And §6 of `docs/ATTACHING.md` now gives the live
     refusal's real order — `refused` first, then the `no` acks, then the close.
     v6 builds offer 8: `herdr-tg projects --json` ships, with
     `connected` read from the running hub's own record of its claims and `null` when no running
     hub can vouch for it. Two more things in the same day, neither a new offer: a project can be
     switched off at the terminal (`herdr-tg disable <repo>`) and a LIVE connection is then
     refused `not_enabled` and closed, not only the next one; and the operator's own typed line
     carries a reaction for the stage it has reached — measured free against the send ceiling in
     docs/RATE-PROBE.md §3 — beside the line offer 6 already posts for a refusal.
     v5 is v3 and v4 attacked: the pre-pong hold is 65 frames
     (the queue plus the one frame an adapter puts back on close), the hub sets
     `in_reply_to_ask` from the message he replied to, the refusal line is threaded under the
     line it refuses, and an engine that cannot read typed words refuses them on the wire.
     v4, 5 September 2026, makes offer 3 true across a refused connection: the hub
     holds 64 frames or 4 MiB before the pong (what a conforming adapter may carry into a
     reconnect; it was 256 KiB), and a connection refused before it is live has every frame the
     hub read acked `no` first. Nothing is destroyed unanswered any more.
     v3, 5 September 2026, closes offer 6's gap: an adapter that cannot hand the
     operator's typed words on answers `ack{status: refused, reason}`, and the hub now READS that
     status and says so in the topic he typed in. Nothing on the wire changed; the hub ignored
     the status of every ack before.
     v2, 4 September 2026, added the one thing this file promised and nothing could do:
     a dispatcher-supplied address, `KICKOFF_HUB_ADDRESS`, defined in docs/ATTACHING.md.
     v1, 3 September 2026. What another org may rely on, what it must bring, and what will
     never be built here. Written to be mirrored: kickoff publishes the same three sections in its
     own repo, and a change to either is a diff rather than a letter. A capability listed under
     OFFERS is a promise; one under REFUSES will not be reconsidered by mail. -->

# What this hub offers, requires, and refuses

One Telegram bot, one forum, one topic per conversation. An agent says what it is doing and what it
is asking; the operator reads it on his phone and answers; the answer arrives in that agent's own
turn.

This file exists because two orgs kept proposing each other's non-capabilities. Half of one day's
mail was kickoff proposing things this project will not build, and this project correcting things
kickoff had already decided. A menu ends both.

## How a conversation gets its address — the hub allocates nothing

This is the part most likely to be assumed wrongly, so it is first.

**The hub does not name anything.** A connection presents a secret and, optionally, an opaque
address string. The secret resolves to a project; the string is carried, never interpreted. The hub
has no opinion about what it means — worktree, room, function, anything — and holds no rule about
its shape beyond what a Telegram button and an audit line can carry.

| who | does what |
| --- | --- |
| whoever dispatches | mints the address, and owns its uniqueness within the project |
| the hub | guarantees ONE live connection per `(project, address)`, and one topic per address |
| the hub, on collision | refuses the second arrival, names the reason, and changes nothing |

So a room is addressed by whatever kickoff calls it. Two rooms colliding is kickoff's bug, and the
hub's job is to say so rather than to prevent it.

**How a dispatcher actually supplies one, since 4 September: `KICKOFF_HUB_ADDRESS`.** Until then
nothing could — there was no variable for it, and the Claude adapter derived one from git with no way
to override it. `docs/ATTACHING.md` is the interface: one namespace, the shape rules an address must
keep, and how an adapter checks them before it dials rather than being refused.

**Git is not part of this contract.** The Claude adapter derives an address from the git worktree
name as a *default*, because a developer who opens a session by hand still needs one and git
guarantees the name is unique. That derivation lives in the adapter (`plugins/kickoff-channel/`),
never in the hub, and a dispatcher that supplies its own address overrides it entirely.

## OFFERS — what you may rely on

| # | offer | how you get it |
| --- | --- | --- |
| 1 | **A conversation of its own.** A forum topic per address, created on first LIVE connection and greeted so it appears in the list. | Connect with a secret and an address. |
| 2 | **Exclusivity.** One live connection per address. A second is refused with a reason it can branch on; a dead one is evicted, a live one is never displaced. | `refused{reason}` on the wire. |
| 3 | **Delivery you can trust.** Every frame after `hello` acked exactly once, with three values — `yes`, `no`, `unseen`. `unseen` means it went out and could not be confirmed, and it is never retried. `hello` is answered by `welcome` or `refused` instead, and is the one frame no ack is coming for. A connection refused before it is live — too much said before the pong, a frame over the ceiling, no pong at all — has every frame the hub read acked `no` before the socket closes; the hub holds 65 frames before the pong — the 64 a conforming adapter may have queued plus the one it puts back at the head of its queue on close — which is what one carries into a reconnect. | `ack{ref, delivered, why}`. |
| 4 | **A question with buttons.** Options you mint; a tap resolves against a written record, and a question answered once can never be answered twice. | `ask` up, `choice` down. |
| 5 | **Retirement.** Buttons come off a question that has stopped being open, whoever closed it — which no screen-reading design can do. | `ask_resolved{how}`. |
| 6 | **Typed steering.** The operator's words relayed verbatim into the agent's own turn. Opaque: the hub does not parse them and never lets them name anything. An adapter that cannot hand them on says so with `ack{status: refused, reason}`, and the hub puts the reason in the topic he typed in, under the line it refuses — so a line he wrote never reaches nobody in silence. `in_reply_to_ask` is set when he replied to a question this conversation's live session asked. | `message{text, from, in_reply_to_ask?}` down; `ack{ref, status, reason?}` up. |
| 7 | **An alarm that outlives us.** A watchdog sharing no code, no process and no runtime with the hub. | Nothing; it is always on. |
| 8 | **Read-only inventory.** `herdr-tg projects --json`: one object per conversation that has ever been given a topic or was opened as a project — a vacant room is not listed until something has connected as it — `{project_id, title, repo, enabled, topic_id, connected, lanes:{address:topic_id}, connected_lanes:[address], allowed_users:[user_id]}`, in that order, sorted by title. A room's `repo` is its seed's, so join on `project_id`, never on the path alone. `topic_id` is `null` before a bridge has ever been live. `connected` is whether the project's OWN voice has a bridge on the socket now — a project reached only through its worktrees never does, and `connected_lanes` is which of its addresses are live. Both come from the running hub's own record of its claims and are `null` whenever no running hub can vouch for them — unknown said as unknown, never a `false` nobody could prove. `allowed_users` is the people let into THAT project's conversations with `herdr-tg allow` (REQUIRES 5), sorted; it is never the people who may speak anywhere, who are not in the file this reads. A registry that is there and cannot be read is refused, never reported as empty. No chat id, no path but the repo's, and no person but the project's own. | Run it at the keyboard; `docs/ATTACHING.md` §7 offer 8 has the shape. |
| 9 | **Files, both ways.** *Built 6 September, both directions.* What he sends into a conversation's topic — a photo, a document, a voice note, a video — the hub fetches into a directory of its own, and it reaches the agent as a **path**; what an agent attaches to a `say` or a `done` the hub reads from that conversation's outbox, and it reaches his phone as a picture or a document, through the same send accounting as words. Bytes never cross the wire: they cross by a bind mount at the same path on both sides, read-only down and read-write up, one pair of directories per conversation, so a wall sees only its own conversation's files. The hub mints every path it writes; a name Telegram reports or an agent supplies is data, never a path. It trusts nothing in an outbox — not a link of either kind, not a file whose bytes have a second name, not a file it does not own, nothing outside the directory, nothing over Telegram's own ceilings (20 MB down, 50 MB up, measured) — and both trees have a shelf life, a cap and a bound on entries. One fetch is bounded in time too, by a deadline the hub chose rather than the HTTP client's. When a file does not come through, the words still do and both sides are told, the agent on the wire and the operator in one line in his topic; never a silent drop. When the hub could not put that line in his topic either — the same refusal that shed the file sheds a sentence about it — the ack says so in its own word, so an adapter never tells its agent that an explanation is waiting on a phone where there is none. An adapter older than this offer gets the words, and he is told the file did not follow. | `message{files?}` down, `say{file?}` and `done{file?}` up, `welcome{outbox?}` to say where; `docs/ATTACHING.md` §14 has the mounts, the names and every failure's line. |

## REQUIRES — what you must bring

1. **Enrolment, at a terminal, per conversation.** Admission is the one thing no message can do.
   A conversation is a row the operator minted at a keyboard, and its secret lives where the
   channel keeps one — under the hub's own state directory, outside every repository — so there
   is nothing in a working tree for git to commit. `herdr-tg open <repo>` mints a project's
   conversation and writes nothing into the repo; `herdr-tg grant <repo> --rooms N` mints N rooms
   for it — complete conversations, each with its own secret, its own topic and its own people,
   vacant until a dispatcher takes one by renaming its slot in the book and handing the room's
   id to what it starts (`KICKOFF_HUB_CONVERSATION`); sixteen may be vacant at once, refilled
   only at a terminal, and a taken room is spent, never recycled. A room may be named by whoever
   holds it, through a `title` file the hub reads once per run — the first time it composes the
   conversation's name, which is at its first admission or when its topic is minted — and never
   re-reads while it runs; an unshowable title falls back to the registry's. The older door,
   `herdr-tg enroll <repo>`, is also the rotation: it rewrites the secret where it already lives
   — the channel's copy always, the repo's `0600` `<repo>/.kickoff/hub.token` only where the repo
   already holds one or nothing was enrolled there before, refusing outright if git would commit
   a copy it is about to write — so a project opened with `open` rotates with nothing returning
   to its tree. `herdr-tg adopt-secrets --apply` copies every repo-held secret across once, and
   `herdr-tg remove-repo-secret <repo>` takes a repo's copy away, per project, at the operator's
   hand; from then on rotate only with a build that knows both homes, because an older one
   rewrites the repo's copy alone and `open` will then refuse, naming `adopt-secrets --apply`.
   The switch is part of admission: `herdr-tg disable <repo>` turns the project off — its seed
   and every room of it; a room's id in place of the folder turns that one room off — and a
   connected bridge loses its claim within a second and is refused `not_enabled`, everything it
   had queued is answered `no` unsent, and the connection closes once the one message it may
   have been in the middle of sending is finished; the next to dial is refused at `hello` — and
   `enable`, by the same folder or id, is the only way back. Re-enrolling keeps the switch where
   it was. A room granted at the terminal that has never connected is not listed anywhere; it is
   told from a project by its id and its missing topic, never by a flag.
2. **An address that is unique within its project.** See above. We will not de-duplicate for you.
3. **A bridge that speaks hub-proto** — NDJSON over `AF_UNIX`, nine frames up, six down — and that
   answers a ping. A topic is minted only after `hello`, a settling window and one answered ping,
   because a process that boots and exits in a tenth of a second would otherwise leave an empty
   topic bound forever.
4. **One connection per address.** If two producers must speak for one conversation, join them on
   your side. `adapters/kickoff-hub-attach/` is our implementation — it holds the claim and opens a
   local door that speaks hub-proto unchanged, so a producer needs no second wire contract.
5. **Who may speak, said at a terminal.** The chat allowlist says WHERE the bot listens and
   nothing about WHO. Two lists say who, and no message, tap or command can change either. The
   people who may speak ANYWHERE the bot listens — type at any agent, tap any button, run
   `/projects` — are named in the configuration: `HERDR_TG_ALLOWED_USER_IDS` (or
   `allowed_user_ids`), plus one person per private chat on the chat allowlist, because in the Bot
   API a private chat's id IS its person's id — so an operator whose allowlist already holds his
   own chat sets nothing, and the startup log says which people it found. Read that the other way
   too: a private chat on the CHAT allowlist is a grant of everything, so a teammate's chat added
   there so `/projects` works for him in private may type at every agent and tap every button. The
   startup log says so once per private chat, naming the number and the narrower verb; if that
   person should reach one project, take the chat off the chat allowlist and use `allow`. A
   project's OWN people —
   a customer, a teammate in one room — are let in with `herdr-tg allow <repo> <user>` and heard
   within about a second, no restart: in that project's topic and in its lanes' topics, and
   nowhere else — not another project's, not General, and not the bot's commands, which answer
   with every project's name and state. Everyone else gets silence and one line in the hub's
   audit naming the sender; a reply would confirm something is listening. `disallow` takes it
   back, re-enrolling keeps the list, and a channel post, a bot and an anonymous admin are nobody.

## REFUSES — settled, and not reconsidered by mail

Each is a line, not an omission. Several were paid for.

1. **Naming another project's conversation.** The secret proves only the project. An address can
   only ever reach the repo whose secret the bridge already holds.
2. **Writing into an org repo.** The hub writes to its own state directory and nowhere else. Hub
   state in a git working tree is state an adopter's coordinator commits and pushes.
3. **Treating inbound content as instruction.** What arrives from Telegram SELECTS from what the
   machine already knows; it never NAMES something new. No message, tap or command can enrol a
   project, edit an allowlist, let a person speak, or touch a credential — and a test fails the
   build if the bot or the hub so much as names the setter that lets a person speak.
4. **Typing into a terminal.** The path that read panes and sent keystrokes was deleted, not
   gated, and a guard now forbids naming a write RPC anywhere in the tree.
5. **Choosing a model, an engine, or a repository.** Those belong to whoever dispatches.
6. **Knowing what a lane, a room, a proof or a re-ground is.** All of them reach the phone through
   the conversation primitives. The moment the hub learns one of those words it stops being a
   multiplexer and becomes a second implementation of someone else's discipline.

## OPEN — joint design, neither side should harden yet

1. **Who starts a container.** The hub has no `Command` in the binary today, and seam ④ of
   `docs/INTERFACES.md` proposes a launcher that is simply another adapter: it holds a hub
   connection, offers what it can start as an `ask`, and acts on the `choice` itself. Whether the
   actor should instead be the hub binary is the operator's call and is being brainstormed across
   both orgs. Listed here rather than under REFUSES because it is genuinely open.
2. **What a room needs that a lane does not.** Three answers decide whether one address space is
   enough: does a room outlive the session that made it; can two rooms of one org be live at once;
   must a room's topic survive with nothing connected to it.
3. **The room-map handshake.** Our counter-proposal: the repo file is entirely yours (room name,
   memory scope, charter, engine); topic ids stay in our registry and are exposed read-only; the
   join key is the repo path, which both sides already know. This avoids the hub writing into a repo
   and avoids the fact that a topic id does not exist at enrol time.

## Two measurements that bind both of us

From `docs/RATE-PROBE.md`, taken against the real Bot API on 3 September:

* **The 20/min group ceiling is per CHAT, and topics buy nothing.** Forty sends across four topics:
  twenty accepted, the twenty-first refused with `retry_after: 41`. More rooms and more lanes share
  one budget. A new conversation also costs two of it before its agent speaks — the topic, then the
  greeting — so the practical figure is about six new conversations a minute across the whole forum.
* **`editMessageText` is free.** Thirty edits after five sends, none refused, and a send still went
  through. A surface that updates one message costs one token; one that posts each update costs one
  per update.

## Changing this file

Bump the version in the header comment and say what moved. An offer may be added at any time. An
offer may not be removed without telling the other org first, because they will have built on it.
