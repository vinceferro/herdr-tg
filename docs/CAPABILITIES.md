<!-- SURFACE, v20, 8 September 2026. v20 corrects answer 6 and moves nothing on the wire.
     `herdr-tg doctor` used to answer "is anything watching?" and "would it fire?" out of the
     HUB's two files, so every machine with a hub on it read as armed whether the watchdog had
     ever been installed there or not, and a note a dead hub left behind was quoted in the
     present tense. Both are read now — the watchdog's own four files, and the note's age — and
     answer 6 says so, including the shape `--json` reports them in.

     v19 changes one offer and opens one question, and nothing
     on the wire moves. Offer 7 said the alarm required nothing because it was always on, which
     was true and useless: the stamp behind it proved a Bot API round trip and NOTHING about the
     door agents arrive at, so a hub whose socket never opened answered Telegram every forty-five
     seconds, stamped a green file, and the one thing watching stayed quiet while every agent on
     the box talked to nobody. The stamp is now earned by THREE legs and withheld unless all of
     them were true inside ninety seconds, so a quiet alarm is a statement about the control
     plane: his phone is reachable, the socket is accepting, AND his taps are still being handed
     to this copy of the bot rather than to another one holding the update slot. What no leg
     proves — that a tap which arrived was acted on — is written into the offer rather than left
     to be inferred from silence.
     The hub also writes down which leg failed, in sentences, beside the file the watchdog
     watches; the watchdog reads that ONLY to word an alarm it has already decided to raise,
     never to decide whether to raise one. An alarm is still the whole of what happens: nothing
     here restarts anything. That makes the health worth reading by something other than the
     operator's thumb, which is the new OPEN 6 — the source is settled (the hub's own state
     directory, and every other surface is a reader of it), and which surface a dispatcher builds
     on is not.
     v18 is two additive fields on the wire and one new
     refusal, and it changes two offers. Offer 2 gains the LEASE: the hub mints a number for
     every claim it grants, stamps it on the welcome that grants it and on everything it sends
     that connection afterwards, and a run a later one has replaced is refused
     `stale_generation` — permanently for that run, so what ends it is a new run and never a
     redial. Exclusivity was only ever half true without it: a bridge whose process had died
     was overwritten in the claims map and went on emptying its queue into a conversation its
     successor now owned, and nothing on the wire could tell it so. Offer 4 gains the other
     half of a tap: a bridge that promises to (`confirms: ["choice"]`) answers every `choice`
     `accepted` — the answer is in the agent's turn — or `refused` with a sentence the operator
     can read, and the line on his phone is edited to say which. Until now it said "Sent" and
     went on saying it whatever became of the answer. A tap is confirmed or taken back; the
     keyboard is NOT put back on a refusal, because the question is written down as closed the
     moment he taps and a live menu on a closed question is the double answer offer 4 refuses.
     Both fields are omitted when absent, so an adapter written against v17 is untouched and is
     never nagged about a promise it did not make. `docs/ATTACHING.md` v22 is how an adapter
     invokes both.
     v17 answers a question this file had never asked: WHICH session
     of an engine a conversation is bound to. Offer 6 promises the operator's words reach "the
     agent's own turn", which says nothing about which turn when one engine holds several sessions
     for one directory — and on 6 September that gap was measured on a live box: a launcher had made
     a restricted session for a room and written down which one it was, and the only session the
     adapter's most-recently-active rule could have picked in that directory was an unrestricted
     one, which may edit files, run a shell and commit. No line was typed into it; what was measured
     is that the rule pointed at the wrong session, and the next line typed would have gone there
     with nothing refusing it. The answer is not a frame. Offer 6 is unchanged, nothing on the wire moved, and no
     frame carries a session. It is a division of labour, and OPEN 5 is where it is written down:
     whoever launches an engine owns the session — its lifetime and the identity it is meant to have
     — and the adapter in front of it owns enforcement, speaking to that session and to no other and
     refusing the operator's line out loud rather than guessing which session he meant. The mechanism
     our own adapter uses today, a small file the launcher writes and the adapter reads
     (`docs/ATTACHING.md` §13.10), is a LOCAL adapter detail and **not the contract between the two
     orgs**: it works only because both are processes on one box sharing one filesystem. The durable
     thing is the typed binding itself, which a transport that is not this machine (OPEN 4) would
     have to carry with no filesystem under it. REFUSES 6 gains the matching sentence, because a
     session is exactly the kind of word the hub must never learn.

     A second round the same evening changed the local mechanism twice, and neither change reaches
     this document's surface — both are named here only so OPEN 5 is read against what exists. The
     binding is now ONE versioned shape rather than two, so a launcher and a reader that disagree
     find out instead of meeting in the middle on the operator's words. And the number that orders
     two writers now has a FLOOR the adapter is started with, because a reader's memory of which
     binding it had reached is destroyed by the restart that a stale writer's file outlives — which
     is why OPEN 5's durable form names the floor beside the generation: whoever launches is the
     only party that can say either.
     Amended 7 September, before anything ran, keeping v17: the local mechanism OPEN 5 names had
     been written down in this project's own names, and the launcher — another org's program, and the
     one that writes the file — had already shipped the object in its own. Two shapes with no key in
     common, which would have refused every binding and with it every line the operator typed in a
     room. The launcher's names are kept, and the one thing the durable form gains is the
     **conversation**: the conversation the binding names is checked against the conversation the
     adapter is attached as, which is the only field that catches a binding written for a sibling
     room whose directory and agent would both pass. Nothing on the wire moved, no offer changed, and
     the hub still learns nothing about a session.

     v16 says what CARRIES the frames, which this file had never
     separated from the frames themselves. `AF_UNIX` appeared once, inside REQUIRES 3, as though the
     socket were part of hub-proto. It is not: the frames are one thing and the transport under them
     is another, and the difference decides which offers a connection can have. So REQUIRES 3 now
     says a bridge reaches the hub over a transport THE HUB OFFERS, and names the only one there is
     — `AF_UNIX` at `/run/user/<uid>/kickoff/hub.sock`, admitted on two facts and not one: the
     peer's uid, read off the socket by the kernel, and the secret in `hello`. Offer 9 says files
     are a capability of that transport rather than of the wire — bytes cross by mount, so a peer
     that does not share this hub's filesystem is offered no outbox. That half is BUILT, and it runs
     at admission, the one moment the hub has the connection's identity in its hand; the matching
     half going the other way — withholding a path in a `message` — is not, and offer 9 now says so
     rather than implying the rule is already there in both directions. Every connection today is
     local, so neither subtracts anything from anybody: the first is the rule a second transport
     inherits, the second is work that transport brings with it. And OPEN 4 is new, because this file had no answer at all to "can a
     bridge on another box reach this hub" and the honest one is no: that belongs under OPEN as joint
     design rather than under REFUSES, with our proposal beside it — a gateway on the hub's box
     holding an ordinary local connection, and a second kind of connection identity inside the hub,
     with the meaning of every frame identical on both. Nothing on the wire moved, nothing was built,
     and no offer was withdrawn.
     v15 says two things offers 4 and 5 already had to be true
     for and were not. Offer 5 promised buttons come off "whoever closed it", and an edit Telegram
     refused ended the matter: the keyboard stayed live on his phone, refusing every tap, until the
     ledger dropped it two days later. It is now written down as closed, so the tap is refused, and
     the next thing that can take it off does — the adapter's own `ask_resolved` after his tap, or
     the next session's arrival — signed off with the button he pressed. That makes an
     `ask_resolved` after a tap the second chance rather than a duplicate, which is the opposite of
     what `docs/ATTACHING.md` told adapters until its v19; the two documents now agree. Offer 4
     gains the ordering an adapter may build on: once the hub has handled an `ask_resolved`, no
     `choice` for that ask is sent. And offer 5 says the bound on `outcome` — 500 characters over
     the whole note, with nothing on the wire to say it was clipped. Nothing on the wire moved.
     v14 is v13 attacked, and three sentences of REQUIRES 1
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
| 2 | **Exclusivity, and a lease that says which run.** One live connection per address. A second is refused with a reason it can branch on; a dead one is evicted, a live one is never displaced. Every claim the hub grants also mints a **lease** — a number stamped on the `welcome` that grants it and on everything the hub sends that connection afterwards, which the bridge stamps back on everything it says. It is the hub's to mint and nobody else's: a run that holds one behind the address's own number is refused `stale_generation` when it redials, and one that keeps talking after a later run has been admitted has every frame answered `no` and acted on by nobody — including whatever it drains after its own claim is gone, which is the case that made exclusivity only half true before. That refusal is **permanent for that run**: what ends it is a new run, never a redial. A bridge that holds no lease — every one shipped before 7 September — is told neither word and is fenced by its socket and its pid, exactly as it always was. | `welcome`'s own envelope carries it; `refused{stale_generation}` and `ack{delivered:"no", why:"stale-generation"}` are how it ends. |
| 3 | **Delivery you can trust.** Every frame after `hello` acked exactly once, with three values — `yes`, `no`, `unseen`. `unseen` means it went out and could not be confirmed, and it is never retried. `hello` is answered by `welcome` or `refused` instead, and is the one frame no ack is coming for. A connection refused before it is live — too much said before the pong, a frame over the ceiling, no pong at all — has every frame the hub read acked `no` before the socket closes; the hub holds 65 frames before the pong — the 64 a conforming adapter may have queued plus the one it puts back at the head of its queue on close — which is what one carries into a reconnect. | `ack{ref, delivered, why}`. |
| 4 | **A question with buttons.** Options you mint; a tap resolves against a written record, and a question answered once can never be answered twice. A question you have said is over cannot be answered AGAIN: from the moment the hub handles your `ask_resolved`, every tap on that keyboard is refused. One `choice` can still reach you — a tap already in flight is marked before your frame is handled and delivered after it — so refuse a `choice` for an ask you have resolved. **And a tap is confirmed or taken back.** A bridge that promises it on its `hello` (`confirms: ["choice"]`) answers every `choice` with `accepted` — the answer is in the agent's own turn, and nothing weaker counts — or `refused` with one sentence the operator can read; the line on his phone is edited to say which, taken or not taken and why, and an edit costs nothing where a second message would cost one of the twenty. A bridge that promises nothing gets what every bridge got before: the line says his answer was sent, and he is never told a session failed to confirm something it was never asked to. The keyboard is **not** put back on a refusal — the question is written down as closed the moment he taps, and a live menu on a closed question is the double answer this offer exists to refuse — so he is told the agent has not got his answer instead. | `ask` up, `choice` down, `ack{ref, status, reason?}` back up. |
| 5 | **Retirement.** Buttons come off a question that has stopped being open, whoever closed it — which no screen-reading design can do. An edit Telegram refuses does not end it: the question stays written down as closed, so a tap on it is refused, and the next thing that can take the keyboard off does — your own `ask_resolved` after his tap, or the next session's arrival — signed off with the button he pressed wherever he pressed one. So send `ask_resolved` after a tap on the same question; it is the second chance, not a duplicate, and it cannot overwrite his words. `outcome` is clipped at 500 characters, counted over the whole note the operator reads and not your text alone, and nothing on the wire says so. | `ask_resolved{how, outcome?}`. |
| 6 | **Typed steering.** The operator's words relayed verbatim into the agent's own turn. Opaque: the hub does not parse them and never lets them name anything. An adapter that cannot hand them on says so with `ack{status: refused, reason}`, and the hub puts the reason in the topic he typed in, under the line it refuses — so a line he wrote never reaches nobody in silence. `in_reply_to_ask` is set when he replied to a question this conversation's live session asked. WHICH session, turn or process on the far side takes them is the adapter's to decide and to enforce, never the hub's — the hub names none, and OPEN 5 says who owns that binding and why a wrong answer is not a delivery failure but a misdelivery. | `message{text, from, in_reply_to_ask?}` down; `ack{ref, status, reason?}` up. |
| 7 | **An alarm that outlives us.** A watchdog sharing no code, no process and no runtime with the hub. It watches **three legs, and a stamp is earned only by all three**: the phone line — Telegram answered a real round trip; the agents' door — a connection came out the far end of the loop that accepts them, which a connection the kernel completed into a backlog nobody is reading does not prove; and the update stream — the dispatcher is still going to Telegram for his taps and is not being turned away. The hub touches the file the watchdog watches only when all three were true inside the last ninety seconds, so a quiet alarm now means the whole control plane is up: Telegram answered this hub, the socket accepted a connection, and his taps are still arriving at THIS copy of the bot. A hub that can prove only some of them goes quiet, because withholding the stamp is the only signal a script that reads a modification time can hear — which is also why a watchdog installed before this change starts alarming on a dead door with no update at all, on any box that had already stamped once. **A hub that has never earned a stamp is an alarm too**, and that is the first-run box: no forum configured, or a socket that would not bind, means the hub answers Telegram every forty-five seconds and withholds every stamp for its whole life. There is no stamp there to go stale, so the alarm arms on the hub's note instead — which the hub writes on every tick whichever way the verdict went — and that half needs this version of the script, so install it rather than assuming the copy already on the box covers it. The alarm says which leg failed, because his next move differs: a dead phone line is "nothing you send reaches an agent, and nothing an agent says reaches you", a dead door is his phone still working while "no agent can ask you anything, and nothing you tap reaches an agent", and a held update stream is his phone still working while "nothing you tap is reaching an agent" — the likeliest cause being a second copy of this bot. The third leg is two failures in one shape, because they are opposite: a dispatcher wedged behind a stuck handler STOPS looking and goes stale like any other leg, while one being refused never goes stale at all — so a run of refusals that outlasts the freshness window is watched in its own right, one refused call is a blip, and two blips further apart than a run survives are two blips and not an outage that lasted between them. What no leg proves is written here rather than left to be inferred from silence: **that a tap which arrived was ACTED ON**. A handler that took one and hung looks well here until the wedge backs up far enough to stop the stream being driven — which is narrower than it sounds, since the dispatcher stops pulling as soon as one conversation's worker will not take another update, but it is not the same claim. **The alarm tells him and restarts nothing.** Restarting belongs to whoever dispatches; there is no `Command` in this binary at all (OPEN 1), and an alarm that tried to fix what it watches is one more thing that dies with it. | Nothing to invoke. The hub's own account of which leg failed is `hub.health` in its state directory, rewritten on every tick whichever way the verdict went: one word — `serving` or `not serving` — then one plain sentence for the phone line, one for the door, and one for the update stream. The fourth line is APPENDED, so a watchdog installed before it reads the first three and is unaffected. OPEN 6 is how another program is meant to read it. |
| 8 | **Read-only inventory.** `herdr-tg projects --json`: one object per conversation that has ever been given a topic or was opened as a project — a vacant room is not listed until something has connected as it — `{project_id, title, repo, enabled, topic_id, connected, lanes:{address:topic_id}, connected_lanes:[address], allowed_users:[user_id]}`, in that order, sorted by title. A room's `repo` is its seed's, so join on `project_id`, never on the path alone. `topic_id` is `null` before a bridge has ever been live. `connected` is whether the project's OWN voice has a bridge on the socket now — a project reached only through its worktrees never does, and `connected_lanes` is which of its addresses are live. Both come from the running hub's own record of its claims and are `null` whenever no running hub can vouch for them — unknown said as unknown, never a `false` nobody could prove. `allowed_users` is the people let into THAT project's conversations with `herdr-tg allow` (REQUIRES 5), sorted; it is never the people who may speak anywhere, who are not in the file this reads. A registry that is there and cannot be read is refused, never reported as empty. No chat id, no path but the repo's, and no person but the project's own. | Run it at the keyboard; `docs/ATTACHING.md` §7 offer 8 has the shape. |
| 9 | **Files, both ways.** *Built 6 September, both directions.* What he sends into a conversation's topic — a photo, a document, a voice note, a video — the hub fetches into a directory of its own, and it reaches the agent as a **path**; what an agent attaches to a `say` or a `done` the hub reads from that conversation's outbox, and it reaches his phone as a picture or a document, through the same send accounting as words. Bytes never cross the wire: they cross by a bind mount at the same path on both sides, read-only down and read-write up, one pair of directories per conversation, so a wall sees only its own conversation's files. **This is therefore a capability of the transport, not of the wire.** A path is worth nothing to a peer that cannot open it, so a connection that does not share this hub's filesystem is offered no outbox — no `outbox` in its `welcome`, and nothing for `say{file}` to name, which is already what an adapter reads as "this hub carries no files". Every connection today is local, so this takes nothing from anybody now; it is the rule a second transport (OPEN 4) inherits rather than one that would have to be invented then. **Only the up direction is built.** It is decided at admission, which is the one moment the hub holds the connection's identity; afterwards a claim records the address, the instance and the pid, not who, so there is nowhere today to ask the same question of a `message` going down — and a `message` carrying a path nobody at the other end could open would be a file lost in silence. Whoever builds OPEN 4 builds that half. This is where the debt is written down, not where it is claimed paid. The hub mints every path it writes; a name Telegram reports or an agent supplies is data, never a path. It trusts nothing in an outbox — not a link of either kind, not a file whose bytes have a second name, not a file it does not own, nothing outside the directory, nothing over Telegram's own ceilings (20 MB down, 50 MB up, measured) — and both trees have a shelf life, a cap and a bound on entries. One fetch is bounded in time too, by a deadline the hub chose rather than the HTTP client's. When a file does not come through, the words still do and both sides are told, the agent on the wire and the operator in one line in his topic; never a silent drop. When the hub could not put that line in his topic either — the same refusal that shed the file sheds a sentence about it — the ack says so in its own word, so an adapter never tells its agent that an explanation is waiting on a phone where there is none. An adapter older than this offer gets the words, and he is told the file did not follow. | `message{files?}` down, `say{file?}` and `done{file?}` up, `welcome{outbox?}` to say where; `docs/ATTACHING.md` §14 has the mounts, the names and every failure's line. |

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
3. **A bridge that speaks hub-proto** — NDJSON, nine frames up, six down — **over a transport the
   hub offers**, and that answers a ping. There is exactly one transport today: `AF_UNIX` at
   `/run/user/<uid>/kickoff/hub.sock`, in a `0700` directory, the socket itself `0600`. A connection
   on it is admitted on two facts and not one — **the peer's uid**, which the kernel reports off the
   socket and which must be the hub's own, and **the secret** in `hello`; a connection from any other
   uid is closed with no reply at all, after its `hello` has been read, because a refusal would
   confirm that something is listening. The frames mean the same thing whatever carries them; what a
   transport decides is who may open a connection at all, how a claim is fenced when a bridge dies
   without saying `bye` (here: the pid the kernel reports, checked in `/proc`), and which offers are
   available — files (offer 9) are the local transport's, because bytes cross by mount. A topic is
   minted only after `hello`, a settling window and one answered ping, because a process that boots
   and exits in a tenth of a second would otherwise leave an empty topic bound forever.
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
   multiplexer and becomes a second implementation of someone else's discipline. **A session of an
   engine is the same kind of word.** Which session, turn or process on the far side takes the
   operator's words is the adapter's to decide and to enforce (OPEN 5); no frame carries a session,
   a directory or an engine's name, and `hub-proto` has no field for one. A hub that learned which
   session a conversation meant would have to be told when it was replaced, and it is the wrong
   party to tell.

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
4. **A transport that is not this machine.** Everything above assumes the bridge is a process on the
   hub's own box. Two things depend on that and nothing else does: the uid the kernel reports, which
   is half of admission (REQUIRES 3), and the shared filesystem, which is the whole of files (offer
   9). Neither is a reason the frames could not cross a network; both are reasons the hub will not
   simply listen on one, because it would then be admitting on a secret alone and offering paths
   nobody at the other end can open. Our proposal, written down in advance so that neither side
   designs against a different one: a **gateway** — a process on the hub's box that holds one
   ordinary local connection per remote conversation and carries frames for a peer elsewhere over
   something it authenticates itself — and, inside the hub, **a second kind of connection identity
   beside today's local peer** (in the code, one more variant of the type that today says "this uid,
   this pid").
   What the frames MEAN would be identical on both: every frame after `hello` acked exactly once,
   one live connection per address, a topic per address, a tap resolved against a written record,
   retirement, and a `bye`. What differs is what the hub can prove and therefore what it can offer —
   no uid to admit on, no `/proc` to evict a corpse from (so a remote claim needs a fence of its own,
   probably a lease the gateway renews), and no shared filesystem, so no outbox and no path in a
   `message` — the first of which the hub already withholds, while the second is unbuilt, for the
   reason offer 9 gives. Nothing here is built, no frame changes if it is, and until it exists the honest answer
   to "can a bridge on another box reach this hub" is no.
5. **Which session of an engine a conversation is bound to.** One engine can hold several sessions
   for one directory, and exactly one of them is the conversation on his phone. "Whichever moved
   last" is a guess, and on 6 September that guess was measured pointing at an unrestricted session
   for a room whose launcher had made a restricted one — so the next line typed would have been
   misdelivered, with nothing to refuse it. Two halves, and only one of them is ours. **Whoever launches owns the
   binding**: it makes the session, decides its directory, its agent and what it is allowed to do,
   knows the moment it is replaced, and is the only party that can say which session a room's
   conversation means. **The adapter in front of the engine owns enforcement**: given a binding it
   speaks to that session and to no other, filters what it carries the other way to the same session
   so a stranger's session cannot speak through the topic either, and refuses the operator's line
   out loud — never falling back to a session it chose itself — when the binding is absent, stale,
   or names a session that is not open. The hub is in neither half: it learns nothing about
   sessions, and no frame carries a session.
   What is genuinely open is the **shape of the binding as a thing two orgs agree on**. Ours today
   is a small file the launcher writes and the adapter reads (`docs/ATTACHING.md` §13.10), which
   works only because both are processes on one box with one filesystem; it is a local
   implementation detail and **not the contract between the two orgs**, and neither side should
   build on the file. The durable form is the **typed binding itself** — the **conversation** it is
   for, the session, the directory it is canonically for, the agent it is supposed to be, a number
   saying which generation of it this is, so a stale writer cannot retarget a newer conversation,
   and the **floor** that number is held at for one run of the adapter, because the adapter's own
   memory of how far it had got is exactly what a restart destroys and a stale writer's file
   survives. Both numbers come from whoever launches; an adapter can enforce them and can mint
   neither. The conversation is the field that makes the binding self-describing rather than
   positional: **the conversation the binding names is checked against the conversation the adapter
   is attached as**, and a binding written for a sibling room is refused by its own words — the one
   failure a directory and an agent can both pass, since two rooms of one wall can share both. That
   is what a transport which is not this machine (OPEN 4) would have to carry with no filesystem
   under it, and what an engine that can hold a tag of its own could answer for itself instead of
   anyone writing a file at all.

6. **How a dispatcher reads this hub's health.** Offer 7 turns the alarm into a statement about the
   control plane, which makes it worth acting on by something other than the operator's thumb. What
   the code settles is the **source**: the hub's own state directory, and nothing else.
   `hub.heartbeat`, whose modification time is the alarm and whose staleness is the whole signal, and
   `hub.health` beside it — the word, then one sentence per leg, rewritten on every tick so it can
   never be a stale account of a hub that has since recovered. Of a hub that has since DIED it is
   exactly that, and a reader has to know it: nothing rewrites the file once the process is gone,
   and the sentences carry ages frozen at the moment it wrote them, so a hub killed on Friday still
   says "the phone line answered 12 seconds ago" on Monday. Both readers therefore hold it to the
   same ninety seconds the hub's own tick keeps, and past that it is the last thing a hub managed to
   say and is reported as that. Every other surface is a **reader** of
   those two files and never a second source: a hub that answered "am I healthy?" about itself would
   be answering the one question it is not a witness to, which is why the watchdog does its own
   reading rather than asking the hub, and why `herdr-tg doctor` reports the stamp by opening the
   file too. What is NOT settled is which reader a dispatcher should build on, and we are not
   deciding it by writing it down. **The file** is what the watchdog itself uses, needs no process to
   poll, and survives a hub that cannot run — and it is local, so it is worth nothing to a peer that
   does not share this filesystem (OPEN 4), exactly as a path in a `message` is (offer 9). **A
   command** — `herdr-tg doctor --json`, which carries how long ago the hub stamped, whether the
   alarm is armed and whether it would fire, and, under `watchdog.health`, the word and one sentence
   per leg read straight out of the note — is a shape that could survive a transport which is not
   this machine, and it costs a process per poll and answers only where there is a terminal. Its two
   "is anything watching" answers are read from the watchdog's OWN files — `watchdog.armed`,
   `watchdog.tick`, `watchdog.disarmed` and `watchdog.latch` in the same directory — and never
   worked out from the hub's: a machine the watchdog was never installed on has a hub stamping away
   on it and nothing watching, and reporting that as armed is a controller waiting for an alarm
   nobody can send. The shape says which is which. Under `watchdog.observed` are the facts as read —
   `watchdog_has_run_here`, `armed`, `checked_seconds_ago`, `silenced`, `silenced_seconds_ago`,
   `alarm_already_sent`, `holding_the_hub_a_window` and `window_ends_in_seconds` beside it,
   `stamped_seconds_ago`, `note_seconds_ago`,
   `health` and, when the note is older than that ninety seconds, `last_thing_the_hub_said` in its
   place; under `watchdog.inferred` are the two judgements this command makes of them —
   `the_hub_has_gone_quiet` and `would_alarm`, which is false whenever nothing is watching, the
   alarm has been silenced, nothing has looked for three of its own checks, or it has written down
   that it is giving the hub one more window to come back. `armed`, `stamped_seconds_ago`,
   `would_alarm` and `health` stay where they were at the top of the block, with the corrected
   values, so a reader written before the split still finds them. Both are readable today. Neither is the contract until the
   operator says which, and what does not change either way is the words: they are written in one
   place and read from it.

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
