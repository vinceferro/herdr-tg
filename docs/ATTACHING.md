<!-- INTERFACE, v23, 7 September 2026. The one abstract surface an adapter attaches to: one
     configuration namespace, one wire, one document.

     v23 moves one sentence and no frame. It also finishes §13.10 and §13.11 for the reverse
     agent fence in `kickoff-hub-attach`: two rows for a turn this side cannot place at all, and
     the rule that a QUESTION nobody can place is turned down while a PERMISSION prompt is not —
     a question's refusal says nobody is coming, a permission's says No in the operator's name to
     a tool call he was never shown. Offer 7 in §7 told an adapter the alarm was always on
     and left it at that, which read as "something watches the process". It watches the CONTROL
     PLANE now: the hub touches the watchdog's file only when Telegram answered it, a connection
     came out the far end of the agents' door, and the dispatcher is still being handed his taps
     — all three inside the last ninety seconds. So a hub that has stopped accepting goes quiet
     and his phone buzzes, and so does one whose taps a second copy of the bot has taken; both
     used to be invisible to everything, and the first is the one an adapter that cannot dial is
     sitting in. Nothing to invoke, nothing on the wire, and the gap beside it is unchanged:
     nothing watches YOUR adapter.

     v22 is two additive fields and one new refusal, and it is the first version that lets an
     adapter answer for the operator's thumb. THE LEASE: the hub now mints a number for every
     claim it grants and stamps it on the `welcome`'s own ENVELOPE — there is no payload field
     carrying it and there cannot be one, since a frame is one flat object and the two would be
     one key. A bridge stamps it back on everything it sends, the `hello` it redials with
     included, and a run a later one has replaced is refused `stale_generation`, which is
     PERMANENT for that run: the way back is a new run, never a redial. Until it existed, a
     bridge whose process had gone and whose address a later run took was simply overwritten in
     silence and went on draining what it had queued into a conversation somebody else now
     owned. THE CHOICE ACK: a bridge may promise on its `hello` which of the hub's frames it
     will answer for (`confirms: ["choice"]`) and then answer every `choice` `accepted` or
     `refused` — where `accepted` means the answer is in the agent's turn and nothing weaker.
     Until now the hub queued a tap, told the operator "Sent: <label>", and that line stood
     whatever the far side did with the answer: a session that had already ended, an engine
     that refused the reply, a producer that was never there. His line is now edited to what
     became of it, and §8 gains a thirteenth rule saying which of the two an ack rides on — what
     happened where the agent is, never the fact that a frame was read. Both fields are omitted
     when absent, so an adapter written against v21 puts byte for byte what it always put on the
     wire and is fenced by its socket and its pid exactly as before.

     v22 amended after its review round, same version because the wire did not move: §13.11 is new
     and lists every sentence this command can put under a line on his phone, verbatim; §9 now
     names the two DIFFERENT costs of a producer's lease reaching the hub — the delivery fence on
     an ordinary frame, the address's floor on a `hello` — where it named only the second; and §9's
     fifth point says a door answers for a tap only when it KNOWS, so a producer that goes without
     a word leaves the hub's own "not confirmed" line standing rather than being contradicted by a
     guess.

     v22 amended again after its second review round, same version and for the same reason. §9 says
     `stale_generation` STOPS AT THE DOOR: a producer holds no lease, and one that reads the word
     stops dialling for the life of its session — while the next run of the wall binds that same
     door seconds later, which is precisely what a session that joined the relay needs to find. Its
     socket is ended instead. §13.10 gains the row it was missing, the one reason that belongs to a
     withheld QUESTION rather than to a refused line, and §13.11 gains the reason a `message` is
     refused when the note binds an agent the worker's server cannot resolve — asked before the
     words are posted, because a prompt naming an unknown agent is answered 204 with nothing
     written and the words are simply gone.

     v21 is one adapter's flags and one paragraph of doctrine, and nothing on the wire. The flags:
     `--opencode-binding-file` and `--opencode-binding-generation`, §13.1 and the new §13.10 — the
     file a launcher writes to say which session of its engine this conversation IS, and the oldest
     writing of that file this run may obey, so that a typed line goes to that session and to no
     other. Until them, the watcher chose by recency among the root sessions of the project directory,
     and on 6 September that guess was measured pointing at the wrong session on a live box: the
     only root session listed for a steering room's directory was an unrestricted coordinator, so
     the next line typed would have gone into a turn nobody meant. The doctrine, which is the part a stranger
     should read even with no opencode anywhere near them: whoever LAUNCHES an engine owns the
     session and the identity it is meant to have; the ADAPTER in front of it owns enforcement, and
     enforcement means refusing the operator's line out loud rather than guessing — both ways, since
     a question from an unbound session must not reach his topic either. The file is a LOCAL
     implementation detail of this adapter, and §13.10 says so in the same breath as it specifies it:
     it works only because the launcher and the adapter are two processes on one box with one
     filesystem, and the durable thing is the typed binding it carries, not the file.
     `docs/CAPABILITIES.md` v17 OPEN 5 is the same division of labour from the hub's side, where it
     also says the part that never changes: no frame carries a session, and the hub learns nothing
     about one.

     Five corrections landed in v21 the same day, each of them a way the first build muted a wall or
     let one half through: the generation fence no longer reads a note with NO number as older than
     one that has a number, and closes only behind a note the server confirmed (either bricked typed
     steering for the life of the process); the note is re-read after the server has been waited on,
     so a launcher's rollover during that wait refuses rather than delivering into the session he
     has stopped talking to; the note's key set is CLOSED, which is what the code always did and the
     opposite of what this document first said; a project reached through a symlink is the same
     project; the note must be a regular file no larger than four kilobytes; and the outbound half
     now runs the same checks as the inbound one, saying out loud — once — when a question cannot be
     shown at all.

     Four more landed in v21 the same evening, from a second round on the same build. The flag is
     `--opencode-binding-file` and not a session file, because what the launcher writes is a BINDING
     — the session, the project it is for, the agent it should be running, and which writing of it
     this is — of which the id is one field. There is now ONE shape, a JSON object stamped
     `"version": 1`:
     the bare id on a line is gone, and a launcher still writing one is told that in its own
     sentence rather than told its perfectly good file is unreadable rubbish. The fence gained the
     half that a restart cannot destroy — `--opencode-binding-generation <n>`, the floor the wall was
     started for, said on the command line by the same party that numbers the bindings, under which
     nothing is obeyed for the life of that process however often the file is rewritten; without it
     a watcher that came back from a crash read the stale file it found and took it as the newest
     thing it had ever seen, which is the rollback the fence exists to refuse. And the FILE is now
     proved before a byte of it is believed — the directories it sits under, the link it might be,
     who owns it, who else can write it — because each of those is a way for somebody who is not the
     launcher to choose which session the operator is steering.

     Amended 7 September, before anything ran — and the version does NOT move, because this is
     v21's own shape corrected rather than a new interface for anyone to migrate to. §13.10's binding
     was specified here in names of this project's own — `v`, `session`, `directory` — while kickoff,
     which is the program that WRITES the file, had already shipped an object spelling them
     `version`, `session_id` and `canonical_project_dir`, and carrying a `conversation` and a
     `verified_at` this document had no place for. Two shapes sharing no key at all: every binding a
     launcher wrote would have been refused key by key, and every line the operator typed in that
     room refused with it, in the room, at the worst possible moment. It was caught before a room
     ran and before any such file existed on this box, so nothing has to be migrated and nothing that
     ran is invalidated. The launcher's names are the ones kept, verbatim rather than aliased: it
     shipped first, this repo does not carry two spellings of one thing, and `conversation` is the id
     this architecture already treats as the stable one. Only the version key was asked for in
     return, because a shape nobody has written yet must refuse rather than be half-obeyed. What the
     richer shape buys is a check the old one could not make at all: the conversation the binding
     names is compared with the conversation this attach is attached as, so a wall pointed at a
     sibling room's session is refused by the binding's own words, whatever its directory and agent
     say. `docs/CAPABILITIES.md` v17 is amended the same day and for the same reason.

     Four more corrections the same day, from an adversarial round on that amendment, and the
     version still does not move. A binding naming a conversation this attach cannot yet check is a
     transient refusal and not a settled one, so a question asked in that window is KEPT and offered
     again rather than destroyed — deciding it once cost an agent its whole turn on a keyboard that
     was never drawn. The way out that refusal names is now one the wall could take: a wall pointed
     at its secret by path is told `KICKOFF_HUB_CONVERSATION` in place of `KICKOFF_HUB_TOKEN_FILE`,
     never beside it, because §5 refuses the two together. Every refusal about what is WRITTEN now
     names the part that stopped it — the key it did not know, the value that was the wrong shape,
     and, for a note still written with `v`/`session`/`directory`, the whole rename in one line
     instead of a key at a time. And §13.10's recipe no longer writes `"conversation": ""` on a
     launcher that has no room, which refused the whole binding it had just written.

     v20 says what the pid of §3 is FOR, which this document left to be inferred. It is a local fence
     and nothing else: the hub evicts a dead claim by looking for `/proc/<pid>`, and it accepts a
     release only from the number that took the claim. Nothing is routed by it, and the pid you put
     in `hello` is compared with the socket's for one line in the journal and then ignored. Which RUN
     you are is the `instance`, which you mint — and worth saying because our own two adapters mint
     theirs as `<pid>-<milliseconds>`: a number that means something only to this kernel, travelling
     inside the one string a fleet does read. The pid is also the one identifying thing that exists
     only because both ends are on one machine, which is now written down where it belongs:
     `docs/CAPABILITIES.md` v16 makes the transport a thing of its own — REQUIRES 3 names today's
     one and the two facts it admits on, offer 9 says files are a capability OF that transport, and
     OPEN 4 says what a transport that is not this machine would have to be. Nothing on the wire
     moved.

     v19 inverts one instruction and puts a number on another, both in offer 5, and neither is a
     wire change. The instruction: this document told an adapter NOT to send `ask_resolved` after a
     `choice` for the same question, because a second retirement overwrote the operator's own words.
     A retirement now reads what he pressed before it reads any note, so it cannot — and the case
     the old sentence forbade is exactly the hub's recovery for a keyboard Telegram refused to take
     off after his tap, which is the one menu that is provably still live on his phone. An adapter
     following v18 left it there until the next session arrived. Send it. The number: `outcome` is
     clipped at 500 characters, the whole note counted and not your text alone, with `… (clipped)`
     past it and no `clamped` on the wire to tell you. §6 gains the other half of the same fact — a
     question answered on the phone first is not re-retired, the ack is `yes` either way, and no
     frame ever says which side closed a question. What §6 does NOT promise, because it is not true
     yet: a `choice` for an ask you have resolved can still arrive, so refuse it.

     v18 is a Claude worker run as attach's `--run` child against the live hub, and what that proof
     found. Three things moved, none of them on the wire. §13.4: a `bye` that reaches the hub before
     the pong is a goodbye, not "connected but never answered" — `--check` leaves no refusal behind
     it now, and §13.9's "a new frame" bullet is struck because the frame was already there. §13.5:
     when the ENGINE exits under `--run`, every question the door still holds is withdrawn before
     the `bye`, with an outcome he can read — *the session that asked has ended* — so no question
     keeps its buttons with nothing behind them; a producer's own goodbye while the engine lives
     keeps §9's grace clock, as before. §13.1: a third worked invocation, the Claude worker in a
     wall — a trust dialog that defaults to "No, exit" and that `--check` cannot see, `--channels`
     variadic and therefore last with the prompt on stdin, the plugin's tools deferred behind a
     `ToolSearch` turn even in print mode, and a project directory that must be the real checkout.
     Offer 5 says what `outcome` on a withdrawal is for: the hub shows it under the question, where
     it used to show only "no longer being asked".

     v17 is `docs/CONVERSATIONS.md` built, steps 0–6. A conversation is a row minted at a
     terminal whose secret lives where the channel keeps it — the hub's own state directory,
     outside every repo — and an adapter answers "which conversation am I" by a four-term
     ladder (§5): told — a path, or a conversation (`KICKOFF_HUB_CONVERSATION`, the ninth
     variable, additive) — bound by a link the channel wrote, found on the same walk the legacy
     term takes, the legacy walk to `.kickoff/hub.token`, and a refusal naming a verb. Term 1
     never falls through. The relay's derived door (§9) is keyed on the CONVERSATION, not the
     repository — two rooms in one repo were deriving one door — and a project's door does not
     move when it crosses from the walk to the link. Under `--run` a told conversation is pinned
     into the child and the ninth variable is the one a second engine's overlay must never blank
     (§2, §13.3). Nothing on the wire moved; `hub-proto` gained two doc comments and no byte.

     v16 is v14 and v15 attacked, and it moves four things a stranger's adapter has to know.
     Two new words, both additive and both in sets a reader must treat as OPEN: `not-stored` on
     a `message` file, for the case where nothing was downloaded because this machine had nowhere
     to put the bytes — separate from `download-failed` because "send it again" is a loop with no
     end in it there; and `no-file-unsaid` on an `ack`, for the case where the file did not go AND
     nothing could be put in his topic to say so, which is the one `no-file` an adapter must not
     report to its agent as "the reason is on his phone", and the one that mends itself in a
     minute. §14.2's fold rule changes: an accepted answer whose `files` is short of what the
     frame carried now WAITS for the other producers and sends the largest count, because the
     first answer to arrive is a race between processes and the loser made the hub tell him a file
     was lost that an agent in the same lane was looking at. And §14.3 gains a sixth check on an
     outbox file — exactly one link — because a hard link passed the other five while sharing its
     bytes with a name outside the outbox. Also: the operator's line for a worker that took only
     his words no longer names a cause the hub cannot see, §14.4's too-big sentences say only
     numbers somebody measured, one fetch is bounded by a deadline the hub chose rather than the
     HTTP client's, and the sweep now bounds entries as well as bytes.

     v15 builds the UP half of §14 and closes one hole §14 opened. `welcome{outbox}`, `say{file}`,
     `done{file}` and `ack.why: no-file` are on the wire; the hub opens what an adapter names in
     that conversation's outbox following no link, checks what it OPENED and never the name, and
     uploads it through the same send accounting as words, with one line in his topic whenever it
     will not. The hole: §14.1 claimed no lane can be called `-`, and §2 and §4 said the opposite
     in as many words — the hub would have addressed one, and `-` is the segment BOTH trees write
     a project's own voice under, so a lane of that name was handed the project's own two
     directories. `-` is now `bad_lane`, and §2 and §4 say so. The namespace convention was never
     a rule about the wire: `hello` carries a lane directly, and §14 invites adapters written from
     §6 alone. Also on the wire and previously unpinned: a `say` or `done` carrying a file with NO
     words still sends `text` as the empty string, because a hub older than files requires the
     field and a frame without it is a parse error mid-turn.

     v14 builds the DOWN half of §14: `message{files}` and the bridge's `ack{files}` are on the
     wire, the media tree exists with its modes, shelf life and cap, and both of our adapters carry
     a file into the agent's turn. The up half — `welcome{outbox}`, `say{file}`, `done{file}` — is
     still the design v13 wrote. §14.2's `ack{files}` row now says what is counted.
     v13 adds §14, Files — a DESIGN, not yet built: bytes never cross the wall on the wire, they
     cross by bind mount at the same path on both sides, read-only down and read-write up, one
     pair of directories per conversation. Five additive optional fields, absent never null:
     `welcome{outbox?}`, `message{files?}` and `ack.why: no-file` down; `say{file?}`,
     `done{file?}` and `ack{files?}` up. §6's tables are deliberately unchanged — an adapter
     written from §6 alone is still complete, and carries no file. This is the first section that
     touches `crates/hub-proto`, which §12 says this document does not; §12 is left as written
     because it is about the namespace slice, and §14 says so itself.

     v12 changes nothing on the wire and one field in offer 8: `allowed_users`, a ninth field —
     the people a project has let into its own conversations at the terminal with
     `herdr-tg allow <repo> <user>`, as `docs/CAPABILITIES.md` REQUIRES 5 describes. Behind the
     wire the hub now asks WHO typed or tapped, not only WHERE: a `message` you receive carries in
     `from.user_id` a person who has passed that check, and a tap you receive as `choice` was
     made by one. Nothing an adapter sends can let a person in.

     v11 is v10 attacked. Offer 8 gains an eighth field, `connected_lanes`, because `connected` is
     the project's own voice and a project reached only through its worktrees never has one. §6's
     `refused` row now says the live refusal's real order — `refused{not_enabled}` FIRST, then `no`
     for every frame the hub had read and not sent, then the close — where v10 had the acks before
     the refusal, which is the order at `hello` and not the live one; a stranger who stopped
     reading at the refusal would have lost those acks. Keep reading until the socket ends.

     v10 builds offer 8 and adds one thing to §6 that changes no frame: `herdr-tg projects --json`
     ships (§7, offer 8), and `refused{not_enabled}` can now arrive on a LIVE connection — it is
     what a connected bridge gets, then a close, when its project is switched off at the terminal
     with `herdr-tg disable <repo>`. Every frame the hub had read and not sent is acked `no` before
     the close, and the redial is refused at `hello` the way it always was. Nothing on the wire
     changed shape; an adapter that already handles `not_enabled` at `hello` handles this. Behind
     attach's door (§13) a greeted producer hears the reason before the close ends it — the door
     used to mark its link down first, which ended those sockets with the reason still unwritten.

     v9 is v7 and v8 attacked — two reviewers against the built slices, every change a defect
     reproduced against the running code. The pre-pong hold is 65 frames, not 64: the queue plus
     the one frame a bridge puts back at the head of it on close (§6). The hub now sets
     `in_reply_to_ask` from the message he swiped to reply to, so the reply path offer 6 described
     is taken end to end rather than only on the adapter's side of a fake hub. The line the hub
     posts for refused words is threaded under the line it refuses. `kickoff-channel` refuses
     typed words on an engine that cannot read a channel message rather than writing them into
     one, and attach's door folds several producers' answers into the one the hub hears (§13.9).
     Every request the watcher makes of opencode has a deadline, so one the server takes and never
     answers cannot park every later line; words opencode took and the agent then could not act on
     are said so in the topic. Nothing on the wire changed.

     v8 closes the pre-pong bound §12 used to defer. The hub holds 64 frames or 4 MiB before the
     pong — exactly what a conforming adapter may have queued when it dials, where it held 256 KiB
     and refused every bridge that carried an ordinary backlog into a reconnect — and on every way
     a connection is refused before it is live, every frame the hub read is acked `no` before the
     socket closes. The bridge's half is rule 10 of §8, extended: a frame flushed whole on a
     connection that ends is a frame no ack is coming for, and it is handed back as unconfirmed.
     Nothing on the wire changed.

     v7 closes offer 6's gap and §13.9's first bullet: typed steering reaches an opencode session
     (`kickoff-hub-attach` carries a `message` to `POST /session/{id}/prompt_async`, verbatim, in
     the session the server lists for the project directory), and every `message` is answered on
     the wire with `ack{ref, status, reason?}`. The hub now reads that status — it read the status
     of no ack at all before — and a `refused` becomes one line in the topic he typed in. Nothing
     on the wire changed.

     v6 is v5 attacked — three reviewers against the built `kickoff-hub-attach`, every change a
     defect reproduced against the running code:

       §13.4 — `--check` blessed three environments the start then refused with exit 2 (a relay flag
               on attach itself, a TMPDIR no private door can be made under, an `--opencode` URL
               with no port); it printed a phantom "closed without a word" line after every refusal
               it had already named; it read a 0700 hub directory as "not mounted"; it spoke to a
               person in the register written for an agent. Every sentence in its table is now the
               one the code prints.
       §13.3 — the private door is `mkdtemp`, not a folder named by pid: under `--unshare-pid` every
               wall is PID 2. The pinned `KICKOFF_HUB_TOKEN_FILE` is the path attach FOUND, not `-`.
               The paste block of §2 is for the hand-started shape only; under `--run`, paste
               nothing. The start-time warning this section promised now exists.
       §13.5 — bwrap's init reaps and forwards NOTHING: a signal to bwrap ends bwrap and leaves the
               wall running with the claim held. How a bwrap wall is actually stopped is written
               down, and pinned by a test. A uid-remapped bwrap wall needs a fourth variable.
       §13.2 — "stuck" on `already_claimed` is elapsed time, not three refusals (which was three
               seconds, shorter than a predecessor's stop), and the producers hear "no" for what
               they queued before their sockets end.

     v5 BUILDS §13. `kickoff-hub-attach` is now code — `adapters/kickoff-hub-attach/`, the one
     process that holds the claim, opens the door, watches an opencode server, can be a wall's
     entrypoint, and proves reachability with `--check`. `adapters/fanin/` and
     `adapters/opencode-bridge/` are gone; both folded into it, every check they held moved across.
     The interface changes v4 named are in place: the `KICKOFF_HUB_TOKEN` refusal (a secret handed
     by value), a `KICKOFF_HUB_TOKEN_FILE` that is the secret rather than its path, and the
     container worked example (§10) now uses attach as its entrypoint, so the relay socket is no
     longer something a container must be told. §13 remains, describing what was built.

     v4 added §13 as a design; every reference to `adapters/fanin/` and `adapters/opencode-bridge/`
     outside §13 is now `adapters/kickoff-hub-attach/`.

     v3 is v2 attacked. Every change below is a defect somebody reproduced against the running code
     rather than a rewording:

       §2 — `-` now means "as if this variable were not set", for every variable in the namespace.
            A namespace does not stop a variable crossing an engine boundary, and an opencode-shaped
            config can only overlay, never remove; without a spelling for "ignore this" an inherited
            address or token file was unfixable, and the empty string could not be it. The operator's
            paste block therefore overlays all eight. `KICKOFF_HUB_PROJECT_DIR=""` moved into the
            refuse-when-empty list: the old claim that unset already refuses was false, because
            unset falls through to `CLAUDE_PROJECT_DIR` first. The three socket variables are
            documented as absolute-or-refused, and `<uid>` is now spelled out as the real uid and
            explicitly NOT `$XDG_RUNTIME_DIR`.
       §4 — `bad_lane` has three causes, not two, and `bad_lane` with no `lane` sent identifies one
            of them with certainty. `-` is named as the one address this interface takes for itself.
       §5 — the namespace is inherited exactly as `CLAUDE_PROJECT_DIR` was, and nothing lets an
            adapter check which project it attached as.
       §6 — every field now carries a type, because a wrong one on `hello` is a silent close and
            looks identical to a uid mismatch; one fully worked `hello` and one worked `welcome`.
            `topic_id` is ABSENT, not null. `hello` is not acked.
       §9 — `mainWorkingTree` is defined; it is a git fact and not `KICKOFF_HUB_PROJECT_DIR`. The
            claim that the opencode event bridge cannot attach to a lane relay was true of the fork
            and is not true of the code beside it.
       §10 — a container behind a relay must be told the relay socket, because it cannot derive one.
       §11 — the example does not implement §9's derivation, so it does not prove that claim.

     v2 was v1 built. Written to be implemented by a stranger — someone building an adapter for an
     engine neither org has heard of, holding this file and the `crates/hub-proto` docs and nothing
     else. Everywhere that stranger would still have to read our TypeScript, this file says so out
     loud instead of pretending otherwise.

     It maps offer-by-offer onto docs/CAPABILITIES.md. That file is the menu; this one is how you
     order from it. A change to either is a diff rather than a letter. -->

# Attaching to the hub

## 1. What an adapter is

An **adapter** is a process that holds one connection to the hub on behalf of one conversation. It
proves which project it belongs to with a secret from that project's directory, names which
conversation of that project it is speaking for, and then carries two things in opposite directions:
what an agent wants the operator to see, and what the operator taps or types back.

Everything else about it is yours. The hub does not know what engine you run, what a session is,
what a lane or a room or a proof is, or that you exist between reconnections.

**The smallest thing that counts as one** is a process that:

1. connects to a Unix socket,
2. writes one line of JSON — `hello`, carrying the secret,
3. waits for `welcome`, and then writes one more line — `say`, `ask` or `done`,
4. answers any `ping` with a `pong`, whenever one arrives,
5. reads the `ack` for what it said.

**Steps 3 and 4 are in that order for a reason, and it is not the obvious one.** The hub pings you
immediately and will not create your topic until you have ponged — so against the hub, either order
works. A **relay** (§9) never pings its producers at all, so an adapter that waits for a ping before
it speaks is welcomed and then sits silent for ever behind one. `welcome` is the signal that you may
speak; `ping` is a question you answer whenever it is asked.

That is about sixty lines in any language, and `docs/examples/attach-from-the-document.ts` is those
sixty lines — written from this file alone, importing nothing from this repository, and run against
the real door by `adapters/kickoff-hub-attach/test-two-producers.ts`. It gets a forum topic of its own, delivery
you can trust, and a question with buttons. Everything after §7 is what you add to make it survive a
bad day.

---

## 2. One namespace

**`KICKOFF_HUB_`.** One line of justification: the two paths an adapter cannot rename already spell
it — the socket lives under `/run/user/<uid>/kickoff/` and the secret at `<repo>/.kickoff/hub.token`
— and the prefix is specific enough that a promiscuous environment cannot collide with it by
accident. It is not up for a second discussion.

**One variable is required. The rest are overrides you will probably never set.**

| variable | required | default | what it is |
| --- | --- | --- | --- |
| `KICKOFF_HUB_PROJECT_DIR` | **yes** | — | The directory this adapter speaks for. An absolute path, or the single character `.` meaning "the directory I was started in, and whoever wrote this vouches for it". Any other relative path is refused, because resolving one against cwd is the guess this variable exists to replace. |
| `KICKOFF_HUB_ADDRESS` | no | derived, then none | Which lane of that conversation this is. Minted by whoever dispatched. See §4. |
| `KICKOFF_HUB_CONVERSATION` | no | the repo's own binding, then the legacy walk | Which CONVERSATION this is — `p-` or `c-` and twelve hex characters, the id a terminal printed. Set by whoever dispatched a room. Set and unreadable is a refusal, never a fall-through. Refused beside `KICKOFF_HUB_TOKEN_FILE`: two answers to one question. See §5. |
| `KICKOFF_HUB_TOKEN_FILE` | no | the ladder of §5 | The absolute path to the secret; a relative one is refused. See §5. |
| `KICKOFF_HUB_SOCKET` | no | `/run/user/<uid>/kickoff/hub.sock` | The hub's own socket; an absolute path, a relative one refused. Only for an adapter that holds the claim itself. **`<uid>` is the process's real uid, and it is NOT `$XDG_RUNTIME_DIR`** — see below. |
| `KICKOFF_HUB_RELAY` | no | unset | `1` means this adapter does **not** hold the claim: it attaches to a relay that does. It is the only thing that says so — see §9. Any value but `1` or empty is refused rather than read as "no". |
| `KICKOFF_HUB_RELAY_SOCKET` | no | derived, see §9 | Where the relay's socket **is**; an absolute path, a relative one refused. The relay listens on it; a producer behind one dials it. It does not by itself make you a producer: that is `KICKOFF_HUB_RELAY`, and both sides of a relay read this one. |
| `KICKOFF_HUB_RELAY_DIR` | no | `/run/user/<uid>/kickoff/fanin` | Where derived relay sockets live; an absolute path, a relative one refused. `<uid>` as above. |
| `KICKOFF_HUB_RELAY_GRACE_MS` | no | `90000` | Relay only: how long a producer that vanished has to come home before its open questions come off the phone. |
| `KICKOFF_HUB_TOKEN` | **never** | — | There is no by-value secret and there never will be. Set to anything (other than `-`), it is a **refusal** naming the fix, because the environment is promiscuous and a secret in a variable is a secret in every child's environment. The credential travels as a PATH, `KICKOFF_HUB_TOKEN_FILE`. A `KICKOFF_HUB_TOKEN_FILE` that is 64 hex characters and no path is refused the same way. See §5. |

**A relative path is refused, never resolved.** That goes for all four path variables above. A
relative path is worked out against whichever folder the process happens to be sitting in, which
under a Claude Code plugin manifest is the plugin folder — the one directory this interface exists
to stop guessing from. Nothing listens at the answer, so the link reports itself as merely down and
every message an agent sends waits in a line that never moves.

**`<uid>` is the process's own real uid, read from the kernel — never `$XDG_RUNTIME_DIR`.** Your
platform's documentation will tell you to read that variable, and it is the wrong answer here for a
reason worth one sentence: `XDG_RUNTIME_DIR` is not on kickoff's list of variables that survive its
`env -i` boundary, so an adapter started by a worker would not see it, and the two sides would
derive different paths with neither being wrong. On a box where they agree the mistake is invisible;
under a systemd user unit with its own `RuntimeDirectory`, or in a container, it is not.

Two variables live outside the namespace on purpose:

* **`CLAUDE_PROJECT_DIR`** is Claude Code's, not ours. An adapter may read it as a project directory
  **only when `KICKOFF_HUB_PROJECT_DIR` is unset**. That ordering is a fix, not a detail — see §5.
* **The engine's own endpoint** is seam ②, and the attach interface does not own it. For
  `kickoff-hub-attach` watching opencode it is the value of the `--opencode` flag (§13.1), not an
  environment variable at all; the old `OPENCODE_URL` retired with the bridge. An adapter for a new
  engine names its own flag or variable and documents it beside its own code. Do not put an engine
  endpoint in `KICKOFF_HUB_`.

### The empty-string rule

A variable set to nothing is not a value. opencode substitutes a missing `{env:VAR}` in its config
with the empty string rather than failing, so an empty value here always means *a configuration
asked for something that was not there*.

* **One variable where empty genuinely means "no".** `KICKOFF_HUB_RELAY` unset means "I hold the
  claim myself", which is a real answer, so empty means that too.
* **Every other one refuses outright when it is empty**, and never falls through to its default:
  `KICKOFF_HUB_PROJECT_DIR`, `KICKOFF_HUB_ADDRESS`, `KICKOFF_HUB_CONVERSATION`, `KICKOFF_HUB_TOKEN_FILE`, `KICKOFF_HUB_SOCKET`,
  `KICKOFF_HUB_RELAY_SOCKET`, `KICKOFF_HUB_RELAY_DIR`, `KICKOFF_HUB_RELAY_GRACE_MS`. A configuration
  that set one meant to replace the default, and handing the default back is handing back exactly
  the thing it was replacing. `KICKOFF_HUB_ADDRESS=""` is the case that matters most: the default is
  "speak as the project", and silently taking it for a dispatcher whose variable failed to expand
  puts a lane's words in the project's topic and takes the project's claim. `KICKOFF_HUB_TOKEN_FILE=""`
  is the next: a container told exactly where its secret is goes looking for one instead, and
  attaches with whatever it finds.
* **`KICKOFF_HUB_PROJECT_DIR=""` is in that list and it is the one that reads as an exception.** An
  earlier draft of this document left it out, on the argument that unset already refuses so empty is
  that same refusal. That argument is false in exactly the environment the rest of this interface
  exists for: unset does not refuse on its own, it falls through to the engine's `CLAUDE_PROJECT_DIR`
  first. An adapter that sets the namespace variable from a substitution that failed, in a process
  descended from a Claude Code session, then attaches as **whatever repository that session named**
  — silently, because it really does find a secret there. That is incident 2 of §5, arriving through
  the one variable the rule had exempted.
* `KICKOFF_HUB_RELAY_GRACE_MS` set to something that is not a positive integer is a refusal, not a
  zero. `Number("")` is `0` and `Number("x")` is `NaN`; one withdraws every question instantly and
  the other never withdraws any.

Fail closed and say which variable, in words. A refusal that names the variable costs one line; a
silent default costs a conversation nobody can find.

### `-` means "as if this variable were not set"

The namespace does not stop a variable crossing an engine boundary; it only gives the crossing one
prefix instead of five. **Every `KICKOFF_HUB_` variable is inherited by every descendant of every
process**, including an engine started from inside a session of another engine — and a config of
the shape opencode uses can only *overlay* a variable, never remove one.

So there has to be a spelling for "ignore whatever an outer process left here", and the empty string
cannot be it, because empty is what a failed substitution produces. **The value `-` is that
spelling**, for every variable in the namespace, and an adapter treats it exactly as it treats an
unset one:

| what you write | what the adapter does |
| --- | --- |
| the variable is absent | its default |
| the variable is `-` | its default — identically, no warning, no difference |
| the variable is `""` | refuses and names it (above) |
| anything else | uses it |

A lone hyphen is never a path, never a socket, never a number of milliseconds, and never a
conversation anybody would mint. This interface takes the name for its own use before the hub ever
sees it, and since §14 the hub refuses it as a lane on its own account too: `-` is the segment both
file trees write a project's own voice under, so a lane of that name would be handed the project's
own two directories.

**Any configuration that starts a second engine overlays all eight**, not only the project
directory. Getting one of them wrong is not a loud failure: an inherited `KICKOFF_HUB_ADDRESS` makes
a session speak into a conversation nobody opened for it and take that claim, and an inherited
`KICKOFF_HUB_TOKEN_FILE` attaches it as another repository entirely — silently, because it really
does find a secret. §5 is the two incidents that paid for this paragraph.

### Migration — what changed, and what each old name became

The operator granted a clean break in his own words: *"we can define a clean interface and make
kickoff adopt it and migrate easily."* So the old names are **dropped from the code path, not kept
as aliases.** Carrying eleven aliases into a clean interface is the thing the break exists to avoid,
and every consumer is one edit away. This is a deliberate departure from "list the old names as
accepted aliases" — they are listed here, and marked, but they are not read.

| old | prefixes | becomes | status |
| --- | --- | --- | --- |
| `KICKOFF_CHANNEL_PROJECT_DIR` | 1 of 5 | `KICKOFF_HUB_PROJECT_DIR` | dropped |
| `KICKOFF_CHANNEL_CWD_IS_PROJECT=1` | | `KICKOFF_HUB_PROJECT_DIR=.` | dropped |
| `KICKOFF_CHANNEL_VIA_FANIN=1` | | `KICKOFF_HUB_RELAY=1` | dropped |
| `KICKOFF_FANIN_PROJECT_DIR` | | `KICKOFF_HUB_PROJECT_DIR` | dropped |
| `KICKOFF_FANIN_DIR` | | `KICKOFF_HUB_RELAY_DIR` | dropped |
| `KICKOFF_FANIN_GRACE_MS` | | `KICKOFF_HUB_RELAY_GRACE_MS` | dropped, and its parse becomes fail-closed |
| `OPENCODE_BRIDGE_REPO` | | `KICKOFF_HUB_PROJECT_DIR` | dropped, **and its bare-cwd default dies with it** |
| `KICKOFF_HUB_SOCKET` | | `KICKOFF_HUB_SOCKET` | kept, **narrowed**: it means the hub and only the hub. Pointing it at a relay is no longer how you join one — `KICKOFF_HUB_RELAY_SOCKET` is |
| `KICKOFF_HUB_TOKEN_FILE` | | `KICKOFF_HUB_TOKEN_FILE` | kept, and generalised from one adapter to all of them. It was already the right shape |
| `CLAUDE_PROJECT_DIR` | | unchanged | kept as an **engine-owned** read, demoted below the namespace |
| `OPENCODE_URL` | | `--opencode <url>` | retired, and the endpoint is a flag on `kickoff-hub-attach`, not a variable — seam ② |

Eleven variables across five prefixes became eight in one, of which a normal adopter sets one;
the ninth, `KICKOFF_HUB_CONVERSATION`, was added for rooms and a normal adopter still sets one.

**On our side this is done.** Both adapters that remain — the Claude tool server
`plugins/kickoff-channel/server.ts` and the one command `adapters/kickoff-hub-attach/` — read
`plugins/kickoff-channel/attach.ts`, and it is the only file either of them reads a variable in;
`where.ts` beside it answers only what the MACHINE says once a directory has been named, and
`hub-link.ts` is the one wire (§8). The one other file in the repo that reads these variables is
`docs/examples/attach-from-the-document.ts`, and it does so deliberately — it is a stranger's
adapter, written from this document, importing nothing of ours.
If it ever needed to import `attach.ts`, this document would have failed.
**The edit on kickoff's side** is one: wherever the dispatcher starts an agent, it now exports
`KICKOFF_HUB_PROJECT_DIR` and — this is the new part — `KICKOFF_HUB_ADDRESS`; and, for a room,
`KICKOFF_HUB_CONVERSATION` with the id it took out of the seed's book.

### The operator's own opencode config

`~/.config/opencode/opencode.json` sets three of the dropped names and **may not be edited by
anyone but him.** Between keeping those three working and telling him what to paste, this document
**chooses the paste**, for two reasons: keeping them is the alias-carry the clean break was granted
to avoid, and the failure if he does not paste is loud and safe — the tool server refuses to
connect and every tool call returns a sentence saying the session never said which project it
belongs to. A missed paste is a session that visibly cannot reach him, never a session whose words
go to the wrong topic.

**This paste is for the hand-started shape only** — `opencode serve` started by hand beside a
relay, the shape `kickoff-hub-attach` retires. Under `kickoff-hub-attach --run` (§13) **paste
nothing**: attach pins all nine variables into the engine, the tool server inherits them, and his
file as it stands — the three dropped names are inert, `CLAUDE_PROJECT_DIR: ""` is harmless under a
pinned `KICKOFF_HUB_PROJECT_DIR` — attaches unchanged. The block below would *break* that shape:
its six `-` entries overlay the pinned door, address and token path with "derive it yourself",
which is exactly wrong for a minted address (the tool server derives git's name, looks for another
door, and the agent reads "not said yet" for ever) and for a wall (no git to derive from, so it
refuses outright). §13.3 has the seven measured cases. If he wants one file for both shapes, the
only block that works in all of them is one with no `-` overlays — the file he has.

Replace the `environment` block of the `kickoff-channel` entry with exactly this, and **leave the
`command` line alone** — the path to `server.ts` is unchanged and nothing here renames or moves it:

```json
"kickoff-channel": {
  "type": "local",
  "command": ["bun", "<the same absolute path you already have>/plugins/kickoff-channel/server.ts"],
  "environment": {
    "KICKOFF_HUB_PROJECT_DIR": ".",
    "KICKOFF_HUB_RELAY": "1",
    "KICKOFF_HUB_ADDRESS": "-",
    "KICKOFF_HUB_TOKEN_FILE": "-",
    "KICKOFF_HUB_SOCKET": "-",
    "KICKOFF_HUB_RELAY_SOCKET": "-",
    "KICKOFF_HUB_RELAY_DIR": "-",
    "KICKOFF_HUB_RELAY_GRACE_MS": "-",
    "CLAUDE_PROJECT_DIR": ""
  },
  "enabled": true
}
```

**Every one of the eight is there on purpose, and six of them say `-`.** This one entry covers every
project and every lane he has, and it is also the entry that runs when he starts opencode *from
inside a Claude Code session* — which inherits whatever that session was dispatched with. Overlaying
only the two that this configuration positively needs would leave the other six to arrive from
outside: an address minted for a different conversation, or a path to a different repository's
secret. `-` is how a config that can only overlay says "ignore what you were handed"; see the rule
above.

**The ninth variable, `KICKOFF_HUB_CONVERSATION`, is deliberately NOT in the block, and must never
be added to it with `-`.** It is the one variable that carries a room's identity through to the
tool server: under `--run` attach pins it (§13.3) and blanks the token path, so a config that
blanked it would leave the tool server with only git — which names the folder's own conversation,
never a room — and every word of the room's agent would land in the seed's topic while every line
looked right. Inherited from an outer session it is harmless, for the same reason: a tool server
told a conversation reads that conversation's secret or refuses, and never falls through.

`CLAUDE_PROJECT_DIR: ""` **stays**, and it is the one line here that is not ours. It stops being
load-bearing — `KICKOFF_HUB_PROJECT_DIR` now outranks it — but it costs nothing and it is the second
lock on the incident in §5. One entry still covers every project and every lane: opencode sets the
child's cwd to the session's own directory, which is what `.` means.

**If he pastes half of it**, the failure is loud but the diagnosis used to be wrong: naming the
folder while missing `"KICKOFF_HUB_RELAY": "1"` makes the tool server hold the claim itself and race
his own relay for it, and the sentence an agent read then sent him hunting for "another session" that
was in fact the relay. An adapter that finds something listening on the relay socket for its own
conversation while it is not a producer says **that** instead, and names the line that is missing.

### The running session

A Claude Code channel plugin reloads only when its session does. The session live while this was
written holds the old code and must keep working: it reads `CLAUDE_PROJECT_DIR`, which is still set
by the engine, dials the hub directly, and sends the same `hello` it always sent. Nothing in this
interface changes the wire, the socket path, or the frames, so an old adapter and a new hub, or an
old adapter and a new relay, behave exactly as they did. **A restart to pick up new names is fine
and expected; a running process misbehaving is not, and nothing here does that.**

---

## 3. The five things that identify a connection

| thing | where it comes from | who owns it |
| --- | --- | --- |
| the **project** | the secret, resolved by the hub | enrolment, at a terminal |
| the **address** | the `lane` field of your `hello` | whoever dispatched you |
| the **instance** | a string you mint once per process | you |
| the **pid** | the socket's own credentials, never the number you sent | the kernel |
| the **lease** | the `generation` on the envelope of your `welcome` | the hub, minted afresh for every claim it grants |

The hub builds the conversation's address from **the project the secret resolved to** plus **the
name you sent**, and never from anything else on the wire. That construction is the whole security
argument: a `project_id` you put in `hello` is not consulted, so naming an address can only ever
reach a conversation of the project you already proved you are.

**The lease is the one you do not bring.** The other four are yours or the kernel's before you
dial; the lease is granted, and it says which RUN of the address is speaking. You remember it,
stamp it on everything you send, and mint nothing — §6 has it in full. Two things it is not. It
is not the **instance**: you mint that once per process, it is what a tap and an `ask_resolved`
are matched on, and it can outlive several leases where a lease can be replaced with the
instance unchanged. And it is not §13.10's binding generation, which is one launcher's number
for a note on one box and never touches the wire.

**The pid is a local fence, and never fleet identity.** It does exactly two jobs, both on this
machine: the hub evicts a claim whose holder is gone by looking for `/proc/<pid>`, and it accepts a
release only from the process that took the claim. Nothing is routed by it — not a topic, not a
conversation, not a line anybody looks up later — and the number in your `hello` is compared with
the socket's for one line in the journal and then ignored, so a wrapper that does not know its own
outermost pid is not penalised (§6). Which RUN you are is the `instance`, which you mint and which
the hub compares as a string. So do not put a pid in a fleet record, do not join anything on one,
and do not build a name for a run out of one: a pid is meaningful only to the kernel that issued it,
it is handed out again once that kernel's numbers wrap round, and it exists at all only because this
transport has both ends on one machine (`docs/CAPABILITIES.md` REQUIRES 3; its OPEN 4 is what a
transport without a pid would have to fence with instead). One caution stands rather than being
asserted away: an adapter in its own PID namespace is judged by a number that means something else
on the host side, and that has not been measured here (§10).

---

## 4. The address

### Who mints it

**Whoever dispatches the agent.** Set `KICKOFF_HUB_ADDRESS` and it goes on the wire verbatim. The
hub carries it and interprets nothing: worktree, room, function, ticket number — it has no opinion,
and it will never grow one.

When nobody sets it, an adapter falls back in this order:

1. **A default it can read off the machine.** The two adapters in this repo ask git: in a linked
   worktree they use git's own name for that worktree (the last segment of `--git-dir`), which git
   guarantees unique across a repository. This is a **default for a session nobody dispatched** — a
   developer opening a session by hand — and nothing more. It is not part of this interface; your
   adapter is free to have no such default at all.
2. **Nothing.** Send no `lane` field. That is the project speaking for itself, and it is byte for
   byte what every adapter shipped before addresses existed sent.

Never send `"lane": null`. Omit the field.

**To speak as the project from inside a worktree**, point `KICKOFF_HUB_PROJECT_DIR` at the main
working tree. Facts are derived from the directory you name, not from the process's cwd, so naming
the main tree derives no address and you get the project's own voice. There is no separate "no
address" spelling and there does not need to be.

### What the hub refuses

`refused{reason: "bad_lane"}`, checked after the secret resolves so that a caller with no valid
secret learns only "unknown project":

* **empty** — sending none is how you say "no address"; an empty one is a name the hub cannot
  address and guessing what was meant is how the wrong agent gets an answer;
* **over 64 bytes**;
* **exactly `.` or `..`**;
* **containing `/` or `\`** — so `CEO/steering` is **not addressable**. Adopters minting names freely
  will hit this. Nothing joins an address onto a path today, and the cheapest moment to close that
  door is before anyone is tempted;
* **containing any control character** — a tab or a newline forges a line in the audit file, which
  is one tab-separated record per line and interpolates its subject exactly as given. Any other
  control character reaches a topic title and the journal, where it is invisible and can reorder
  what a person reads.

**One more name is unavailable, and it is unavailable twice over.** `-` means "as if this variable
were not set" (§2), so `KICKOFF_HUB_ADDRESS=-` derives a default instead of naming a conversation:
you cannot ask for one through this interface. And a `hello` that names it anyway, from an adapter
that touches no variable, is `bad_lane` — `-` is the segment both file trees write a project's own
voice under (§14.1), so a lane admitted under that name would be handed the project's own two
directories, reading what he sent the project's own session and writing into the outbox the hub
uploads from under the project's name. The convention alone was not a rule; this is.

**`bad_lane` is permanent.** The same name is refused every time, so retrying it is a spin. Rename or
stop.

There are two more, unrelated causes that arrive wearing the same name, and what a person must do
differs for each — so a single sentence covering all three is a sentence that misdirects two thirds
of its readers.

1. **A hub older than your adapter** does not know the field, and a relay standing in front of it
   folds that condition onto `bad_lane` because the closed refusal set has nothing closer. Restart
   the hub.
2. **A relay holding a conversation you did not name** refuses you the same way (§9). You can tell
   this one apart with certainty: the hub only checks a name that is *there*, so `bad_lane` in
   answer to a `hello` that carried **no** `lane` cannot have come from the hub at all. Name the
   conversation the relay carries, or point at the relay for the project itself.
3. **A name the hub will not address**, which — if you checked the shape rules above before you
   dialled, as this section exists to let you do — is the rarest of the three.

Whatever you say, do not send the reader to delete and recreate a git worktree unless the name you
sent was actually derived from one. That instruction has already cost somebody uncommitted work.

### Uniqueness

**Uniqueness belongs to the minter, and the hub will not de-duplicate for you.** It is safe because
an address is scoped to the project the secret resolved to: two adopters minting `engineering` for
two different projects never collide, because the claim key is the pair. Two of *your* dispatches
minting `engineering` for one project do collide, and what you get is `already_claimed` on the
second — a refusal that names the reason and changes nothing. That is your bug, and the hub's job is
to say so rather than to prevent it.

### The echo, and why you must check it

`welcome` carries back the address the hub admitted. **If you named one and it does not come back,
refuse and disconnect.**

An unknown field inside a known frame is ignored on purpose — it is what lets a new adapter talk to
an old hub at all — but here it means the old hub admitted you **as the whole project**. You take the
project's one claim and its topic, your words land in its conversation, and the project's own
session is then refused. Nothing else in `welcome` can tell that apart from being given a place of
your own: the `project` title is registry-owned and you cannot predict it.

---

## 5. The credential

### The rule

**Configuration travels in the environment. The credential does not.** The environment carries a
*path* to the secret; the filesystem enforces `0600`. Never a token value in a variable, in an argv,
or in an image.

### The two incidents that paid for it

1. **`HERDR_PANE_ID` reached a bridge three levels down** from a terminal that never meant to talk to
   it. The environment is promiscuous: it is inherited by every descendant of every process, across
   engines and wrappers, and a variable set for one purpose arrives somewhere nobody was thinking
   about.
2. **`CLAUDE_PROJECT_DIR` crossed an engine boundary.** An opencode server started from inside a
   Claude Code session inherits it, and its MCP child inherits the server's whole environment — so
   the tool server resolved **another repository's secret**, silently, because it really did find
   one. opencode can only overlay a variable, never remove one, which is why the operator's config
   sets it to the empty string deliberately.

That second one is why `KICKOFF_HUB_PROJECT_DIR` now **outranks** `CLAUDE_PROJECT_DIR` instead of
sitting below it. A dispatcher's explicit word beats an engine's ambient one, and the blanking trick
becomes a second lock rather than the only one.

### The namespace is inherited too, and that is the third incident waiting to happen

Nothing about the prefix `KICKOFF_HUB_` stops it crossing the boundary incident 2 crossed. A Claude
Code session dispatched with `KICKOFF_HUB_ADDRESS=engineering` that starts an opencode server hands
that server, and its MCP child, the word `engineering` — for a session nobody dispatched, in a
different worktree, speaking for a conversation nobody opened. Behind a relay it is worse than a
wrong topic: the producer derives its relay socket from the address, so the two sides derive
different sockets and everything queues against a door nothing is listening at.

**The escape is `-`, and §2 is where it is specified.** Two things follow that a stranger has to be
told rather than left to work out:

* **A config that starts a second engine overlays the whole namespace**, not the one variable it
  cares about. Six of the eight in the operator's own entry say `-` for exactly this reason.
* **An adapter cannot detect that it got this wrong.** There is nothing on the wire that tells you
  which project you attached as beyond the registry title in `welcome`, which you cannot predict
  (§7, offer 8). The address echo (§4) catches a wrong *conversation*; nothing catches a wrong
  *project*. A wrong token is invisible from the inside — it finds a real secret, the hub admits it,
  and the words land in another project's topic. Get the token file right at the door, because
  there is no second chance to notice.

### Where the secret is, and how it is found

A conversation is a row the operator minted at a terminal, and its secret lives **where the channel
keeps it**: under the hub's own state directory — `$XDG_STATE_HOME/herdr-tg`, or
`$HOME/.local/state/herdr-tg`, exactly as the hub derives it — one directory per conversation:

```
<state>/conversations/<id>/secret               0600   the bytes a bridge presents
<state>/conversations/<id>/title                0600   display only, optional, written by whoever holds a room
<state>/by-repo/<sha256(canonical main tree)[..16]>   one line: the id that repo defaults to
```

`herdr-tg open <repo>` mints a project's row and writes nothing into the repo; `herdr-tg grant <repo>
--rooms N` mints N rooms, each its own conversation with an id of the shape `c-` and twelve hex
characters. The older door, `herdr-tg enroll <repo>`, still writes `<repo>/.kickoff/hub.token` at
mode `0600` as well — refusing outright if git would commit it — and `herdr-tg adopt-secrets` copies
every such secret across, once. **No message can open, grant or enrol anything**; they are
terminal-only acts, on purpose, and `open`, `grant` and `remove-repo-secret` refuse unless a person
is at the keyboard.

An adapter finds the secret by a **ladder of four terms, in strict order**, and ours is
`secretFor` in `plugins/kickoff-channel/attach.ts`:

1. **Told** — one of two spellings, and both at once is refused as two answers to one question.
   *A path:* `KICKOFF_HUB_TOKEN_FILE` is used verbatim, with no search — the container answer,
   and the only way in from a machine where git is not a fact. *A conversation:*
   `KICKOFF_HUB_CONVERSATION=<id>` reads `<state>/conversations/<id>/secret`; the id is
   shape-checked before it becomes a path segment. Either way, **told but unreadable is a
   permanent refusal — never a fall-through** — because the repo's link and the repo's token may
   both be right there, and a fall-through under a failed bind-mount is a session speaking as a
   conversation nobody opened for it.
2. **Bound.** A link the channel wrote, `<state>/by-repo/<sha256(realpath of a folder))[..16]>`,
   names the conversation that folder defaults to. It is keyed on the folder the operator OPENED,
   which is routinely not the top of the repository, so it is looked for on **exactly the walk
   term 3 takes**: upward from `KICKOFF_HUB_PROJECT_DIR` to the top of the working tree, then the
   one crossing to the main working tree, and without git the named directory alone. A link is
   therefore found wherever a token could have been — a folder with no git, a project opened
   below the top of a monorepo — and two sibling projects in one repository stay distinct because
   the walk stops at the first link it meets. A lane worktree crosses to the same main tree, so a
   lane and its main tree read one credential by construction rather than by a special case. A
   symlinked checkout is resolved through the link before hashing, because the hub hashes the
   canonical path.
3. **Legacy.** The same walk, to `<repo>/.kickoff/hub.token`. Kept for the whole migration so a
   bridge from before conversations existed and one from after both work; it goes when the last
   repo copy has been taken away.
4. **Nothing.** Refuse permanently, **naming a verb and never a path**: the hint used to carry a
   folder, and in a lane worktree that folder was the worktree, so following it minted a second
   project for the same repository and moved a live conversation somewhere new on his phone.

**A secret the channel keeps that the hub refuses** — `unknown_project` on a term-1 or term-2
secret — is most often a stale copy: a rotation typed with a `herdr-tg` from before conversations
existed rewrites the repo's copy alone. The sentence an agent reads then names
`herdr-tg adopt-secrets --apply` (which copies the repo's current bytes across) for a bound
secret, and the grant for a room; `herdr-tg open` on such a project refuses with the same verb
rather than saying "already open".

**Resolve it afresh on every connection attempt, never once.** The operator may run `herdr-tg open`
or `enroll` while your adapter is running, and that is the documented recovery from
`unknown_project`. An adapter that caches "no secret" and stops retrying makes that recovery a lie.

### What the secret proves

**The project, and nothing else.** It does not name a conversation, it does not carry a display name,
and the `project_id` field beside it is never read. Put anything there; ours puts
`unknown-until-the-hub-says`, which is honest about what it is for.

---

## 6. The handshake, as a sequence

Everything here is one JSON object on one line, LF-terminated, UTF-8. Every object carries `v`
(protocol version, currently `1`, a **JSON number**), `id` (a **string**: opaque, yours, monotonic
per connection) and `t` (a **string**, the kind), flat, in one object — plus, once the hub has
granted you one, `generation` (a **JSON number**), which is your lease and is described below:

```
{"v":1,"id":"f1","t":"say","text":"built it"}
```

**Types are not decoration here.** The tables below give one for every field, because getting one
wrong on `hello` produces the single worst failure mode in this system: a first line that will not
decode is not refused, it is *closed in silence* — the same thing you see when your uid does not
match (§10). No `refused` frame, no log you can read, just a socket that accepted, took a line and
went quiet. A stringly-typed environment makes `"pid":"12345"` the natural guess and it is fatal, so
here is one worked `hello`, complete:

```
{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"<64 hex chars from the token file>","instance":"12345-1757000000000","repo":"$HOME/project","pid":12345,"lane":"engineering"}
```

### The steps

| # | who | what | if it goes wrong |
| --- | --- | --- | --- |
| 1 | you | `connect()` to `KICKOFF_HUB_SOCKET`. | Nothing listening: the hub is not running. Retry with backoff. |
| 2 | hub | Takes your peer credentials off the socket. **A connection from another uid is closed with no reply at all** — a refusal would confirm something is listening. The close happens after it has read your `hello`, so what you observe is a socket that accepted, took a frame, and went quiet. | You must run as the same user as the hub. |
| 3 | you | `hello` — **within 5 seconds**, or the connection is dropped in silence. | |
| 4 | hub | Admits or refuses. **A first frame that is not `hello` is refused `unknown_project`. A first line that will not decode closes silently. A first frame over the ceiling is refused `frame_too_large`.** | |
| 5 | hub | `welcome{project, lane?, topic_id?, limits}`. Check the address echo (§4). **Its own envelope carries your lease** — the `generation` field, below. | |
| 6 | hub | `ping`. **Its own envelope `id` is the nonce.** | |
| 7 | you | `pong{ref: <the ping's id>}` — **within 5 seconds**. | No pong: you never become live, no topic is created, the connection ends. This is deliberate — a channel plugin that is not allowlisted boots and exits in a tenth of a second, and would otherwise leave an empty topic bound forever. |
| 8 | hub | Creates the topic and greets it, so it is visible in the operator's list. | |
| 9 | both | Frames, each answered by exactly one `ack` — **every frame after `hello`.** `hello` itself is never acked; `welcome` or `refused` is its answer. Build a watchdog on "a missing ack means something" without that exception and it tears down every healthy connection at the first frame. | |
| 10 | you | `bye{reason}`, then **wait for the kernel to take it** before exiting. | See the note on `bye` below. |

**Anything you say between step 3 and step 7 is kept and replayed**, bounded at 65 frames — the 64
a conforming adapter may have queued when it dials (§8, rule 10) plus the one frame it was half-way
through writing when the last connection ended, which goes back to the head of its queue — so a
full legal backlog carried into a reconnect is held whole. Over that bound the connection ends without becoming
live, and **every frame the hub read is acked `no` (`why: too-fast`) before the socket closes**. What
the kernel took and the hub never read gets no ack, and that is yours to account for at close (rule
10). An adapter that opens with a question is the whole point of the product, so the buffer exists;
it is not a licence to stream into it. Before 5 September the bound was 256 KiB and the frames went
down with the socket, unanswered — measured: sixty-four messages across three refused connections,
zero corrections to the agent.

### The frames

**Nine up.** `hello` · `say` · `ask` · `ask_resolved` · `done` · `beat` · `ack` · `bye` · `pong`

A `?` marks a field that is **omitted when it has no value** — never sent as `null`, in either
direction. Every field is a JSON **string** unless this table says otherwise.

| frame | fields | buzzes his phone |
| --- | --- | --- |
| `hello` | `project_id`, `token`, `instance`, `repo`, `pid` (**number**), `lane?`, `confirms?` (**array** of strings) | — |
| `say` | `text`, `hint?` (`prose` \| `output`) | no |
| `ask` | `ask_id`, `text`, `options?` (**array** of `{option_id, label}`, both strings) | **yes** |
| `ask_resolved` | `ask_id`, `how` (`answered` \| `withdrawn` \| `timeout`), `outcome?` | no |
| `done` | `text` | **yes** |
| `beat` | `state` (`working` \| `idle` \| `blocked` \| `done`), `note?` | no |
| `ack` | `ref`, `status` (`accepted` \| `refused`), `reason?` | — (a `refused` for one of his `message`s, or for one of his taps, puts its `reason` in his topic; see offer 6 and "saying what became of his answer" below) |
| `bye` | `reason` | — |
| `pong` | `ref` | — |

`hello` deliberately carries **no display name**: the title comes from the hub's own registry, because
an adapter that could name itself could claim another project's topic. `repo` and `pid` are for the
audit record and for a human reading it, never for routing — and the hub uses the pid from the
socket's credentials, not the one you send, so a wrapper that does not know its own outermost pid is
not penalised. §3 says what that pid is for: a fence on this machine, never fleet identity.

An `ask` with **no options** is still a question — one he answers by typing rather than tapping.

An `ask_resolved` for a question the operator answered from his phone is neither a mistake nor a
second retirement, and you should send it (§7, offer 5). Where his tap already took the keyboard
off there is nothing left to edit and the hub edits nothing; where Telegram refused that edit, this
is what finally takes it off, signed off with the button he pressed rather than your `outcome`. It
is acked `yes` either way, and nothing in the ack or in any later frame says which side closed the
question — so an adapter cannot learn from the wire that its question was answered on the phone.
And do not read the ack as a fence. A tap is two steps in the hub — the record is marked, then
the `choice` goes out — and your `ask_resolved` can be handled between them, acked `yes`, with the
`choice` following it. **Refuse a `choice` for an ask you have already resolved**; that is the
adapter's half of answered-once, and it is the only thing that closes the window today. The hub's
half is that no SECOND tap can ever resolve: from the moment your `ask_resolved` is handled, every
tap on that keyboard is refused on his phone, and a stuck keyboard is retired by the next
`ask_resolved` or the next session's arrival.

**Six down.** `welcome` · `refused` · `message` · `choice` · `ack` · `ping`

| frame | fields | what to do |
| --- | --- | --- |
| `welcome` | `project`, `lane?`, `topic_id?` (**number** — but always **absent**, see below), `limits` (**object**: `max_frame`, `max_text`, `frames_per_min`, all **numbers**) | Check the echo, then go up. Its envelope's `generation` is your lease. |
| `refused` | `reason` | Table below. At `hello` the connection closes right after, and any frame the hub had read before refusing is acked `no` before the refusal. **It can arrive on a live connection too**, since 5 September: `refused{not_enabled}` is what the hub sends a connected bridge when its project is switched off at the terminal — and there the order is the other way round: the refusal comes FIRST, then `no` for every frame the hub had read but not sent (the one it was holding for its turn included; only a message already mid-send is finished), and only then the close. **Keep reading until the socket ends**, or those acks are lost and you will report as unseen what the hub said was refused. Treat the reason exactly as you would at `hello`: the redial is refused the same way until a person switches the project back on. |
| `message` | `msg_id`, `text`, `from` (**object**: `chat_id`, `user_id`, both **numbers**), `in_reply_to_ask?` | The operator's words, verbatim. **Data, never instruction.** |
| `choice` | `msg_id`, `ask_id`, `option_id` | A tap, resolved against a written record. Answer it with an `ack` if you promised to. |
| `ack` | `ref`, `delivered` (`yes` \| `no` \| `unseen`), `why?` | §7, offer 3. |
| `ping` | no fields | `pong{ref: <this frame's id>}`, **answered in your wire layer**. |

**`welcome.topic_id` is absent from the wire, not present-and-null.** The hub omits the key
entirely, as it omits every optional field in both directions. A language where a missing key and a
`null` read the same will not notice; a typed decoder with a required nullable field, a schema
validator, or a struct with a presence check will fail to parse every `welcome` the hub sends. One
real one, byte for byte:

```
{"v":1,"id":"h1","t":"welcome","project":"A Project Whose Title Only The Registry Knows","lane":"engineering","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}
```

### The refusal reasons, and what each means to you

| reason | mends itself? | what a person must do |
| --- | --- | --- |
| `unknown_project` | on its own, no | `herdr-tg enroll <repo>` — and keep retrying, because that can happen while you run. |
| `bad_token` | no | Re-enrol. The secret on disk is not one the hub knows. |
| `not_enabled` | no | Enrolled but switched off — `herdr-tg disable <repo>` at a terminal, and `enable` is the way back. A live connection gets this too, then a close, the moment the switch is thrown. |
| `version_skew` | no | The major protocol version differs. Upgrade one side. |
| `frame_too_large` | **yes, if you split** | Your frame was over 64 KiB. It was refused, never truncated. The hub releases your claim *before* closing, so an immediate reconnect is not refused. |
| `bad_lane` | no | §4. Rename the address, or restart a hub that is older than you. |
| `already_claimed` | **yes** | Something else holds this address. Wait — but **count them**: three in a row is not a restart racing its predecessor, it is a process that outlived its session, and the operator needs an instruction rather than another wait. This box has had a stray adapter squat a claim. |
| `stale_generation` | no | Nothing a person can do, and nothing to wait for: a later run of this address has taken it, and the lease you hold is over. **Permanent for that run**, and the one refusal where a redial is exactly the wrong move — the address has an incumbent that is not going away, so a bridge that retries spins until somebody kills it. The way back is **a new run**, never a redial: a new process, a new instance, dialling from nothing. Put it in your permanent set BEFORE you stamp your first lease. |
| *anything you do not recognise* | **treat as temporary** | A hub shipped after you may refuse for something recoverable. Giving up on a guess is worse than waiting. |

### The lease — which run of the address you are

The hub admits one live connection per address (§4), and a run that has been replaced is not a rival
for the address: it is over. The number that says which run you are is the **lease**.

**It arrives on the `welcome`'s own envelope**, as `generation`, and nowhere else. There is no
payload field carrying it, in either direction, and there cannot be one: a frame is one flat JSON
object, so a `generation` inside a payload and the envelope's own `generation` are the same key — a
peer that set both would emit a duplicate key, which no decoder will read, and a peer that set only
the payload's would have it silently swallowed by the envelope's. This protocol met that collision
once already, when `ping` and `pong` tried to name their nonce `id`. **No payload field in either
direction may be named `generation`**, and a test on our side fails the day one is.

A `welcome` whose envelope carries no `generation` is a hub that fences nothing — every hub before
7 September. You then hold no lease, stamp none, and nothing below applies to you.

What you do with it:

1. **Remember it for the connection**, exactly as you remember the address echo. It is
   connection-scoped like everything else in §8 rule 6 — but it is the one thing you carry into the
   next connection, on the `hello` you redial with.
2. **Stamp it on every frame you send after it, the `hello` you redial with included.** One field,
   both directions, one meaning: *the generation this frame's sender holds for this address*.
3. **Never mint one and never invent one.** A lease is the hub's to grant. `0` is not a lease: it is
   read as silence at both ends, deliberately, because `welcome.generation ?? 0` is what a bridge
   written in the language every bridge is written in puts on the wire before it has been welcomed,
   and a zero that reached a fence would lose every comparison it was ever in — fencing that run for
   ever, on every restart, with no wrong-looking value anywhere.
4. **Treat `stale_generation` as permanent**, per the table above, before you stamp your first one.

What the hub does with it, so you can tell a fence from a fault:

* **On a redial, only a lease BEHIND the hub's own number for the address is refused**; one ahead of
  it is admitted. A number this hub never granted means its own record was lost or restored from
  elsewhere, and refusing it would lock a project out of its own hub with nothing a phone could do
  about it. The hub then mints past whatever you hold, so you are never fenced by your own claim.
* **The backlog you carry into a redial is safe.** You wrote those frames before you could possibly
  have read the new `welcome`, so every one of them carries the OLD number and none can be
  re-stamped. The hub refuses a frame stamped AHEAD of the lease it granted this connection, never
  one behind — so a run that lost its socket, redialled and flushed sixty-four queued frames is
  delivering them, not being fenced for them.
* **Once a later run has been admitted**, everything the replaced run says is answered
  `ack{delivered: "no", why: "stale-generation"}` and acted on by nobody — including whatever it is
  still draining after its own claim is gone, which is the case the fence exists for: before this,
  a run whose process had died went on emptying its queue into a conversation its successor owned.
  A run evicted while it was gone is sent `refused{stale_generation}` and its connection is ended.
* **Only where the word can be read.** Both `stale_generation` and `stale-generation` are sent only
  to a connection that has stamped a lease on at least one frame. A bridge old enough not to know
  the word reads an unknown refusal as temporary (§8 rule 8) and an unknown `why` as "his phone did
  not take it" — which would tell an agent that the operator's messaging app refused a frame his
  phone never saw. Such a connection is fenced by its socket and its pid alone, exactly as it was
  before any of this existed.

### Saying what became of his answer

A `choice` is his thumb, and until v22 nothing on the wire could say what became of it: the hub put
the frame in your outbox, his phone said `Sent: <label>`, and that line stood whatever the far side
then did with the answer — a session that had already ended, an engine that refused the reply, a
producer that was not there any more.

**Promise it on your `hello`**, with `confirms`: the hub's own frames you will answer with an `ack`,
named by their `t`. Today there is one, `["choice"]`. A name this hub does not send is a promise
about nothing, which is the same as no promise; **an empty list is no promise** either, exactly as
saying nothing is, because an adapter that builds the list by filtering writes `[]` every time it
has nothing to promise. Absent is what every adapter shipped before v22 sends, and it keeps their
behaviour untouched for ever.

**Answer it** with the frame you already answer his typed words with — `ack{ref, status, reason?}`,
where `ref` is the `choice`'s envelope id:

* `accepted` — **the answer is in the agent's turn.** Not "I read the frame", not "I handed it to
  something": the operator is about to be told his answer was taken, so say it when it was.
* `refused` — it is not, and it will not be later. `reason` is one short sentence in HIS words,
  because he reads it: no status code, no session id, no engine's name, no component of yours.

What he sees. Each of these is an **edit of the line he is already looking at**, never a new
message: Telegram charges a chat twenty messages a minute and charges nothing for editing one it
already has, and a tap happens precisely when the forum is busy.

| what you say | what his phone shows |
| --- | --- |
| `accepted` | `Taken: <label>` |
| `refused` | `Not taken: <label> — <your reason>. The agent has not got your answer.` — and the question itself is signed off `not taken — the agent could not act on your answer` |
| nothing, having promised | `Sent: <label>. The session has not confirmed it took your answer.`, once, after twenty seconds — the question's own shelf life on his phone |
| nothing, having promised nothing | `Sent: <label>`, and he is never nagged about a promise you did not make |

Three things that follow, and each has bitten something here:

* **The first answer is the one he reads.** A second `ack` for the same tap changes nothing, and an
  `ack` naming a frame the hub never sent you writes nothing at all — you cannot put text in his
  topic by answering for something you were not handed.
* **Your `ack` is a frame after `hello`, so the hub acks it.** That ack is bookkeeping; do not
  answer it, or you have written a loop.
* **Answer late rather than not at all.** The twenty-second line is not the end of the matter: an
  answer that arrives a minute afterwards still corrects it, and the record is kept until the queue
  needs the room.

### `bye`, honestly

Send it, and wait for the kernel to take it before exiting — `process.exit()` on the same tick loses
it to a short write.

But do not believe the folklore, which this document is correcting: **on the current hub, a `bye`
after the pong changes nothing.** It is acked and otherwise ignored; the hub does not announce a
departure to the operator at all, so there is no buzz for it to suppress and no 90-second quiet
window. It is worth sending anyway for two real reasons: **a relay does act on it** (§9 — it detaches
you and starts your grace clock), and it is the only clean end-of-connection signal the protocol
has, so a hub that comes to need one will need it from you.

Before the pong it changes one thing, and only for the record. A connection that says `bye` and
closes without ever answering the `ping` is released as one that said goodbye — every frame the hub
had read is still acked `no`, the `bye` included, no `refused` follows, no topic is made — and not
as one that "never answered", which is the reading the hub used to give it. That is the difference
`--check` (§13.4) rests on.

---

## 7. The capability map, offer by offer

Against `docs/CAPABILITIES.md` v1. Its numbering, not renumbered.

### OFFERS — how you invoke each one

**1 · A conversation of its own.** Connect with a secret and an address (§4, §5, §6). The topic is
minted after your pong and greeted so it appears in the operator's list.
*Gap:* `welcome.topic_id` is **absent** — the key is not on the wire at all — and **no later frame
ever carries it**, so an adapter can never learn which topic it got. Nothing needs it — you must not
address anything with it — but if you were hoping to print it in your own log, you cannot.

**2 · Exclusivity.** You get it whether you ask or not: one live connection per `(project, address)`.
Invoke nothing; branch on `refused{reason}`. A dead claim is evicted (the hub checks `/proc/<pid>` of
the incumbent's socket), a live one is never displaced. To have two processes speak for one address,
see §9.

**3 · Delivery you can trust.** Every frame you send **after `hello`** gets **exactly one**
`ack{ref, delivered, why}` — including `beat`, `pong` and `bye`, so that a missing ack means
something. **`hello` is the exception and it is not acked at all**: the hub consumes it in the
handshake, before the acking loop exists, and answers it with `welcome` or `refused` instead. Do not
let a watchdog built on "a missing ack means something" wait for one; it will tear down every
healthy connection at the first frame, and the sentence above is the trap. Invoke it by keeping a
map from the envelope id you minted to what that frame *was*, in plain words, and by acting on all
three values:

* `yes` — it landed. But check `why` too: **`yes` with `why: "clamped"` means it arrived clipped.**
  The operator sees a message ending in `… (clipped)`; you would otherwise go on referring to a part
  he never read.
* `no` — it did not land, and it will not be retried. `why` is one of `too-fast`, `no-topic`,
  `telegram-refused`.
* `unseen` — it went out and could not be confirmed, and **it is never retried.** Telegram has no
  idempotency key, so re-sending a question would leave two live keyboards for it, both tappable
  forever. Folding `unseen` into success is a defect this project has already shipped and fixed;
  branch on `=== "yes"`, never on `!== "no"`.

*A rule, not a gap:* there is no ack timeout on the wire, and an ack names an id minted for ONE
connection — so a frame the kernel took whole on a connection that then ends is a frame no ack is
ever coming for. **Release your in-flight map on every close and tell whoever was waiting that its
fate is unknown** — not that it was lost (the hub may have delivered it and lost the ack with the
socket), and never re-send it (a question that did land has live buttons; a second copy is two
menus). The hub keeps its half: on every way a connection is refused before it is live, every frame
it read is acked `no` first, so what stays unknown is only what the kernel took and the hub never
read. `hub-link.ts` does the bridge's half (`onUnanswered`).

**4 · A question with buttons.** `ask{ask_id, text, options}` up; `choice{msg_id, ask_id, option_id}`
down. You mint both ids and the labels; a tap resolves against the record written beside the message,
never against a button's position, and **a question answered once can never be answered twice**.

Two limits that are yours to respect, and both refuse the *whole* ask:

* an `option_id` may not contain `|` and must be **62 bytes or fewer** (Telegram's 64-byte
  `callback_data` ceiling, less a 2-byte prefix);
* `text` over `limits.max_text` (3500 **characters**, counted as characters, not bytes) is clipped and
  acked `clamped`.

When an option id will not fit, the ack is `no` / `telegram-refused` and the hub also posts a plain
line into the topic telling the operator to answer where it is running — because an agent blocked on
a question that never arrived is the failure this product exists to prevent.

**And a tap is confirmed or taken back, if you promise to say which.** `confirms: ["choice"]` on
your `hello`, then one `ack{ref, status, reason?}` for every `choice` — `accepted` when the
answer is in the agent's turn, `refused` with a sentence he can read when it is not. His own
line changes to say which. Promise nothing and you get what every adapter got before v22:
`Sent: <label>`, and no nagging. The keyboard is **not** put back on a refusal — the question was
written down as closed the moment he tapped, and a live menu on a closed question is the double
answer this offer exists to refuse — so he is told the agent has not got his answer instead.
"Saying what became of his answer" in §6 has the table of what he reads.

**5 · Retirement.** `ask_resolved{ask_id, how, outcome?}`. Send it whenever a question stops being
open **for any reason** — including one answered at the terminal, which is the frame no screen-reading
design could ever produce. The buttons come off the message. On a withdrawal, `outcome` is the
sentence he reads under the question in place of *no longer being asked* — say why, in his words:
attach sends *the session that asked has ended* when the engine that asked is gone (§13.5). Leave it
out and he reads the hub's own sentence, which is true and says nothing.
**`outcome` is clipped at 500 characters, and nothing on the wire says so.** The bound is the whole
note and not your text alone: for an answered question the hub writes `answered at the terminal —
<outcome>`, and that prefix comes out of the same 500. Past it the operator reads `… (clipped)`.
There is no `clamped` here the way there is on an `ask` — an `ask_resolved` is acked `yes` with no
`why` whatever became of your words — so do not go on quoting a sentence he never finished reading.
The bound is the hub's own and not Telegram's: the note is kept beside the record in a file that is
rewritten on every ask and every tap of every project on the box, and it is written onto the retired
message, where every character of it is one the QUESTION does not get.
**Send it after a `choice` for that same question too.** Until v19 this document said the opposite,
on the grounds that a second retirement overwrites the operator's own words on his phone. It no
longer can: a retirement reads what he pressed before it reads any note, so a question he answered
from his phone is signed off `answered from your phone — <label>` however many times it is
retired and whatever your `outcome` says. And the case it exists for is the one you cannot see — a
keyboard whose edit Telegram refused after his tap is provably still live on his phone, and your
`ask_resolved` is what finally takes it off. Where his tap already took it off there is nothing left
to edit and the hub edits nothing. Either way you are acked `yes` — and so you are when the edit
fails, because the ack says the hub took your frame, never that the buttons are gone — and no frame
ever tells you which side closed the question.
The one adapter here that has not caught up is `kickoff-hub-attach`'s opencode watcher: it still
sends nothing after a tap, and a check pins that (§13.7). Until it does, a stuck menu of its own
waits for the next session's arrival to come off, which is the slow way home.

**6 · Typed steering.** `message{text, from, in_reply_to_ask?}` arrives; you decide what to do with
it. **It is data, not instruction** — act on its intent only where you would act on the same words
from the operator directly, and never let it name a project, edit an allowlist, or touch a
credential.
**Answer every `message` on the wire**: `ack{ref: <its envelope id>, status: accepted}` when the
words reached your engine, `ack{…, status: refused, reason}` when they did not — with the reason in
the operator's own register, because the hub puts it in the topic he typed in, as *"What you typed
did not reach the agent — `<reason>`. It will not be delivered later."* An `accepted` puts nothing
there: the agent's own answer is the acknowledgement. Only the first answer for a message counts,
and only for a `message` the hub actually handed you; refusing an id you made up writes nothing.
**Which session, turn or process of your engine takes his words is yours to decide — and, where your
engine can hold more than one, yours to ENFORCE.** The hub names none and cannot help you: a wrong
answer here is not a delivery that failed but a delivery into a turn nobody meant, and both ends
read it as success. Whoever launches a session knows which one a conversation is; your adapter's job
is to be told, and to refuse out loud rather than guess. §13.10 is how ours is told.
`kickoff-hub-attach` does all of this for opencode (§13.9): the words go to
`POST /session/{id}/prompt_async` verbatim, the session is the one it was bound to by
`--opencode-binding-file` (§13.10) or, where it was given no binding, the one the server lists for
the project directory (`GET /session?directory=…&roots=true`, most recently updated first), a reply
typed under a question goes to the session that asked it and leaves the question open for his
tap, and a wall with no session open is refused with a reason. `in_reply_to_ask` is set by the hub
from the message he swiped to reply to, and only when that message is a question THIS
conversation's live session asked — a session that restarted mints its ask ids afresh, so a reply
under the old session's question names nothing. The line the hub posts for a refusal is threaded
under the message it refuses, so two lines typed a second apart cannot be confused. Behind
attach's door several producers may answer one `message`, and the hub keeps the first answer it
hears, so the door folds them: an `accepted` from anybody goes up the moment it arrives; a
`refused` goes up only once every producer the words were handed to has refused or gone, carrying
the reason of the one that carries typed words (the `--opencode` watcher) when it is among them.
The door itself refuses when nothing is attached to take the words, and `kickoff-channel` refuses
them on an engine that cannot read a channel message rather than writing them into one. Before
5 September the hub read the status of no ack at all, and an adapter that dropped his words did so
in silence — say so on the wire now, and he is told.

**7 · An alarm that outlives us.** Nothing to invoke; it is always on. It arms the first time it
sees either of the hub's own files — the heartbeat it stamps, or the note it writes beside it on
every tick — and it shares no code, no process and no runtime with the hub. Both, because a hub
whose door never opened has never earned one: on that box there is no stamp to go stale, and arming
on the stamp alone left the very state you would want to hear about unwatched for the box's whole
life. What it watches is the **control plane**, not merely the process: the hub touches that file
only when Telegram answered it AND a connection came out the far end of the agents' door inside the
last ninety seconds, so a hub that has stopped accepting goes quiet and the operator is told — which
is the state you are in when your dials fail and nobody can tell you why. Withholding the stamp is
the whole signal, so there is no word to read and nothing for you to send.
*Gap, and it is worth knowing:* it watches the **hub**. Nothing watches *your adapter*. The hub tells
the operator nothing when a connection goes away, so an adapter that dies quietly is a conversation
that simply stops. If your agent going silent needs to be noticed, that is your job.

**8 · Read-only inventory.** `herdr-tg projects --json`, at a terminal, built 5 September. One JSON
array, one object per project, sorted by title, fields in this order and no others:
`{project_id, title, repo, enabled, topic_id, connected, lanes, connected_lanes, allowed_users}`.
`topic_id` is `null` until a bridge of that project has been live once — a topic does not exist at
enrolment. `allowed_users` is `[<user_id>]`, sorted: the people let into THAT project's
conversations with `herdr-tg allow`, and never the people who may speak anywhere, who are not in
the file this reads.
`lanes` is every address of the project that has ever been given a topic, `{<address>: <topic_id>}`.
`connected` is whether the project's OWN voice has a bridge on the socket right now — a project
whose sessions are all dispatched into worktrees never has one, so it is `false` for such a project
while its agent is live — and `connected_lanes` is which of its addresses are live right now,
`[<address>]`, sorted. Both come from the one place that knows — the running hub's claims map,
which the hub writes down for this command on every arrival and departure — so both are `null`
whenever no running hub can vouch for the answer: unknown, said as unknown, never a `false` nobody
could prove. A registry that is there and cannot be read is refused with a non-zero exit and
nothing on stdout, never reported as an empty inventory. No chat id, no path but the repo's, and no
person but the project's own.
**There is still no invocation over the wire** — an adapter cannot ask the hub anything; the
inventory is read by whoever dispatches you, at the keyboard, and the join key is the repo path.
*And the safety gap stays:* the address echo (§4) catches a wrong conversation, and nothing catches
a wrong **project**. Attach with a token file belonging to some other repository and everything
looks right from inside — a real secret, an admission, a title you could not have predicted anyway.
§5 says how a token file arrives wrong without anybody choosing it.

### REQUIRES — how you satisfy each one

**1 · Enrolment, at a terminal, per conversation.** `herdr-tg open <repo>`, `herdr-tg grant <repo>
--rooms N`, or the older `herdr-tg enroll <repo>`. Not something your adapter can do, arrange, or
work around; §5. A room is handed to what you start as `KICKOFF_HUB_CONVERSATION=<id>`.

**2 · An address unique within its project.** §4. Set `KICKOFF_HUB_ADDRESS`, keep it unique yourself,
keep it inside the shape rules, and check the echo.

**3 · An adapter that speaks hub-proto and answers a ping.** §6 for the sequence, §8 for the twelve
ways a wire implementation gets it wrong. Answer `ping` **in your wire layer**, not in your
application layer: liveness is what keeps your claim, and it must not depend on anything upstairs
being awake.

**4 · One connection per address.** §9.

---

## 8. Writing the wire — the thirteen rules

There is **one** implementation of this wire in the repo — `plugins/kickoff-channel/hub-link.ts` —
and every file that speaks the wire shares it: the Claude tool server, and attach's `relay.ts`,
`opencode.ts` and `check.ts`. A test fails if any of them starts writing its own again, or if any
other file under `adapters/` or `plugins/` so much as mints a `hello`.

That rule was bought. `adapters/opencode-bridge/bridge.ts` carried a fork of an early version for a
day, and in that day it drifted by twelve invariants — every one of them a defect that had already
been found and fixed on the other side, still live in the copy because nobody had reason to look at
it twice.

For you, that list is more useful as a checklist than as history. If you implement this wire in
another language, these are the thirteen things to get right, and each of them cost somebody a
real debugging session:

1. **The trailing newline is appended in exactly one place.** Omitting it made the far side hang
   forever with no error and no close — measured at 5.01 s, zero bytes read, connection still open.
   Only a client-side timeout catches that.
2. **Read exactly one line. Never to EOF.** Reading to EOF surfaces a reset *after* a good frame has
   arrived, and loses the frame with it.
3. **Bound one frame, not the conversation.** A cumulative ceiling ends a healthy stream in silence
   that reads as a disconnect the peer never performed. 64 KiB, terminator included, restored after
   every complete line.
4. **Short writes are normal, and the kernel drops what it will not take.** Measured: 131 MB offered,
   245 KB accepted, the rest gone and reported as sent. Nothing leaves your queue until its last byte
   is accepted, and you need a **drain handler** — a frame the kernel refused must not sit until you
   happen to send another one. An engine's prompts are sporadic; a permission ask can sit unsent
   indefinitely with the agent blocked.
5. **A half-written frame is re-sent whole, from the start.** Never resumed. Keeping the byte offset
   across a reconnect writes the *tail* of a frame whose head died with the old socket — and that
   headless line is the first thing the hub reads, which closes **silently**, with no refusal, so you
   never learn why.
6. **Drop connection-scoped state on close.** The `hello`/`pong`/`bye` you owe *this* connection die
   with it; a stale one written onto the next connection is two `hello`s the protocol does not have.
7. **Reset your backoff when you are `welcome`d, not when you connect.** A hub that accepts and then
   refuses is otherwise redialled at a flat one second, for ever. `already_claimed` is not permanent,
   so a squatting claim becomes a permanent 1 Hz hammer.
8. **An unknown `refused` reason is temporary.** Treating it as permanent means a hub shipped after
   you stops you for the life of the process.
9. **"No secret" is temporary too.** The documented recovery is `herdr-tg enroll` *while you are
   running*. An adapter that sets a permanent flag and never retries makes that recovery impossible —
   and says so only on a stderr nobody reads.
10. **Your send function must return what actually happened**, and the caller must use it. "Written to
    a live socket", "parked in a queue for a link that has never come up" and "refused for size" are
    three different things, and every one of them was once reported to an agent as success. Bound the
    queue, and when the link goes down for good, **hand the queued frames back** to whoever was told
    they were on their way. And when a connection ENDS — for any reason — every frame the kernel took
    whole that has no ack yet will never get one: hand those back too, as *unconfirmed*, and never
    re-send them. One that did reach the hub was delivered, and a second copy is a second message on
    his phone, or a second live menu for one question.
11. **Nothing but `hello`, `pong` and `bye` may go out before `welcome`.** A connected socket proves
    only that something accepted; the hub can still refuse and close, and a frame written into a
    doomed connection is a frame you reported delivered and then threw away.
12. **Answer `ping` in the wire layer**, and never depend on a caller upstairs being awake for it.
13. **An `ack` for something the hub handed you answers for what became of it, never for having
    read it.** His typed words and his taps are the two frames with a person waiting on the far
    side of them, and he is shown what you say: `accepted` means the words or the answer are in
    the agent's turn, `refused` means they are not and will not be. Answer for what happened
    where the agent is — an adapter that acks on receipt tells him his answer was taken by a
    session that had already ended, and he walks away believing it. And if you promised
    `confirms: ["choice"]`, answer every `choice` you are handed, exactly once: a promise nobody
    keeps is worse than no promise, because the hub holds his receipt open waiting for it and
    then tells him the session never confirmed something it did.

Two more that are not defects but are easy to miss: an **unknown frame kind is ignored**, not fatal;
and **a line that will not decode is one bad frame**, not a dead peer — the hub survives one from you
and you must survive one from it, because that is exactly what a peer one version ahead sends.

---

## 9. Two things behind one address

The hub admits one live connection per address. If two processes must speak for one conversation,
**join them on your side** — the hub never learns there were two, and this is not a seventh hub
capability.

`adapters/kickoff-hub-attach/` is our relay — the door it opens is `relay.ts` inside it. One process
per addressable thing holds the claim; producers attach to it over a local socket that **speaks
hub-proto unchanged**, so a producer needs no second wire contract and nothing in it changes but the
address it dials.

* The **relay** gets `KICKOFF_HUB_PROJECT_DIR` and, if the conversation has one,
  `KICKOFF_HUB_ADDRESS`. It listens on `KICKOFF_HUB_RELAY_SOCKET`, or on the derived path below.
* Each **producer** gets the same `KICKOFF_HUB_PROJECT_DIR` and the same `KICKOFF_HUB_ADDRESS`, plus
  `KICKOFF_HUB_RELAY=1`. That flag, and only that flag, is what makes a process a producer;
  `KICKOFF_HUB_RELAY_SOCKET` says where the door is and is read by the relay too, so setting it
  alone does not put you behind one. Without it the path is derived, below. A producer must
  **never** fall back to dialling the hub when it cannot find its relay: that is two writers racing
  for one claim, which is the whole thing the claim exists to prevent. Refuse and say so.

**The derived path**, documented so a second implementation can meet ours:

```
<KICKOFF_HUB_RELAY_DIR>/<first 16 hex chars of sha256(conversation + "\0" + address)>.sock
```

with `address` the empty string when there is none, and `conversation` the id the ladder of §5
would key on, worked out WITHOUT reading a secret: `KICKOFF_HUB_CONVERSATION` when told; else,
with git a fact here, the id the first link on §5's walk names; else `p-` and the first twelve hex
characters of `sha256(realpath(folder))` for the folder that walk finds a `.kickoff/hub.token` in,
which is the id the hub's own registry minted for it and the id its link names once it has one —
so a project's door does not move the day its link is written, whether it was opened at the top
of its repository or below it; else the same formula for the main working tree. With no git and
no conversation told there is no derived door at all: a wall is told one, or given a private one
(§13.3). Keyed on the conversation and never on the repository, because two rooms in one repo are
both top-level with no lane and a door keyed on the repo gave both the same one. Hashed because
`sun_path` caps at 108 bytes; NUL-joined so that no two `(conversation, address)` pairs can be
spelled two ways onto one socket. The formula's first operand was the main working tree's path
in v16; a producer built on v16's derivation must be told the door, or rebuilt.

**`mainWorkingTree` is a git fact, and it is NOT `KICKOFF_HUB_PROJECT_DIR`.** It is the top of the
repository's *main* checkout — `dirname` of what `git rev-parse --git-common-dir` answers, resolved
against the directory you asked about, which is the repo top in a main checkout and the main
checkout's top when you are in a linked worktree. `KICKOFF_HUB_PROJECT_DIR` is routinely a subfolder
of that (§5 says why the secret search goes upward), and pointing it at a subfolder derives a
*different* socket from the one the relay is listening on. Both sides then look right and never
meet.

So: **both sides derive it from facts they already have only if both sides have git.** An adapter
that cannot run git plumbing — a container (§10), or any language you would rather not shell out
from — must be **told** the path with `KICKOFF_HUB_RELAY_SOCKET`, and that is the supported answer
rather than a workaround. `docs/examples/attach-from-the-document.ts` does exactly this and refuses
to guess; it does not implement the derivation, so the worked example proves the relay path but not
this formula.

### What a relay does that a pipe does not

It answers `hello` itself with the `welcome` it holds (address echo included, so your
refuse-rather-than-impersonate check keeps working unmodified); rewrites envelope ids, because two
producers both mint `f1` and an ack naming the wrong frame tells the wrong agent its message was
lost; **namespaces `ask_id`**, because the hub resolves a tap by ask id and without this a tap on one
agent's question is delivered to the other; answers the hub's `ping` itself so a wedged producer
cannot cost the address its claim; shares the queue by how many producers are actually attached; and
**ends your connection when its own link to the hub drops**, because the wire has no way to un-welcome
anybody and a producer whose link is up would otherwise say "sent" for a message nothing carried.

It **strips a producer's lease and stamps its own**, because a lease belongs to the connection it
was granted to and the door holds exactly one. Strip `generation` from a producer's envelope
where you already rewrite `v` and `id`, and forward the `welcome` down verbatim so a producer
that wants to check its echo still can. This one matters more than it looks, and it costs two
different things depending on which frame carries the number:

* **On an ordinary frame** — a producer's `ask`, `say` or `ack` forwarded up with its own number
  still on it — the hub's delivery fence refuses every frame stamped **higher** than the lease it
  granted this connection, and a producer's invented or borrowed number is exactly that. The
  agent's question never reaches the phone, and what comes back is
  `ack{delivered: "no", why: "stale-generation"}`, which an adapter renders to its agent as a newer
  run having taken its place. False, with no wrong-looking value anywhere to find it by.
* **On a `hello`** — which is the one frame a door must never forward at all — it would be worse
  still: the hub mints past whatever lease an arriving run claims to hold, so the floor for the
  whole address would rise, and the project's real run would be refused `stale_generation` on its
  next redial and locked out of its own conversation with nothing on his phone to say why.

Expect nothing of a producer that ignores the lease entirely; most will, and
`docs/examples/attach-from-the-document.ts` is one of them.

And **the word stops at the door**. `refused{stale_generation}` is a statement about the lease the
door holds; a producer behind it holds none, and never did. Forwarded down, it says something about
the producer that is not true — and it is the one word the table above tells a producer to act on
for good, so a producer that reads it correctly stops dialling for the life of its session. The run
is over; the wall is not, and under a restart policy the next run of it binds the same door seconds
later. A producer that is not the door's own child is still there to find it. So **end their
sockets** instead: a close is what this wire already means by "the door went away — wait, and dial
again", which is exactly what the next run needs them to do. Put the fact in your own journal, where
whoever runs the wall reads it.

It also takes on the lifecycle job that standing in front of the hub took *away* from the hub. The
hub retires a dead asker's questions from the `instance` in `hello` and the pid on the socket — behind
a relay both are the relay's, for every producer, for ever. So:

* **You are known by the `instance` in your own `hello`, never by your socket.** Keep it for the life
  of your process; change it when you restart. A producer that reconnects under the same instance is
  the same voice and still gets the tap it is waiting on.
* **When your socket goes and nothing comes back under your name within the grace window** (default
  90 s), your open questions are withdrawn and their buttons come off. The one thing that does not
  wait for the window is the engine itself ending under `--run`: then every question the door holds
  is withdrawn at once, with the reason he reads, because nothing is coming back (§13.5).

### Five things a relay does not inherit from the hub

Written down here because none of them is stated anywhere else:

1. **It never pings its producers.** A producer's liveness behind a relay is unchecked, and a
   producer's own `pong` is dropped as an answer to a ping nobody sent.
2. **It refuses a producer whose address does not exactly equal the one it holds** — `bad_lane`,
   including a producer that sends **no** address against a relay holding one. An adapter that
   cannot be told an address therefore cannot join a relay that holds one; every adapter in this
   repo can be, through `KICKOFF_HUB_ADDRESS`, so what remains is a configuration mistake rather
   than a hole: set the same address on the producer and its relay and it attaches. This is also
   the one refusal a producer can attribute with certainty — see §4, cause 2.
3. **It checks your secret against the one it authenticated with**, so a producer still needs the
   token file even though the relay holds the claim. Defence in depth over the socket directory's
   permissions, and it costs nothing because the frame already carries it.
4. **It answers `version_skew` to a `hello` with no `instance`.** A small overload of a reason that
   otherwise means a protocol mismatch; do not be confused by it, and always send an `instance`.
5. **A promise to confirm is the CONNECTION's, and the promise belongs to the connection that
   made it.** `confirms` is on the door's own `hello`, made once for the whole door and read by
   the hub off the claim, while the producers that would keep it attach and detach behind you.
   So a door that promises `["choice"]` earns the operator's `The session has not confirmed it
   took your answer.` for a tap that a producer which attached later did take, and a door that
   promises nothing gets the pre-v22 silence for every producer, including the ones that would
   have answered. Ours promises for the door and folds its producers' answers into the one the
   hub hears, exactly as it folds their answers about his typed words; the late-attaching
   producer is written down here as a cost rather than solved.

   **And a door only answers for a tap when it knows.** Where the producer holding a tap goes
   without a word — its socket dies with the frame in its hand — the door says **nothing at all**:
   "taken" would be a guess and "not taken" a claim about a tap the worker very probably did take,
   and neither is a fact the door has. The hub's own window closes on the silence and tells him the
   session did not confirm it, which is the only true sentence available. Refuse a tap only when a
   producer actually refused it. His typed words are the other way round — a door that carried them
   nowhere says so — because the hub renders that refusal as *"What you typed did not reach the
   agent"*, and there silence says nothing at all.

---

## 10. A container, worked

One conversation, in a container, talking to a hub on the host — with `kickoff-hub-attach` as the
container's entrypoint (§13.1). attach holds the claim, opens a door of its own, and starts the
engine as its child, so the wall has one process to run and one to stop.

**Mount two things.**

| mount | as | why |
| --- | --- | --- |
| the host's `/run/user/<uid>/kickoff/` **directory** | the same path inside | Never the socket **file**: the hub unlinks and rebinds it on every start, and a bind-mounted file becomes a stale inode the moment the hub restarts. The directory is `0700`. |
| the token file, read-only | anywhere, e.g. `/run/secrets/hub.token`, mode `0600` | The credential is a file the filesystem protects, not a variable the environment leaks. |

**Set three things**, and run attach as the entrypoint with the engine after `--run`:

```
KICKOFF_HUB_PROJECT_DIR=/workspace          # where the repo is mounted; for the audit record
KICKOFF_HUB_TOKEN_FILE=/run/secrets/hub.token
KICKOFF_HUB_ADDRESS=<the name your dispatcher minted>

# entrypoint (docker: behind --init, which reaps AND forwards signals; bwrap: without --as-pid-1,
# whose init reaps and forwards NOTHING — §13.5 says how a bwrap wall is actually stopped):
kickoff-hub-attach --opencode http://127.0.0.1:9700 --run opencode serve --port 9700 --hostname 0.0.0.0
```

**The relay socket is NOT one of them, under `--run`.** §9's derived path is a git fact, and a
container with the repo mounted but no git binary — the ordinary case — cannot compute one. attach
does not need it to: with a child to hand it to, it makes a **private** door of its own under the
temporary directory and pins it into the child's environment, so the tool server the engine spawns
finds it with nothing configured (§13.3). Nothing outside the wall dials that door, so nothing
outside the wall needs its name. (Without `--run` — a bare producer in a container — you would still
be told the door with `KICKOFF_HUB_RELAY_SOCKET`, off the relay's own first line of output.)

**Run as the same uid as the hub.** This is not optional: the hub reads the peer credentials and
closes a connection from another uid **with no reply at all** — you will see a socket that accepts and
then does nothing, which is the least debuggable failure in this system. Before that, the directory
is `0700`, so a foreign uid cannot even look inside it; `--check` says so in words.

**And if the wall remaps the uid** (`bwrap --unshare-user --uid 0`), the hub still reads the
operator's uid — measured — but the *path* is derived from the tenant's: `/run/user/0/kickoff/hub.sock`,
where nothing is. Either keep the operator's uid (do not pass `--uid`/`--gid`), or bind the directory
at `/run/user/<tenant uid>/kickoff/`, or set a fourth variable, `KICKOFF_HUB_SOCKET`, to where it
really is.

**What happens if each is missing:**

| missing | what you see |
| --- | --- |
| the socket directory mount | connect fails, nothing is listening. Retry with backoff; the hub may simply be down. |
| the socket **file** bind-mounted instead of its directory | it works until the hub restarts, then connect fails for ever against a stale inode. |
| matching uid | the connection is accepted and then closed in silence. No `refused`, no log you can see. |
| the token mount | `unknown_project` if you send something wrong, or your own "no secret at `<path>`" if you check first. **Keep retrying** — this mends when the file appears. |
| `KICKOFF_HUB_TOKEN_FILE` | the search starts at `KICKOFF_HUB_PROJECT_DIR`; with no git in the container it checks that one directory and stops. Set the variable. |
| `KICKOFF_HUB_PROJECT_DIR` | refuse to start, and say which variable. Never guess from cwd. |
| `KICKOFF_HUB_ADDRESS` | you connect as **the project itself** — taking its claim and its topic. Inside a container there is usually no git to derive a default from, so this is the variable to get right. |
| `KICKOFF_HUB_RELAY_SOCKET`, a bare producer behind a relay (no `--run`) | with no git in the container the derived path cannot be computed, so the adapter refuses to start and names the variable. Under `--run`, attach makes its own private door instead, so this is not needed. |
| a JSON type wrong in `hello` (`"pid":"12345"`) | **exactly what a uid mismatch looks like**: accepted, one line read, then silence. A first line that will not decode is closed with no `refused` frame at all. §6 has one worked `hello` with every type in it; check yours against it before you go looking for anything else. |

One caution this document flags rather than asserts: the hub identifies your process by the pid it
reads from the socket, and evicts a dead claim by looking for `/proc/<pid>`. An adapter in its own PID
namespace may therefore be judged by a pid that means something different on the host side. It has not
been measured here. If you run adapters in PID namespaces, measure it before you rely on eviction.

---

## 11. Where this document cannot make a stranger self-sufficient

Named rather than papered over, because the test this was designed against is an adopter with this
file and the `crates/hub-proto` docs and nothing else.

1. **The engine half is not specified and cannot be.** This file covers seam ① — adapter to hub. What
   your adapter *reads* from your engine, and how it injects the operator's answer back into an
   agent's turn, is yours. `docs/INTERFACES.md` describes both of ours as worked examples; neither is
   a contract.
2. **The exact sentences an agent should read are not here.** The three-outcome vocabulary — reached
   / queued / permanent — is in `plugins/kickoff-channel/server.ts`, and it is the least portable and
   most argued-over part of this system. If your agent-facing strings can promise something that is
   only true on some engines, read that file before writing them.
3. **There is no conformance suite you can run**, and there is one worked example.
   `docs/examples/attach-from-the-document.ts` is the smallest adapter of §1, written from this file
   and importing nothing of ours; `adapters/kickoff-hub-attach/test-two-producers.ts` runs it against
   the real door, and that run is what found the ordering defect now fixed in §1. It is an example,
   not a suite: it covers the good-day path and none of §8. Our own suites are
   `bun test-against-a-fake-hub.ts` and, under `adapters/kickoff-hub-attach/`, `bun
   test-two-producers.ts`, `bun test-what-breaks-it.ts`, `bun test-against-fakes.ts`,
   `bun test-check.ts`, `bun test-run.ts`, plus `cargo test -p herdr-tg the_real_plugin -- --ignored`
   — the last is the real tool server against the real hub with only Telegram faked. A third-party
   adapter has no equivalent, and a fake written from your own reading of this document proves only
   that you agree with yourself.
4. **The relay's local protocol is hub-proto, but its lifecycle rules are not in `hub-proto`.** §9
   lists the four differences; they live in `adapters/kickoff-hub-attach/` (`relay.ts` and
   `ledger.ts`), and if you write your own relay you are re-deriving them from prose.
5. **`limits` is told to you, not enforceable in advance.** `max_text` counts characters, `max_frame`
   counts bytes, and the option-id ceiling is in neither — it is Telegram's, and you learn it by
   being refused.
6. **The relay's derived socket path is git arithmetic, and the worked example does not do it.**
   §9 gives the formula and names its input exactly, but the example refuses unless
   `KICKOFF_HUB_RELAY_SOCKET` is set rather than deriving one — so nothing here demonstrates that a
   second implementation's derivation meets ours. If you implement the formula, check your answer
   against the path a running relay prints before you trust it.

---

## 12. Not in scope

Said plainly so nobody looks for it here.

* **`crates/`.** This is adapter-side. The hub already accepts an address and nothing on the wire
  changes.
* **The conversations redesign** (`docs/CONVERSATIONS.md`), rooms, enrolment and topic cleanup.

(Supervision and systemd units used to be listed here; they are built — `kickoff-hub-attach` and its
`deploy/kickoff-hub-attach@.service` template, §13. So was the pre-pong 256 KiB bound; it is 65
frames now and the hub acks what it refuses, §6.)

---

## 13. One command

<!-- BUILT, 4 September 2026. This section was a design; it is now code. Every file it names under
     `adapters/kickoff-hub-attach/` exists; `adapters/fanin/` and `adapters/opencode-bridge/` are
     gone, folded into it, every check they held moved across. It reads as a design in places (the
     tense of "becomes", the table of "what the build must touch") because it was written before the
     build; that is left as the record of the decisions, and where it says a file "is today" the
     file is now `adapters/kickoff-hub-attach/`. Written against one test: an adopter who reads this
     section and nothing else can start a worker. -->

The operator looked at what it takes to put one opencode agent on his phone and asked, in his
words, *"is there a way I don't have to type all of that stuff?"* — because a Claude worker is one
command and an opencode worker is **three hand-started processes** for one conversation:

| today | what it does | who starts it |
| --- | --- | --- |
| `adapters/fanin/fanin.ts` | holds the one slot at the hub for this address | he does, by hand |
| `opencode serve` | the engine; its MCP tool server attaches to the relay | he does, by hand |
| `adapters/opencode-bridge/bridge.ts` | watches the server's event stream, relays its prompts | he does, by hand |

That is a design smell, not a documentation one. This box has the corpse of it right now: an event
bridge still retrying against a relay socket whose relay died hours ago, because when one of three
hand-started processes goes, the other two hang.

**`kickoff-hub-attach` is one process that does all three jobs**, and can start the engine as its
child so that a wall — bwrap or docker, the machinery the operator is building — has exactly one
entrypoint. It reads the namespace of §2 through the one reader, holds the claim, exposes the door
of §9 so the engine's tool server attaches to it unchanged, watches the opencode server when told
to, and proves an environment can reach the hub before anyone trusts it.

Three things do not change. **The Claude path**: `plugins/kickoff-channel/server.ts` dials the hub
directly, is already one command, and is not touched — it ships to the session the operator is in
as this is written. **The wire**: `crates/` and `hub-proto` are untouched, and
`plugins/kickoff-channel/hub-link.ts` stays the one implementation, guarded by a test. **The
stranger's adapter**: `docs/examples/attach-from-the-document.ts` must still attach to attach's door
without a line of it changing, and it is run twice below — once as a producer at the door and once
as the "engine" a wall starts.

### 13.1 The command line

```
kickoff-hub-attach         [--opencode <url> [--opencode-binding-file <path> [--opencode-binding-generation <n>]]] [--run <command...>]
kickoff-hub-attach --check [--opencode <url> [--opencode-binding-file <path> [--opencode-binding-generation <n>]]] [--run <command...>]
```

Everything about *which* project, *which* conversation, *where* the secret is and *what* to dial
comes from the environment, §2, and only from there — `plugins/kickoff-channel/attach.ts` is the
only file that reads a variable, and this command adds no way round it. So a project directory is
never a flag: it is `KICKOFF_HUB_PROJECT_DIR`, and `.` means "the directory I was started in, and
whoever typed this vouches for it".

| flag | what it does | default |
| --- | --- | --- |
| *(none)* | Hold the claim for `(project, address)` and open the door. This alone is what `adapters/fanin/` is today, and it is what a Claude worker in a wall needs. | — |
| `--opencode <url>` | Also watch the opencode server at `<url>`: its questions and permission prompts go to the phone as `ask`, a tap goes back to the server's own reply endpoint. This is what `adapters/opencode-bridge/` is today, minus its process. The URL is the flag's value and nothing else; `OPENCODE_URL` retires with the bridge. | not watching |
| `--opencode-binding-file <path>` | Bind this conversation to ONE session of that server: the one named in the file at `<path>`, which whoever launched the engine writes and this command only ever reads. His typed words then go to that session and to no other; a question or a permission prompt from any other session on the same server is not drawn; and a line that arrives while the binding is absent, unreadable or naming a session that is not open is refused in his own words rather than guessed at (§13.10). An absolute path, and only alongside `--opencode`. | not bound — the most recently active root session of the project directory takes his words |
| `--opencode-binding-generation <n>` | The oldest binding this run may obey: a whole number, the one the launcher had reached when it started this worker. Nothing the process reads afterwards can lower it, so a watcher that comes back from a crash or a restart still refuses a file left over from before the last rollover — the half of the fence that a process's own memory cannot hold, because a restart is what destroys memory (§13.10). It survives a restart only in so far as whatever restarts the worker says the number again: the unit does, from `OPENCODE_BINDING_GENERATION` in the instance's own environment file, which systemd re-reads on every start (§13.6). With it set, a binding that names no generation at all is refused too: a wall that was given a number is a wall whose launcher numbers every writing. Only alongside `--opencode-binding-file`. | no floor — the fence orders only the writings this process has itself acted on |
| `--run <command...>` | Start `<command...>` as this process's child, in the project directory, with the namespace pinned in its environment so that any adapter descending from it finds the door (§13.3). When the child exits, withdraw every question the door still holds — his phone loses the buttons and reads *the session that asked has ended* — then say `bye`, close the door, exit with the child's status (§13.5). Everything after `--run` is the command; nothing after it is read as a flag. | no child |
| `--check` | Prove this environment can reach the hub, one plain line per fact, then exit 0 if every fact holds and 1 if any does not. Sends `hello` and `bye` and nothing else; creates no topic and leaves no refusal behind (§13.4). `--opencode` and `--run` may stay on the line — the check reads them for what it can verify and starts nothing — so a wrapper runs its real line with `--check` in front of it. | — |

**Exit status.** `0` a clean end; `1` a `--check` that found something to fix; `2` a refusal to
start, with the sentence naming the variable on stderr — the same `2` the relay and the bridge use
today; `127` the `--run` command could not be found; otherwise **the child's own status**, with a
child killed by signal *n* reported as `128 + n`, which is what `docker stop` and systemd read.

**Its first lines** say what it is speaking for, and where the door is — because §10 says a
container behind a relay is told the door "from the relay's own first line of output", and that has
to stay true:

```
kickoff-hub-attach: speaking for /home/<you>/scratch/oc-dogfood · lane-0904-1200
kickoff-hub-attach: the door is /run/user/<uid>/kickoff/fanin/9dcb04a8b0e28966.sock
kickoff-hub-attach: connected as "oc-dogfood · lane-0904-1200"
```

The name in quotes is the hub's own title for the conversation — the project's title, a separator,
and the address, which the hub clips from the left with an ellipsis when it is long
(`…fy-0904-check`). It is printed once, as the hub sent it.

Every line it prints for itself is prefixed `kickoff-hub-attach:`; the child's output passes
through unprefixed, so a journal shows both and a reader can tell them apart.

#### Worked invocation 1 — a local worker on this box

Once, per project, at a terminal: `herdr-tg enroll <repo>`. Once, per box: bun and opencode on
`PATH`, and the shim `~/.local/bin/kickoff-hub-attach` that `scripts/install-attach.sh` writes
(§13.6). Then, in the worktree the worker is for:

```
cd ~/scratch/oc-dogfood
KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach --check --opencode http://127.0.0.1:9711 --run opencode serve --port 9711
KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach         --opencode http://127.0.0.1:9711 --run opencode serve --port 9711
```

The second line is the worker: the claim, the door, the server, the watcher, and the tool server the
server spawns — one process tree, one command, and when the server dies the whole thing says
goodbye and exits with its status. The address is git's name for the worktree, or none in the main
tree, exactly as §4 says; a dispatcher that minted one sets `KICKOFF_HUB_ADDRESS` in front of the
same line. `--port` is not optional: `opencode serve` without it picks a random port, and the
watcher would then be watching nothing. The number appears twice on the line so the server and the
watcher can never disagree about it.

#### Worked invocation 2 — a wall's entrypoint

This is the shape kickoff's wrapper copies. docker is shown because it is the one everyone can
read; the bwrap differences are the two sentences after it.

```
docker run --rm --init \
  --user "$(id -u):$(id -g)" \
  -v "/run/user/$(id -u)/kickoff:/run/user/$(id -u)/kickoff" \
  -v "$repo/.kickoff/hub.token:/run/secrets/hub.token:ro" \
  -v "$worktree:/workspace" \
  -p "127.0.0.1:$hostport:9700" \
  -e KICKOFF_HUB_PROJECT_DIR=/workspace \
  -e KICKOFF_HUB_ADDRESS="$address" \
  -e KICKOFF_HUB_TOKEN_FILE=/run/secrets/hub.token \
  <image> kickoff-hub-attach --opencode http://127.0.0.1:9700 --run opencode serve --port 9700 --hostname 0.0.0.0
```

Line by line, each is one of §10's rules or one of the measurements in `docs/TAXONOMY.md`:

* `--init` — the wall's own reaper. attach forwards signals and propagates the child's status; it
  does not reap orphans, and refuses to be PID 1 with a child (§13.5). docker's init (tini) also
  forwards the signal `docker stop` sends, so the wall stops the way a desk does. **bwrap is not
  docker here**: without `--as-pid-1` its init sits at PID 1 and reaps — so do not pass
  `--as-pid-1` — but it forwards **nothing**, and a signal to the bwrap process ends bwrap and leaves
  the wall running with the claim held. §13.5 says how a bwrap wall is stopped; it is not by
  signalling bwrap.
* `--user "$(id -u)"` — the hub reads the peer's uid **in its own namespace** and closes any other
  uid without a word, and the directory is `0700` and the socket `0600` besides. A foreign uid fails
  before any of that: it cannot stat inside the directory, and the check says "this user may not
  look inside … run as the same user as the hub" when it sees that; a foreign uid that can (root
  in a rootful container) is closed in silence, and the check names that too. Under
  `bwrap --unshare-user --uid 0` the tenant sees itself as uid 0 and the hub still reads the
  operator's uid, measured — but the tenant derives `/run/user/0/kickoff/hub.sock`, where nothing
  is; §10 gives the three ways out (keep the uid, bind the directory at the tenant's path, or set
  `KICKOFF_HUB_SOCKET`). The check does not compare uid numbers, for exactly that reason.
* the **directory** `/run/user/<uid>/kickoff/`, never the socket file — the hub unlinks and rebinds
  its socket on every start, and a bind-mounted file is a stale inode after the first restart.
* the secret as a **file**, read-only, and its **path** in `KICKOFF_HUB_TOKEN_FILE`. The value never
  travels in the environment; attach refuses if anyone tries (§13.4).
* the worktree at `/workspace`, and `KICKOFF_HUB_PROJECT_DIR` naming it. A linked worktree's `.git`
  is a file pointing at the main repository's `.git/worktrees/<name>` — which is **not** in the
  wall — so git answers nothing here, no address can be derived, and `KICKOFF_HUB_ADDRESS` is the
  variable to get right: without it the wall speaks as the whole project and takes its claim.
* no `KICKOFF_HUB_RELAY_SOCKET`. Under `--run`, when git cannot derive a door and nothing named one,
  attach makes a **private** door in a folder of its own and hands it to the child (§13.3). Nothing
  outside the wall needs to reach that door, so nothing outside the wall needs to know its name.
  The wrapper therefore sets **three** variables, the three §10 always required — four when it
  remaps the uid, as the bullet above says.
* `--hostname 0.0.0.0` and `-p` — so kickoff on the host can reach the server to open sessions and
  send prompts. The watcher still dials `127.0.0.1:9700` inside. Under bwrap without
  `--unshare-net` the wall shares the host's loopback and there is no `-p`; then every wall needs a
  port of its own, and kickoff mints it the way it mints the address.
* the image holds bun, opencode, `plugins/kickoff-channel/` and `adapters/kickoff-hub-attach/` —
  the tool server is a bun script the engine spawns, so bun is in the image whatever attach is. It
  holds **no** copy of the operator's `~/.config/opencode/opencode.json`: that file carries his
  provider keys, and its `environment` block is built for the host, not a wall (§13.3).

#### Worked invocation 3 — a Claude worker in a wall

The Claude path is one command already, and under attach it stays one: the tool server the engine
spawns is `plugins/kickoff-channel/server.ts`, it reads the nine pinned variables of §13.3, and
nothing in it changes. What a wall has to know is around it, and none of it is written anywhere
else — a proof run on 6 September against the live hub found the four points below. The check
first; it makes no topic and, since a `bye` before the pong is a goodbye to the hub (§13.4), leaves
no refusal behind:

```
cd "$worktree"
KICKOFF_HUB_PROJECT_DIR=. KICKOFF_HUB_ADDRESS="$address" \
  kickoff-hub-attach --check --run claude -p --channels plugin:kickoff-channel@herdr-tg-local
```

Then the worker. Print mode is the **outbound leg only**: `reply`, `ask` and `done` reach the phone
through the door, and nothing comes back, because print mode ends the turn and his tap arrives as
a channel message that needs a turn to land in. A wall that needs his answer runs the engine
interactively under a pty — tmux, or the wall's own — not `-p`.

```
cd "$worktree"
printf '%s\n' "$prompt" | \
KICKOFF_HUB_PROJECT_DIR=. KICKOFF_HUB_ADDRESS="$address" \
  kickoff-hub-attach --run claude -p --output-format stream-json \
    --allowedTools "$tools" --channels plugin:kickoff-channel@herdr-tg-local
```

`$tools` is the comma-separated list of the plugin's four tools as the engine names them —
`mcp__plugin_kickoff-channel_kickoff-channel__reply`, and the same prefix for `ask`, `ask_resolved`
and `done`. The marketplace name after the `@` is whatever the wall registered
`plugins/kickoff-channel/` under; `herdr-tg-local` is this box's.

* **`--channels` is variadic, so it goes last and the prompt goes on stdin.** It takes every word
  after it as one more server name, and a prompt put after it is refused as an untagged channel
  entry — *`--channels` entries must be tagged* — and the engine exits `1` before it looks for any
  server. attach never reads stdin and the child inherits it (§13.5), so the pipe above reaches
  the engine untouched.
* **The trust dialog defaults to "No, exit", and `--check` cannot see it.** On a directory Claude
  has never been trusted with, an interactive session asks first — *is this a project you created
  or one you trust?* — and the default answer exits. Print mode skips the dialog. The two shapes
  a wall can take, both measured on 2.1.259: **without a tty** there is no dialog at all — the
  engine drops into print mode, and with nothing on stdin refuses *input must be provided either
  through stdin or as a prompt argument* and exits `1`; **under a pty** — tmux, or the wall's own —
  with nobody at the keyboard, the session sits at the dialog with *No, exit* selected, and a
  wrapper that types its prompt and Enter into the pane confirms that default: the engine exits
  `0`, attach prints `the engine exited (status 0)` and exits `0` itself, and the wall reads a
  clean run that did nothing. Either launch once interactively and answer it, or mark the project
  directory trusted in Claude's own per-project record before the wall starts
  (`hasTrustDialogAccepted` in its user configuration, as of Claude Code 2.1.259; the key is
  Claude's, not this interface's, and may move). The check proves the binary is on `PATH`; it
  does not prove the engine will speak.
* **The plugin's tools are deferred, in print mode too.** As of 2.1.259 the engine lists them as
  deferred and spends a turn on a `ToolSearch` before the first `reply` — reply-then-done is four
  turns, not two. A wall's turn or token budget must expect the extra turn, and a prompt that says
  "call reply" must not assume the tool is already loaded.
* **`KICKOFF_HUB_PROJECT_DIR` must be the real checkout or worktree, at the path the hub knows it
  by.** The address (§4), the door (§9) and the secret (§5) are all worked out from that tree; a
  copy of it at another path is a different project to all three and finds none of them. Where the
  tree cannot answer — the linked worktree of invocation 2, whose `.git` file points outside the
  wall — `KICKOFF_HUB_ADDRESS` and a told secret or conversation stand in, as that invocation shows.

What a run that holds looks like, in attach's own lines: `a producer attached (1 now)` a few seconds
after the engine starts; at the end `a producer said goodbye` · `a producer went away; 0 left` ·
`the engine exited (status 0)`, and the door is gone. Measured 6 September: `reply` and `ask` both
reached the phone through the door, teardown was clean, and the print run took thirteen seconds
wall. The one leg that run did not see is his tap coming back down the door into a Claude child's
turn — nobody tapped in the window; the direct path and the door's path with opencode are each
proven on their own (§9).

### 13.2 What becomes of `adapters/fanin/` and `adapters/opencode-bridge/`

The criterion the operator set is *"a clean house, and an house that makes sense"*, and the test of
it is that a stranger opening `adapters/` finds **one thing to run**. Two directories each with a
`start` script is two things. So both go, and their logic becomes modules of the one command:

```
adapters/kickoff-hub-attach/
  main.ts            the command: flags, the order of operations, signals, exit  (run this)
  relay.ts           the door — producers, envelope ids, ask-id namespacing, the queue share
  ledger.ts          who the producers are and what they wait on; survives a restart
  opencode.ts        the watcher — events to asks, taps to replies
  check.ts           --check
  run.ts             --run, and the ONE place anything is spawned
  README.md          the operator's page: what to type
  package.json       start and test
  test-harness.ts    the shared rig (fake hub, real producers, a real worktree)
  test-two-producers.ts · test-what-breaks-it.ts · test-against-fakes.ts   moved, §13.7
  test-check.ts · test-run.ts                                                new, §13.7
```

| today | becomes | why |
| --- | --- | --- |
| `adapters/fanin/fanin.ts` | `relay.ts` (the door, lines "Writing to a producer" through "The door") + the startup, signal and `bye` code folded into `main.ts` | The relay's logic is the product; its process was only a process. |
| `adapters/fanin/ledger.ts` | `ledger.ts`, moved unchanged | It already knows nothing about sockets. |
| `adapters/fanin/test-harness.ts` | moved; `FANIN` becomes `ATTACH`, `startFanin` becomes `startAttach` | Same rig, same spawn, new path. |
| `adapters/fanin/README.md` | folded into the new `README.md` | One page for one command. |
| `adapters/fanin/package.json` | gone | |
| `adapters/opencode-bridge/bridge.ts` | `opencode.ts` — the mapping (`onOpencodeEvent`), `answer()`, `watch()` and the open-question record; its `HubLink`, startup and signals go | What is genuinely opencode's stays; the wire was already shared. |
| `adapters/opencode-bridge/README.md` | folded into the new `README.md` | |
| `adapters/opencode-bridge/package.json`, `.gitignore` | gone | |
| `adapters/opencode-bridge/test-against-fakes.ts` | moved; spawns `main.ts --opencode <fake url>` | §13.7 |

**How the watcher joins the relay: as a producer, at the door, in the same process.** Today the
bridge attaches to the relay with two variables and not a line of its own changing, proven by
`test-two-producers.ts` Part 2c. Collapsing the process boundary changes nothing on the wire: the
watcher in `opencode.ts` holds a `HubLink` whose socket is attach's own door and whose `hello`
carries the same secret, the same address and an instance of its own. The relay half greets it,
numbers it, namespaces its ask ids, shares the queue with it and — when the hub link drops — ends
its connection exactly as it ends every other producer's, so the watcher's queued asks go back to
waiting rather than being reported as said. One routing path for a tap, one greeting path, one
ledger, and the machinery that exists for two producers is not duplicated for a third that happens
to live in-process. The relay's refusal "a relay told to attach to a relay would dial its own door"
stays for attach as a *whole* — `KICKOFF_HUB_RELAY=1` in attach's own environment is still refused
— because that sentence is about who holds the claim, and the watcher does not.

What this costs, said plainly: the watcher's asks carry a producer number like anyone else's, and
the `already_claimed` rule that lived in the bridge moves into attach's hub link, where the relay
never had one — the relay waited on a squatter for ever, saying only "the hub would not take this
relay". The rule changed on the way: the bridge counted **three refusals in a row**, and with the
link's backoff that is a claim held for three *seconds* — shorter than the ten a predecessor attach
gets to stop, so an ordinary restart tripped it. A squatter is a claim held longer than anything
legitimate holds one, so attach calls a run stuck by **elapsed time** (30 seconds; with the backoff
that is the sixth refusal, at about 31), and at that moment answers every frame its producers had
queued with an `ack` saying no *before* it ends their sockets — the first version ended the sockets
first, and the "no" had nothing to travel on. The watcher keeps no counter of its own: every
refusal it sees is the door relaying the hub's, and the door decides.

### 13.3 How the engine's tool server finds the door

The tool server is the same `plugins/kickoff-channel/server.ts` under both engines, and it reaches
the door in the same way it always has: `KICKOFF_HUB_RELAY=1` plus a door it is either **told**
(`KICKOFF_HUB_RELAY_SOCKET`) or **derives** (§9's formula, from git). attach's job is to make sure
one of those is true, and it does it two ways at once.

**Under `--run`, attach pins the whole namespace in the child's environment.** The nine variables
of §2, every one set explicitly, so nothing is derived twice and nothing is inherited from above:

```
KICKOFF_HUB_PROJECT_DIR=<attach's project directory, absolute>
KICKOFF_HUB_ADDRESS=<the address attach holds, or - when it holds none>
KICKOFF_HUB_CONVERSATION=<the conversation attach was told, or - when it was told none>
KICKOFF_HUB_TOKEN_FILE=<- when a conversation was told; else the path attach was told, or the one the ladder found — the channel's copy included>
KICKOFF_HUB_SOCKET=<the hub socket attach dials>
KICKOFF_HUB_RELAY=1
KICKOFF_HUB_RELAY_SOCKET=<the door>
KICKOFF_HUB_RELAY_DIR=-
KICKOFF_HUB_RELAY_GRACE_MS=-
```

A child is never given both a conversation and a path, because the reader refuses that as two
answers to one question — and when attach was told a conversation, the CONVERSATION is what is
pinned, not the path. The §2 overlay a second engine applies blanks the path and never the ninth
variable, so a tool server behind it still reads the room's secret and derives the room's door;
pinned the other way round (the first version), that tool server derived the seed's door and read
the seed's secret, and every word of the room's agent landed in the seed's topic.

The engine inherits that, and the tool server it spawns inherits the engine's. A Claude engine's
plugin, which has no `environment` block of its own, therefore finds the door with no further
configuration — and so does any adapter a stranger writes, including
`docs/examples/attach-from-the-document.ts`, which is why §13.7 runs it as a `--run` child, once
from a directory with no git and once from a lane worktree. The lane is why the token path is the
one attach *found*: a lane holds no secret of its own, attach finds the main tree's by the search,
and a child that does not search (§5 says being told is cheaper) would otherwise refuse "no secret
at `<lane>/.kickoff/hub.token`" while attach above it had just authenticated with that very file.

**On this box, under `--run`, the operator pastes nothing.** His file as it stands — an
`environment` block carrying three dropped names and `CLAUDE_PROJECT_DIR: ""` — works: opencode
gives its MCP child the server's whole environment with the block overlaid on top, the block
touches no `KICKOFF_HUB_` name, so the tool server inherits the pinned door and attaches. Measured
with the real engine, seven ways:

| the engine's config, and how it was started | what the tool server did |
| --- | --- |
| his file, hand-started beside a bare attach | refuses: "never said which project" — the shape attach retires |
| **his file, `--run`** | **attached, `said`, the hub logged the `say`** |
| his file, `--run`, in a directory with no git (a wall's shape) | attached, `said` |
| the §2 paste block, hand-started | attached, `said` |
| the §2 paste block, `--run`, address = git's name | attached, `said` |
| the §2 paste block, `--run`, **a minted address** | the tool server derives git's door, dials one nothing opens, `reply` says "not said yet … the relay is not running" for ever |
| the §2 paste block, `--run`, **no git** | the tool server refuses: "cannot work out where that relay is" |

So the eight-`-` block is the hand-started shape's answer to inheritance, and under attach it is
the wrong one: attach's pinning **is** the un-inherit that block exists for, with the right values
in it. When a dispatcher mints an address that is not git's name, or a wall has no git, a config
that overlays the pinned door with `-` makes the tool server look for a door nothing opens — the
split-brain nobody would diagnose in under an hour. attach cannot read the engine's config, so it
cannot refuse this; **`--check` prints the fact in words (§13.4), and attach prints the same
sentence as a warning when it starts** — the same function produces both, so they cannot drift.
The one thing this section retires is a variable the block never carried: `OPENCODE_URL`, which
becomes the value of `--opencode`.

**In a wall, the config is kickoff's, and its entry needs no `environment` block at all:**

```json
"kickoff-channel": {
  "type": "local",
  "command": ["bun", "/opt/herdr-tg/plugins/kickoff-channel/server.ts"],
  "enabled": true
}
```

There is nothing to overlay because there is nothing to un-inherit: attach is the outermost thing
in the wall, it pinned all nine, and the tool server takes them as they are. The `-` block exists
for a config that must serve every project and every lane on a host where an outer session may
have left variables behind; a wall has one project, one address and no outer session. Hand it to
the engine however kickoff prefers — the wall's own `~/.config/opencode/opencode.json`, or
`OPENCODE_CONFIG=<path>`, or `OPENCODE_CONFIG_CONTENT='{"mcp":{…}}'` on the environment; the
installed opencode reads all three, checked against its binary. **Do not mount the host's file
into the wall**: its `-` block would make the tool server derive a door from git, there is no git
to ask in a wall, and it would refuse to attach with a sentence about `KICKOFF_HUB_RELAY_SOCKET`
that the block itself prevents anyone from setting.

**Where the door is, in order:**

1. **Told.** `KICKOFF_HUB_RELAY_SOCKET`, verbatim, as today.
2. **Derived from git.** §9's formula, as today. This is the host case, and it is what makes the
   paste block meet attach with no variable set.
3. **Made, under `--run` only.** When neither of the above can say, and there is a child to hand it
   to, attach makes a folder of its own under the temporary directory — `mkdtemp`, mode `0700`,
   `kickoff-hub-attach-<six random characters>/` — and puts the door there: nothing else needs to
   dial it, so nothing else needs to know its name, and two walls cannot collide on it *because the
   name is random*. The first version named the folder by pid, and under `bwrap --unshare-pid`
   attach is PID 2 in every wall, so two walls sharing the host's `/tmp` derived one folder and the
   second died "another attach is already holding it". It is unlinked on exit; a wall killed
   outright leaves its folder behind, and no later wall reuses it. A temporary directory that is
   not absolute, or that does not exist, or that puts the path past the 108-byte ceiling, is a
   refusal naming `TMPDIR` — this box has already met the literal string `%h/.cache/tmp` there —
   and `--check` makes the same refusal without making the folder. Without `--run` there is no
   third option and attach refuses as the relay does today, naming `KICKOFF_HUB_RELAY_SOCKET`.

The private door of option 3 is never derived from the mount path: two walls with different
projects mounted at `/workspace` and the same address name would derive one door under the
mounted `fanin/` directory and the second would refuse "another attach holds it" — true, and the
wrong reason.

### 13.4 `--check`

**What it proves, and how it proves it without a topic.** The hub's admission is ordered, and the
order is what makes this clean (`hub.rs`, `admit` and `serve_connection`): peer credentials off the
socket, uid first — another uid is closed without a reply — then version, then secret to project,
then enabled, then the address shape; then the claim is taken and **`welcome` is sent**; then
`ping`; and **the topic is created only after the pong**, inside the `if live` branch. A connection
that ends before the pong is released — and, unless it said `bye`, audited as a refusal — and
nothing reaches Telegram. So `--check` connects, sends `hello`, treats the arrival of `welcome` as proof — socket reachable, uid admitted,
secret resolved to an enabled project, address well-formed and echoed, claim free — sends `bye`,
and closes **without ever ponging**. Confirmed in the code, not assumed.

One cost, so that nobody is surprised by it. The check **holds the claim for the length of one
round trip**; the hub releases it the instant the connection ends, so a wrapper that runs the check
and then starts the worker is not refused. It leaves **no refusal** behind: the hub reads a `bye`
that arrives before the pong as a goodbye — the connection is released, every frame it had read is
acked `no` (the `bye` included), no `refused` frame is sent, no topic is made, and what the hub
records is a bridge that said goodbye before it became live, not a refusal — one line in its
journal at `info`, carrying the reason the `bye` gave (*just checking*, for the check), where the
unit runs it at that level and a hub started by hand at the default `warn` writes nothing. Until 6 September it
did not: the pre-pong loop knew only the pong, the close settled as *"connected but never answered;
it is probably not allowed to talk to me"*, and every check a wrapper ran before trusting a wall
looked like an intruder in the hub's record. The new frame this paragraph once said would be needed
to tell the hub the difference turned out to be one the check was already sending.

**What it prints.** One line per fact, in the order the facts are established, each beginning `ok`
or `NOT`; a `NOT` line carries the sentence that says what to do. The last line is the count. Exit
`0` when every line is `ok`, `1` otherwise. In order:

| fact | `ok` reads | `NOT` reads |
| --- | --- | --- |
| the configuration | *(no line of its own)* | the reader's own sentence, in the register `main.ts` dies with, e.g. `NOT  nothing named a project directory (KICKOFF_HUB_PROJECT_DIR), so there is no way to reach the operator from here` · `NOT  KICKOFF_HUB_ADDRESS is "CEO/steering", which cannot be addressed: it has a slash in it, and a conversation name cannot contain one` — the five shape rules of §4, before dialling · `NOT  KICKOFF_HUB_PROJECT_DIR is set to the empty string; a variable set to nothing is not a value` |
| attach as a producer | *(no line)* | `NOT  KICKOFF_HUB_RELAY is set on attach itself; it belongs on a producer that attaches to attach, not on attach` — the start refuses this, so the check does; it arrives by inheritance, since every `--run` child has it pinned |
| the project | `ok   speaking for /workspace (not inside a repository)` — or `(the main tree of a repository)`, `(a linked worktree of <main>)` | *(a refusal is the configuration row above)* |
| the address | `ok   the conversation: lane-0904-1200 (named by KICKOFF_HUB_ADDRESS)` — or `(git's name for this worktree)`, or `ok   the conversation: the project itself` | *(a refusal is the configuration row above)* |
| the secret | `ok   the secret: <path> (told by KICKOFF_HUB_TOKEN_FILE)` — or `(conversation c-…, named by KICKOFF_HUB_CONVERSATION)`, `(conversation p-…, the one this repository is bound to)`, `(found above /home/<you>/proj, the older way)` — which term of §5's ladder found it | `NOT  no secret at <path> (KICKOFF_HUB_TOKEN_FILE); mount it there, or at a terminal: herdr-tg open` · `NOT  no secret for conversation c-…; open it at a terminal (herdr-tg open / herdr-tg grant), or fix KICKOFF_HUB_CONVERSATION` · `NOT  no secret for this project; open it at a terminal with herdr-tg open` — a verb, never a path |
| the secret, by value | *(no line)* | `NOT  KICKOFF_HUB_TOKEN is set, and the secret never travels as a value; put it in a file and name the file with KICKOFF_HUB_TOKEN_FILE` — and a `KICKOFF_HUB_TOKEN_FILE` that is 64 hex characters and no path reads `NOT  KICKOFF_HUB_TOKEN_FILE looks like the secret itself; it takes the path to the file` |
| the hub's socket | `ok   the hub's socket: /run/user/<uid>/kickoff/hub.sock is there` | `NOT  nothing at /run/user/<uid>/kickoff/hub.sock; the hub is not running, or the directory /run/user/<uid>/kickoff/ is not mounted here (mount the directory, never the socket file)` · a directory this uid cannot look into (it is `0700`): `NOT  this user may not look inside /run/user/<uid>/kickoff/ (it is the hub's, mode 0700); run as the same user as the hub` — the first thing a foreign uid hits, before any connect |
| reached | `ok   reached the hub` | `EACCES`: `NOT  the hub's socket refused this user; run as the same user as the hub` · `ECONNREFUSED`: `NOT  a socket file is there but nothing is listening behind it; the hub is not running` |
| admitted | `ok   admitted as "oc-dogfood · lane-0904-1200"` — the hub's own title for the conversation, once | closed after `hello` with no frame: `NOT  the hub took the hello and closed without a word; either this process is not running as the hub's user, or the hello was malformed, and from outside the two cannot be told apart` — and *only* then: the close that follows a `refused` is not reported, so a refusal is exactly one line · accepted and mute: `NOT  the hub accepted the connection and said nothing for 6 seconds; it is running but wedged — restart herdr-tg` · `unknown_project`: `NOT  the hub does not know this project; open it at a terminal: herdr-tg open` · `bad_token`: `NOT  the secret is not one the hub knows; enrol the project again at a terminal: herdr-tg enroll` · `not_enabled`: `NOT  this project is enrolled but switched off` · `version_skew`: `NOT  this command and the hub do not speak the same version; upgrade one of them` · `bad_lane`: `NOT  the hub will not address a conversation called <x>; if the hub is older than this command, restart herdr-tg` · `already_claimed`: `NOT  another connection holds this conversation right now; if it is your own worker, run the check before it and not beside it; if nothing of yours is running, a stray process is squatting the claim` · echo missing: `NOT  the hub did not give <x> a place of its own; it is older than this command` · anything else: `NOT  the hub refused for a reason this command does not know (<reason>)` |
| the door | `ok   the door: <path> (worked out from git), free` — or `(named by KICKOFF_HUB_RELAY_SOCKET), free`, or under `--run` with no git `ok   the door: will be made in a private folder under <TMPDIR> when the worker starts, and handed to its engine` | `NOT  another attach already holds the door at <path>; this conversation has a worker already` · no git, no `--run`: `NOT  this folder is not inside a repository and nothing named a door; set KICKOFF_HUB_RELAY_SOCKET` · under `--run` with no git: `NOT  TMPDIR is "%h/.cache/tmp", which is not an absolute path, so a private door cannot be made under it; set TMPDIR to a real directory` (or "does not exist", or "past the 108-byte socket limit") — the start's own refusal, made here without making the folder |
| the tool server | `ok   a tool server that works out its door from git here finds this one` — or, with no git, `ok   a tool server here must be told the door, and a worker started with --run tells it` / `ok   a tool server here cannot work out a door from git, so it must be given the same KICKOFF_HUB_RELAY_SOCKET=<door>` — or, when attach's door is not git's: under `--run`, `ok   the tool server the engine spawns is told this door by --run; a config that overlays KICKOFF_HUB_RELAY_SOCKET with - would look for <other> instead and never find this one`; told without `--run`, `ok   the door was named by KICKOFF_HUB_RELAY_SOCKET, so a tool server that works one out from git would look for <other>; give the engine the same KICKOFF_HUB_RELAY_SOCKET=<door>` | derived from a minted address, without `--run`: `NOT  a tool server that works out its door from git here would look for <other>; either use the worktree's own name as the address, or give the engine a config that names KICKOFF_HUB_RELAY_SOCKET=<door>`. Whichever line prints, attach prints the same sentence as a warning when it starts, from the same function. |
| the engine's address, with `--opencode` | `ok   the engine's address: http://127.0.0.1:9711` | `NOT  --opencode http://127.0.0.1: names no port; the watcher would dial the wrong server. Give it the port opencode serve was given, e.g. --opencode http://127.0.0.1:9711` — the start refuses the same URL; the unit's `${OPENCODE_PORT}` unset is how it arrives |
| the binding naming the worker's session, with `--opencode-binding-file` | `ok   the worker's session: <path> names a session, and every line the operator types goes to that one or is refused; it names <agent>, so a question is shown only where that session's turn ran under <agent>` — or, where the note names no agent, `…; it names no agent, so a question from that session is shown whatever agent its turn ran under`, because the reverse half of the fence runs only where the note asks for it and nothing else said which of the two a wall was in — or, before whatever starts the engine has written it, `ok   the worker's session: <path> is not written yet; until whatever starts the engine writes it, every line the operator types is refused out loud rather than guessed at`; and, when the directory the file goes in does not exist yet, `warn the worker's session: nothing has made <dir>/ yet, so whatever starts the engine must make it before it can write there` — for a directory that is missing and for no other reason, since one that is there and cannot be looked into is already named by the `NOT` above it | `NOT  --opencode-binding-file <path> is not an absolute path; give the full path of the file whatever starts the engine writes the worker's binding into` · `NOT  --opencode-binding-file was given without --opencode; the binding names a session on an opencode server, and without --opencode there is no server being watched` · `NOT  --opencode-binding-generation <n> is not a whole number; give the number of the binding this worker was started for, e.g. --opencode-binding-generation 7` · `NOT  --opencode-binding-generation was given without --opencode-binding-file; the number is the oldest binding that file may name, and without the file there is no binding for it to hold` — all four of them refusals the start makes too · `NOT  the worker's session: <path> is not safe to read, so nothing it says is trusted — <which of §13.10's checks it failed>; it must be a plain file this user owns that nobody else can read or write, in directories nobody else can write` · `NOT  the worker's session: <path> is there and could not be read; attach must be able to read it, and it is written by whatever starts the engine` · `NOT  the worker's session: <path> is written in a form attach does not know — <the key or the value that stopped it, named>; it holds one JSON object, saying "version": 1 and naming the session as "session_id"` · `NOT  the worker's session: <path> is not one attach can read — <the key or the value that stopped it, named>; it holds one JSON object, saying "version": 1 and naming the session as "session_id"` · `NOT  the worker's session: <path> does not say which form it is written in; whatever writes it must add "version": 1 to the object it writes, or every line the operator types is refused` · and the three of the binding's own claims that are decided with no server at all, exactly as the start decides them: `NOT  the worker's session: <path> is older than the one this worker was started for; it is binding <n> and the number given is <n>, so every line the operator types would be refused` · `NOT  the worker's session: <path> does not say how new it is, and this worker would be started for binding <n>; whatever writes it must number every writing` · `NOT  the worker's session: <path> is written for a different project than the one this is run in, so every line the operator types would be refused` · `NOT  the worker's session: <path> is written for conversation <the one it names> and this worker speaks for <the one it is>, so every line the operator types would be refused` · `NOT  the worker's session: <path> is written for conversation <the one it names>, and nothing here says which conversation this worker is, so the claim cannot be checked and every line the operator types would be refused; start it with KICKOFF_HUB_CONVERSATION, or whatever writes the note must leave "conversation" out` · and, on a wall pointed at its secret by path, the way out that wall could actually take instead — `KICKOFF_HUB_TOKEN_FILE` and `KICKOFF_HUB_CONVERSATION` are refused together (§5), so it is never told to add one to the other: `NOT  the worker's session: <path> is written for conversation <the one it names>, and this worker was pointed at its secret by path (KICKOFF_HUB_TOKEN_FILE), which does not say which conversation that secret is for, so the claim cannot be checked and every line the operator types would be refused; start it with KICKOFF_HUB_CONVERSATION in place of KICKOFF_HUB_TOKEN_FILE, or whatever writes the note must leave "conversation" out`. Never a `NOT` for a binding that is not there yet — the order a wall is started in is check, start, write the binding — and whether the session is OPEN is not asked here at all: that is the server's answer, and it is asked afresh on every line he types (§13.10). |
| the engine, with `--run` | `ok   the engine: opencode, found at <path>` | `NOT  the engine: opencode is not on PATH` |
| PID 1, with `--run` | `ok   not PID 1` | `NOT  this process is PID 1 and nothing reaps for it; put the wall's own init in front (docker run --init; bwrap without --as-pid-1, which reaps and forwards nothing — stop a bwrap wall by signalling attach itself)` |
| the count | `everything a worker here needs is in place` | `<n> thing(s) to fix before a worker here can reach him` |

The uid line is deliberately **not** a comparison of numbers. Under `bwrap --unshare-user` this
process sees uid 0, the mounted socket's owner reads as the overflow uid, and the connection
succeeds once the socket is where the tenant derives it — `KICKOFF_HUB_SOCKET`, or the directory
bound at `/run/user/0/kickoff/` — because the hub reads the credential in its own namespace. A
numeric check would say `NOT` to a wall that works; the connect and the `welcome` say what is true.

`--check` is what a wrapper runs before trusting a wall, and it is also the first thing a person
runs when a worker is silent: every failure this document has spent ten sections describing —
the silent close, the stale inode, the missing mount, the empty variable, the door nothing listens
at — prints as one line that names the fix.

### 13.5 `--run`

**The order of operations.** Read the namespace, and refuse with the variable's name if it is
wrong. Refuse if this process is PID 1 and there is a child to start. Open the door — dialling any
socket file already there, so that a live attach for this address is refused with exit 2 and a
leftover file is unlinked, exactly as the relay does today. Start the hub link. **Spawn the child.**
Start the watcher, if `--opencode`. The door is bound before the child exists, so the first tool
server the engine spawns finds it; the hub link need not be up — a child whose hub is down is told
"not said yet" by its tool server, which is the honest sentence, and the link keeps dialling.

**The child** runs in the project directory, inherits attach's stdin, stdout and stderr, and gets
attach's environment plus the eight pinned variables of §13.3. attach never reads stdin itself: it
is not an MCP server and nothing on that stream is for it.

**Signals.** attach installs handlers for `SIGTERM` and `SIGINT` — both, because a process with no
handler for them at PID 1 ignores them, measured on this box with bun, and a wall that cannot be
stopped is a wall that gets killed with its questions still open. On either, it forwards the same
signal to the child, waits up to ten seconds, then sends `SIGKILL`, and proceeds as if the child
had exited on its own. It does **not** reap orphans: a grandchild the engine leaves behind reparents
to PID 1, and if PID 1 is a JavaScript runtime it stays a zombie for ever — also measured. Waiting
on a process this runtime did not start means reaching into libc from the process that holds the
hub's claim, for one job an init does in a kilobyte of C, so this design declines it: `--run`
refuses at PID 1 and names the fix, and `--check` says the same thing before it gets that far.

**Behind `docker --init`** attach is PID 2, tini forwards the signal `docker stop` sends, and every
signal and every exit behaves as it does on a desk; docker ends the container when PID 1 exits.

**Behind bwrap's init it does not**, measured on this box (bubblewrap 0.12) and pinned by
`test-run.ts` so it cannot drift again. bwrap's init reaps zombies and does nothing else: it
forwards no signal, and it ignores `SIGTERM` itself. So —

* a `SIGTERM` to the bwrap process **ends bwrap (143) and reaches nothing inside**: attach and the
  engine keep running, the door stays bound, the claim stays held — the corpse squatting the claim
  that this section opens with, and the successor's `already_claimed` run follows;
* **stop a bwrap wall by signalling attach's own host pid** — the *child* of the pid bwrap reports
  on `--info-fd` (that pid is the init). attach then forwards, waits, says `bye`, and the wall is
  empty; bwrap returns the child's status;
* pass **`--die-with-parent`**, so a wall whose wrapper dies is killed with it rather than left
  squatting. That is `SIGKILL` to the wall — no `bye` — and the hub releases the claim when the
  socket closes, which is the right outcome for a wrapper that is gone;
* do **not** read bwrap's return as "the wall is empty": when attach exits, bwrap returns at once
  while the engine's orphans stay inside with bwrap's init until they exit on their own.

Said plainly, the decision: the wrapper owns the host-pid lookup. The alternative — `--as-pid-1`,
with attach at PID 1 signalable by its host pid and orphans dying with it — costs zombies for the
life of the wall, since attach does not reap, and an agent's shell spawns constantly; attach keeps
refusing `--run` at PID 1. The wrapper is kickoff's (§13.9); these four bullets are what it copies.

**When the child exits**, for any reason: every question the door still holds is withdrawn first —
`ask_resolved{how: withdrawn}` for each, with the outcome *the session that asked has ended* — so
the hub takes the buttons off his phone and puts that sentence under the question, instead of
leaving a keyboard that a tap an hour later would send to nothing; the ledger is then written down
empty, so the next run of the same address has no stale question to route a late tap for. This is
the **engine's** exit and nothing less — and it is every question at the door, not only those from
producers that descend from the child: the door is the engine's descendants' (§13.3), and a
producer pointed at it by hand from outside the engine has its open question retired with the same
sentence. A producer that says goodbye while the engine lives — a tool
server ending with its turn, a watcher reconnecting — keeps §9's grace clock, because the voice
that asked may come back under its own instance and still wants its tap; only when the process
those producers descend from is gone is there nothing to wait for. Then `bye` goes on the hub link
if it is up, and attach waits, bounded, for the kernel to take the frames — `process.exit()` on
the same tick loses them to a short write; a link that is down cannot carry the withdrawals, and
attach says so once and does not wait on it; the door is unlinked, and a private door's folder with
it; and attach exits **with the child's status** — its exit code, or `128 + n` for a signal. A child that could not be started at all is `127` and one line saying so.
attach's own exit takes the door with it, so the next start of the same address finds either
nothing or a leftover file, never a live socket with nobody behind it.

**Where the spawn is, and why it may be there.** In `run.ts`, one call site, and the comment on it
says this: *the ADAPTER may spawn; the HUB never does.* The hub's closed list of capabilities
(`docs/INTERFACES.md`) puts "spawning, supervising, or killing anything" under things the hub is
not, and its argument is that no string from the wire may ever reach a command line. Nothing here
contradicts that: the command attach runs comes from **its own argv**, typed by a person or written
by a wrapper, and no frame from the hub can add to it, change it or start it — a `choice` reaches
an opencode reply endpoint or a tool server's turn, never `run.ts`. The hub still has zero
`Command` in its binary, and the test that pins the deletion of the keystroke path does not know
`adapters/` exists. This is seam ④'s own proposal, arrived at from the other side: the thing that
starts an engine is an adapter, holding a hub connection like any other, and the hub cannot tell
it from one that does not.

### 13.6 The unit

`deploy/kickoff-hub-attach@.service`, a `systemd --user` **template**, so that a worker on a box is
supervised the way `herdr-tg.service` already is: restarted on a crash, started at login, its
output in a journal. It reads the same namespace and nothing else; the only thing it adds is the one
number that is the engine's rather than the hub's.

```ini
[Unit]
Description=kickoff-hub-attach — the worker "%i", wired to the hub
Documentation=https://github.com/vinceferro/herdr-tg/blob/main/docs/ATTACHING.md
After=default.target
# Never give up. The same reasoning as herdr-tg.service: the operator is on a phone, and a
# start-limit burst would leave a worker dead until he reaches a keyboard.
StartLimitIntervalSec=0

[Service]
Type=simple
# The whole of one worker's configuration: docs/ATTACHING.md §2, KEY=VALUE, one file per instance.
# `-` so that a missing file is attach's own sentence ("nothing named a project directory") on the
# journal rather than systemd's opaque refusal to start.
EnvironmentFile=-%h/.config/kickoff-hub-attach/%i.env
# OPENCODE_PORT, OPENCODE_BINDING_FILE and OPENCODE_BINDING_GENERATION are the lines in that file
# that are not §2's: the port, and the note naming this worker's own session with the number it was
# written at, belong to the engine and not to the hub.
# OPENCODE_PORT appears twice on the line so the server and the watcher can never disagree about it.
# `opencode serve` without --port picks a random port, so it is not optional here — and it is
# checked before the start, because an unset ${OPENCODE_PORT} expands to NOTHING: the watcher would
# then dial port 80 while `opencode serve --port ''` listens on 4096, and the worker would hold the
# claim, get its topic, and deliver nothing. attach refuses the port-less URL too; this line is the
# one that names the variable and the file. (`$$` is a literal `$` for the shell.)
ExecStartPre=/bin/sh -c 'test -n "$$OPENCODE_PORT" || { echo "OPENCODE_PORT is not set; put it in %h/.config/kickoff-hub-attach/%i.env" >&2; exit 2; }'
# OPENCODE_BINDING_FILE is optional, and is the path of the note whatever starts the engine writes
# this worker's own session into (docs/ATTACHING.md §13) — attach only ever reads it. Give it a path
# of this instance's own — two workers on one box are two sessions, and one shared note would point
# them both at whichever was written last — and write that path OUT IN FULL:
#     OPENCODE_BINDING_FILE=/var/lib/kickoff-hub-attach/oc-dogfood.binding
# An environment file is not a unit file and not a shell: `%h` and `%i` are expanded in the settings
# of THIS file and in nothing systemd reads out of the env file, so a value written with them
# reaches attach as those four characters. attach refuses a path that is not absolute and exits 2,
# and with Restart=always below that is a worker dying every five seconds for ever over one line the
# operator was told to write. This box has met an unexpanded specifier as a path before.
# OPENCODE_BINDING_GENERATION is optional too, and only with a note: it is the number of the binding
# THIS worker was started for, and nothing the note says later may go below it. It is here rather
# than remembered because a restart is what destroys the memory — the monotonic rule inside a
# running watcher dies with the process, so a watcher that comes back reads whatever note it finds
# and takes it as the newest thing it has ever seen. systemd re-reads this env file on every start,
# so a launcher that rewrites the line at each rollover and restarts the unit is fenced ACROSS the
# restart, which is the whole case the number exists for.
# Refused here rather than at attach, because attach cannot know why a path is not a path and this
# file can: it is the one that told the operator what to write.
ExecStartPre=/bin/sh -c 'case "$$OPENCODE_BINDING_FILE" in ""|/*) ;; *) echo "OPENCODE_BINDING_FILE must be a full path written out; an environment file expands no systemd specifiers" >&2; exit 2;; esac'
# The flag has to disappear COMPLETELY for an instance that names no note, and `${OPENCODE_BINDING_FILE}`
# cannot do that: systemd expands an unset `${...}` to a single EMPTY argument, so attach would be
# handed the flag with nothing after it, refuse it by name and exit 2 — every worker that never
# wanted a binding would be dead, and the operator would read a sentence about a flag he never set.
# `${VAR:+...}` in the shell expands to no words at all, so an instance without the line starts
# exactly the command it started before this line existed. The number is passed the same way, and
# attach refuses it without the file, so the two lines are set together or not at all. (`$$` is a literal `$` for the shell, as
# above; `exec` leaves attach as the main process, which the KillMode below depends on.)
ExecStart=/bin/sh -c 'exec %h/.local/bin/kickoff-hub-attach --opencode "http://127.0.0.1:$$OPENCODE_PORT" $${OPENCODE_BINDING_FILE:+--opencode-binding-file "$$OPENCODE_BINDING_FILE"} $${OPENCODE_BINDING_GENERATION:+--opencode-binding-generation "$$OPENCODE_BINDING_GENERATION"} --run opencode serve --port "$$OPENCODE_PORT"'
# SIGTERM goes to attach ONLY; it forwards to the engine, waits, and says bye. SIGKILL to whatever
# is left after TimeoutStopSec. The default, control-group, would hit the engine and attach at the
# same instant and the goodbye would never be said.
KillMode=mixed
TimeoutStopSec=20
Restart=always
RestartSec=5
# No hardening block, on purpose. This unit runs an agent's shell, and every line of
# herdr-tg.service's hardening is a line that agent would trip. The wall is the safety, and the
# wall is kickoff's; this unit is for a worker on the operator's own desk.

[Install]
WantedBy=default.target
```

**How an instance is named.** The instance is a short label the operator picks for one worker —
`oc-dogfood`, `herdr-tg-main` — and the label names one file, `~/.config/kickoff-hub-attach/<label>.env`,
that holds that worker's namespace. The label is not the address and not the directory; those are
in the file, which is why two workers for two worktrees of one project are two labels and two
files. A file, written out in full because an environment file is not a shell and expands nothing:

```
KICKOFF_HUB_PROJECT_DIR=/home/<you>/scratch/oc-dogfood
OPENCODE_PORT=9711
```

and the same file for a worker whose launcher binds it to one session and numbers its writings —
every path written out, because this file expands nothing:

```
KICKOFF_HUB_PROJECT_DIR=/home/<you>/scratch/oc-dogfood
OPENCODE_PORT=9711
OPENCODE_BINDING_FILE=/home/<you>/.local/state/kickoff-hub-attach/oc-dogfood.binding
OPENCODE_BINDING_GENERATION=7
```

**The binding naming the worker's session** (§13.10) is the unit's other engine-side fact, beside
the port — and unlike the port it is optional. `OPENCODE_BINDING_FILE` goes in the same env file,
and the `ExecStart` above passes `--opencode-binding-file` with it *only when it is set*: that is
what `${VAR:+…}` is doing there, because an unset `${OPENCODE_BINDING_FILE}` reaches a command as
the flag followed by one empty word, which attach refuses by name — every worker that never wanted a
binding would be dead, over a flag its operator never set. Give it a path of that instance's own, because two
workers on one box are two sessions and one shared file would point them both at whichever was
written last — and write that path **out in full**, `/home/<you>/.local/state/kickoff-hub-attach/oc-dogfood.binding`.
`%h` and `%i` are expanded in the settings of the unit file and in nothing systemd reads out of an
environment file, so a value written with them arrives at attach as those four characters; attach
refuses a path that is not absolute and exits 2, and under `Restart=always` that is a worker dying
every five seconds for ever over one line. The unit's second `ExecStartPre` refuses it first and
says why, because attach cannot know that a specifier is what it was handed. Nothing makes that directory:
`--check` warns while it is missing, and whatever starts the engine's session makes it and writes
the file into it. The file need not exist when the unit starts — it usually does not, since the unit
is what starts the engine — and attach refuses a line typed before it is there rather than guessing
(§13.10).

**The floor goes through the same door.** `OPENCODE_BINDING_GENERATION` is the third optional line
of the env file, passed as `--opencode-binding-generation` by the same `${VAR:+…}` shape and only
when it is set, so an instance that names no number starts byte for byte the command it started
before. It matters that it comes from the *file*: `EnvironmentFile=` is re-read on **every** start,
so a launcher that raises the number in `~/.config/kickoff-hub-attach/<label>.env` at each rollover
and then `systemctl --user restart`s the instance is fenced **across** the restart — which is the
case the number exists for, and the one a fixed `ExecStart` could never serve, since `Restart=always`
replays the identical line. attach refuses the number without the file, so the two lines are set
together or not at all.

Add `KICKOFF_HUB_ADDRESS=` when a dispatcher minted one; leave it out to take git's name for the
worktree. Every other §2 variable may appear and means what §2 says. On this box the user manager's
`PATH` already carries mise's shims, checked, so `bun` and `opencode` resolve; on a box where it
does not, `PATH=` goes in the same file — a `--user` manager does not inherit a login shell's
environment, which the watchdog unit already had to learn.

```
scripts/install-attach.sh                       # writes ~/.local/bin/kickoff-hub-attach and installs the unit; starts nothing
systemctl --user enable --now kickoff-hub-attach@oc-dogfood
journalctl --user -u kickoff-hub-attach@oc-dogfood -f
```

The unit is the **opencode** worker. A Claude worker on a desk is the operator's own interactive
session with the channel plugin, which is not a service and is not this unit's business; a Claude
worker in a wall is `--run claude …` under the entrypoint of §13.1 — worked invocation 3 there, with
the four things a wall has to know about that engine — which the unit does not need either.

### 13.7 The tests

**Survive unchanged, not a line.** `plugins/kickoff-channel/test-against-a-fake-hub.ts` and
`cargo test -p herdr-tg the_real_plugin -- --ignored` — the Claude path, which this section does
not touch, and the proof that the two shared files it does touch (below) changed nothing that path
uses. Everything under `crates/`. And `docs/examples/attach-from-the-document.ts`, the stranger,
which is run twice: once as a producer at attach's door, where it is heard beside the tool server
and the watcher with one claim at the hub, and once as the "engine" attach starts with `--run` in a
directory with no git, where it inherits the private door and is heard. **If that file has to change,
the interface changed, and that needs saying loudly** — it is the alarm this section is designed
against.

**Move, subject renamed, checks kept.** Every behavioural check in `adapters/fanin/` survives,
because attach *is* the relay:

* `test-two-producers.ts`, 37 checks: Part 1 (two producers dialling the hub race), Part 2 (one
  claim, both reached, verbatim text, distinct ids for one minted `a1`, a tap to the asker only, an
  ack naming the producer's frame, typed words to every producer, a ping answered while all are
  silent, one goodbye not taking the lane down, the queue share under a flood), Part 3 (wrong lane,
  wrong secret, no hello, a second attach refused with exit 2) and Part 4 (a producer whose door is
  gone says "waiting" and names the door). One rename:
  `a_second_relay_for_one_conversation_refuses_to_start_rather_than_racing_the_first` becomes
  `a_second_attach_for_one_conversation_refuses_to_start_rather_than_racing_the_first`.
* `test-what-breaks-it.ts`, 17 checks: A (the hub goes away under attach), B (a producer dies with
  a question open), C (260 open questions), D (three producers, no hub), E (attach restarts and comes
  back under the same instance).
* `test-against-fakes.ts`, 23 checks, now spawning `main.ts --opencode <fake url>` against the fake
  hub: the handshake (secret in `hello`, no display name, nothing before `welcome`), an unknown
  refusal waited out with growing backoff, the `already_claimed` counter counting a run, and every
  mapping check — question to ask with published labels, tap to the v2 reply endpoint carrying the
  label, no second retirement on a tap, a re-tap posting nothing, permission to three buttons, a
  bogus option posting nothing, reject to the permission endpoint, a keyboard answered elsewhere
  retired reading both `properties` and `data`. Every one of them reads the ask id off the wire and
  never predicts it, so the producer number attach now prefixes changes nothing they assert.
  `every_adapter_speaks_the_same_wire_from_the_same_file` keeps its name and changes its list to
  `plugins/kickoff-channel/server.ts`, `adapters/kickoff-hub-attach/relay.ts`,
  `adapters/kickoff-hub-attach/opencode.ts` and `adapters/kickoff-hub-attach/check.ts`, and gains
  one rule: no other `.ts` under `adapters/` or `plugins/` that is not a test or the harness may
  carry `t: 'hello'`.

**Retired, because the subject is gone.** One check:
`the_real_event_bridge_attaches_to_the_relay_without_a_line_of_its_own_changing` proved that a bridge
*process* joins a relay *process* unmodified, and there are no longer two processes. The property
it protected — the event voice and the chosen voice share one claim and their ids stay apart — is
covered in the same part of the same suite by
`the_event_voice_and_the_chosen_voice_share_one_claim_inside_one_process`, which starts attach with
`--opencode` against a fake server, has the tool server say one thing and the server raise one
question, and checks the fake hub saw one `hello`, one `say`, one `ask`, and two ids.

**New.** Two suites, and each RED is watched failing for its reason before it goes green:

* `test-check.ts` — the facts of §13.4, one at a time, against fakes: every line `ok` and exit 0
  when all is well, and the fake hub saw exactly `hello` then `bye` — **no `pong`, nothing else** —
  which is the proof that no topic could have been made; no socket; a socket file with mode `0` so
  `connect()` fails `EACCES` and the line says "run as the same user as the hub"; a hub that reads
  the `hello` and closes; each refusal reason and its sentence; a bad address refused before the
  hub saw anything; `KICKOFF_HUB_TOKEN` set, refused before the hub saw anything; a token file that
  is the secret itself; a live attach on the door; a held claim.
* `test-run.ts` — the lifecycle of §13.5: `--run sh -c 'exit 7'` exits 7 after the hub saw `bye`;
  `--run sleep 60` sent `SIGTERM` exits 143 with the child gone; a child that traps `SIGTERM` is
  killed after the wait and attach exits 137; `--run env` shows the eight pinned variables and the
  door; the child's cwd is the project directory; the stranger as the engine, with no git and no
  door named, is welcomed through a private door and heard, with one claim at the hub; a command
  that does not exist exits 127; the stranger again from a *lane* worktree, where the pinned token
  path is the one attach found; and, when `bwrap` is on the box, attach under
  `bwrap --unshare-pid --as-pid-1` refuses `--run` with the PID 1 sentence, under the default
  reaper runs, two walls sharing one TMPDIR each get a private door, a `SIGTERM` to bwrap itself
  is measured to reach nothing inside while attach's own host pid stops the wall with a `bye`, and
  `--die-with-parent` kills the wall and releases the claim without one. When `opencode` is on the
  box, the real engine runs under `--run` with the operator's own `environment` block copied
  read-only from his file (or none, where there is no file), a session is opened, and the tool
  server the engine spawns is seen attaching at attach's door with the pinned door in its
  environment (read from `/proc`), and a tool server given exactly that environment is heard at
  the hub. Each is skipped, and says so, where its binary is absent.
* `test-what-breaks-it.ts` gains one: a hub that answers every `hello` with `already_claimed` is
  given more than thirty seconds before the door calls it stuck, and a producer that queued a frame
  meanwhile hears `ack no` for it before its socket ends.
* `test-check.ts` also holds the three refusals the start makes that the first check did not (a
  relay flag on attach, a TMPDIR that is not a path, an `--opencode` URL with no port), every
  refusal reason beside a live door with exactly one line each, a `0700` directory, the reader's
  register, a mute hub, a told door, and the hub's composed title printed once.
* **The exact-session binding of §13.10**, added 6 September, lives in the two suites it belongs to
  rather than a third: `test-against-fakes.ts` runs an attach of its own with
  `--opencode-binding-file` against a fake opencode holding two root sessions in one directory, and
  pins that his words go to the session the note names and never to the most recently active one,
  that a note absent, empty, half-written, stale, archived, a subagent's, another project's, or
  naming a different agent refuses the line in his own words and posts nothing, that a note replaced
  between two lines takes effect on the second with nothing restarted, that a generation going
  backwards cannot retarget, that a reply under an open question still goes to the session that
  asked, that a question from a session the note does not name never reaches him, that a tap is
  never answered into a session the note no longer names, and — the fleet ratchet — that the id is
  in no frame of the whole run. It also holds the second round's: that a question from a session the
  note names but the checks refuse never reaches him and his tap for it is not posted either, and
  that he is told rather than left with a "Sent"; that a note replaced while a line is being checked
  does not land it in the session it named first; that a launcher going back to a note with no
  number is still heard, and one correcting a note at the same number after the server refused it is
  taken; that a question nobody can be shown — the note unreadable, or an older event shape naming
  no session at all — is said out loud rather than dropped in silence; that a session the server
  lists for this project while saying it belongs to another is still refused; that a note which is
  not a regular file is refused instead of wedging the process; and that a project reached through a
  symlink is the same project. `test-check.ts` holds the check's own four: a path that is not
  absolute, the flag without `--opencode`, a note not written yet blessed with `ok`, and a note
  whose contents it cannot read refused — with the id never printed back at whoever ran the check.

### 13.8 What the build must touch outside the new directory

Listed so that nobody discovers it in a diff.

* **`plugins/kickoff-channel/attach.ts`** — the one reader, shared with the Claude path — gains
  one refusal: `KICKOFF_HUB_TOKEN` set to anything, and a `KICKOFF_HUB_TOKEN_FILE` that is the
  secret rather than a path, each with the sentence in §13.4. No correctly configured session sets
  either, so `server.ts` does not change behaviour and its two suites are the proof. §2 gains the
  row, marked *never*.
* **`plugins/kickoff-channel/hub-link.ts`** — the one wire, shared with the Claude path — gains two
  optional things, neither of which `server.ts` uses: a `once` mode that dials one time and never
  redials, and a callback that reports how a dial ended — the error's code (`ENOENT`, `EACCES`,
  `ECONNREFUSED`) or "closed before `welcome`" — which the module today discards at line 454, the
  defect `docs/TAXONOMY.md` §8 already named. The sentences the agent reads (`whenUnreachable`,
  `whenDropped`) do not change.
* **`docs/ATTACHING.md`** — §1, §9 and §11 name `adapters/fanin/`; §2 names `OPENCODE_URL` and
  says "all three adapters"; §10's worked container gets attach as its entrypoint (the shape in
  §13.1) and loses the sentence that `KICKOFF_HUB_RELAY_SOCKET` is not optional in a container,
  which stops being true under `--run`; §12 stops listing supervision as a separate slice.
* **`docs/CAPABILITIES.md`** REQUIRES 4, **`docs/INTERFACES.md`** and **`CLAUDE.md`**'s layout name
  the two directories that go.
* **`deploy/kickoff-hub-attach@.service`** and **`scripts/install-attach.sh`**, new. The installer
  writes the two-line shim (`exec bun <repo>/adapters/kickoff-hub-attach/main.ts "$@"`), copies the
  unit to `~/.config/systemd/user/`, runs `daemon-reload`, and **starts nothing** — the same
  discipline as `install-channel-plugin.sh`, which proves the plugin before installing it and
  never opens the door itself.

### 13.9 Not in this section, said plainly

* **The wrapper.** bwrap and docker lines are kickoff's; §13.1's second invocation is the shape it
  copies, not a script this repo ships.
* **The launcher** that starts a wall from a tap — seam ④ — is kickoff's adapter. attach is what it
  starts.
* ~~**Typed steering into opencode.**~~ Built, 5 September: the watcher carries a `message` to the
  session as a prompt (`opencode.ts`, `carry`), answers the hub with `ack{status, reason?}`, and
  the door refuses out loud when nothing is attached to take the words. §7, offer 6, has the rule.
  Every request of the server has a ten-second deadline, so one it takes and never answers cannot
  park every line typed after it; words opencode took and the agent then could not act on
  (`session.error`) are said so in the topic, once; a wall started with `--run opencode …` and no
  `--opencode` is half a phone — no questions, no prompts, typed words refused out loud — and the
  start and `--check` both say so. **Which** session of that server his words go to stopped being a
  guess on 6 September: `--opencode-binding-file` binds the conversation to one session in both
  directions, and §13.10 is the whole of it.
* **`session.idle` as `beat`.** Still acked and dropped by the hub, as `docs/TAXONOMY.md` §7 records.
* **Reaping at PID 1.** Declined, §13.5, and the wall's init does it.
* ~~**Telling the hub a check from a real connection.** A new frame, `crates/`, another slice.~~
  Not needed after all, 6 September: the `bye` the check was already sending is the difference,
  and the hub now reads a `bye` before the pong as a goodbye (§13.4).

### 13.10 Which session his words go to — the exact-session binding

**What a guess cost.** Without the flag, the watcher picks the session for a typed line by recency:
`GET /session?directory=<the project directory>&roots=true`, root sessions only, nothing archived,
the one that moved last, and it says which rule it used in its own transcript — *the one session
open*, or *the most recently active of N sessions* (`adapters/kickoff-hub-attach/opencode.ts`,
`sessionForTypedWords`). For a wall running one session for one directory that is exactly right.
For a wall running two it is a guess, and on 6 September the guess was measured going wrong on a
live box. What was measured is the *listing*, and it is worth being exact about that: whoever
launched the worker had made a restricted session to steer a room and had written down which one it
was; the only root session the server listed for that directory was an unrestricted coordinator, and
the session the room actually steers was a different one. So the rule this watcher used would have
handed the operator's next line to the unrestricted session — the one that may edit files, run a
shell and commit — and nothing here would have refused it, because nothing here had been told which
session was the worker's. That is a *delivery* into a turn nobody meant, which is worse than a line
that does not arrive, because both ends read it as success. No such line was typed on that box; the
measurement is what the rule would have done with one.

**The launcher owns the binding; attach enforces it.** Whoever starts an engine decides what a
session is *for*: its directory, the agent it runs, what that agent may touch, how long it lives,
and when it is replaced. None of that is this project's to decide (`docs/CAPABILITIES.md`
REFUSES 5), and none of it can be inferred from a directory listing. So the identity of the worker's
session is written down by the launcher, and attach only ever reads it. attach mints no session,
repairs none, and — the whole point — once it has been given a binding it never falls back to a
session it picked itself. `docs/CAPABILITIES.md` OPEN 5 is the same division of labour said from the
hub's side.

**The flags.** `--opencode-binding-file <path>` names the file the launcher writes, and
`--opencode-binding-generation <n>` is the oldest writing of it this run may obey. Both are flags of
this command and **not** `KICKOFF_HUB_*` variables, because §2 is the fleet-facing namespace every
adapter shares and this is one adapter's private arrangement with one engine — and nothing outside
this wall has any business knowing the id.

The **path** and the **number** are checked before anything is opened, and each refusal is exit 2
with the sentence on stderr, as every other refusal to start is:

* nothing after the flag — *`--opencode-binding-file` needs the path of the file whatever starts the
  engine writes the worker's binding into*;
* not absolute — *`--opencode-binding-file <path>` is not an absolute path; give the full path of
  the file whatever starts the engine writes the worker's binding into*. attach reads the file again
  on every line the operator types, so a relative path would be read against whatever directory
  attach happened to be started in;
* given without `--opencode` — *`--opencode-binding-file` was given without `--opencode`; the binding
  names a session on an opencode server, and without `--opencode` there is no server being watched*;
* nothing after the number — *`--opencode-binding-generation` needs the number of the binding this
  worker was started for, e.g. `--opencode-binding-generation 7`*;
* not a whole number — *`--opencode-binding-generation <n>` is not a whole number; give the number of
  the binding this worker was started for, e.g. `--opencode-binding-generation 7`*. Refused rather
  than ignored, because a floor misread as none is a fence standing open, and it would stand open in
  silence;
* given without the file — *`--opencode-binding-generation` was given without
  `--opencode-binding-file`; the number is the oldest binding that file may name, and without the
  file there is no binding for it to hold*.

The **file** is not checked then, because at that moment it is usually not there.

**The binding.** One small file, and **one shape** in it: a JSON object that says which shape it is.
**The names in it are the launcher's**, not this project's. kickoff writes this file, and it had
already shipped the object below; this section had specified one of its own, spelling the same things
`v`, `session` and `directory`, and the two shapes shared **no key at all**. A launcher's binding
would have been refused key by key and every line the operator typed in that room refused with it —
in the room, at the worst possible moment, for a reason neither side could see. It was caught before
a room ran and before any such file existed on this box, so nothing had to be migrated; the
launcher's spellings were then taken verbatim rather than aliased, because it shipped first, because
this repo does not carry two names for one thing, and because `conversation` is the id this
architecture already treats as the stable one. The one thing asked for in return is the version key,
which is what makes a shape nobody has written yet refuse instead of being half-obeyed.

This is the exact object to write:

```json
{"version": 1, "conversation": "c-0123456789ab", "session_id": "ses_00000000000000000000theOne",
 "canonical_project_dir": "/srv/rooms/steering", "agent": "kickoff-room-steering",
 "generation": 7, "verified_at": "2026-09-07T08:00:00Z"}
```

| key | required? | what it says |
| --- | --- | --- |
| `version` | required | which form the object is written in — `1` is the only one there is |
| `conversation` | optional | which conversation the binding was written for; where it is there it is checked against the conversation this attach is attached as |
| `session_id` | required | which session of that engine this conversation is |
| `canonical_project_dir` | optional | the directory the launcher says that session is canonically for, absolute |
| `agent` | optional | the agent that session is supposed to be running |
| `generation` | optional | which writing of this binding it is, counting up |
| `verified_at` | optional | the launcher's own record of when it last confirmed the session |

* **`version`** is the form the object is written in, and today `1` is the only one there is. It is
  read before anything else in the object, so a launcher writing a form this attach does not know is
  told *that* rather than told its perfectly good binding is unreadable rubbish — and a binding that
  says nothing about its form is told the one key to add and what to set it to, because the person
  reading that sentence is the one who can add it. **The bare session id on a line is gone**: it was
  the form this flag was born with, it could say which session and nothing else, and a launcher that
  learned to narrow the binding had no way to say so that an older reader would not silently ignore.
  A file still holding one is told apart from nonsense and gets its own sentence — the launcher is
  one to upgrade, not one that has written rubbish. **A note written with the names this reader
  wanted before the launcher's own were taken** — `v`, `session`, `directory` — is told apart the
  same way, and given the whole rename in one line rather than a key at a time: it has no version
  key either, so "add a version" was true and useless, and adding it left two more names this side
  had never accepted, each refused with nothing to act on.
* **Every refusal about what is WRITTEN names the part that stopped it** — the key it did not know,
  the key that must be renamed, the value that was not the shape that key takes — in the journal
  and in `--check`, where whoever wrote the launcher looks. The operator's own sentence never
  carries it: he has never been told the note exists, and he is not the person who can mend it. The
  two refusals that name nothing say all they have: the note is not there, or the operating system
  would not hand it over.
* **`conversation`** is the conversation this binding was written for — `p-` or `c-` and twelve hex
  characters, the id §5 spells. Where the binding names one it is compared with **the conversation
  this attach is attached as**, before a request goes anywhere, and a binding naming another one is
  refused whole. That is the check the old shape could not make at all: two rooms of one wall are two
  conversations, and a binding written for the room next door can name a directory that resolves
  perfectly well here and an agent that matches, and still be this wall pointed at a sibling's
  session. What attach is attached as is the conversation a dispatcher named in
  `KICKOFF_HUB_CONVERSATION` (§2), else the one the credential it found belongs to — and two of §5's
  four ways of finding a credential cannot say which conversation they are at all. Where this side
  cannot say, a binding that names one is **refused** rather than passed over: a claim nobody can
  check is not a check, and the line whoever runs the wall reads names a way out that wall could
  actually take — for a wall pointed at its secret by path that is *not* `KICKOFF_HUB_CONVERSATION`
  beside the path, which §5 refuses, but that variable **in place of** it, or the key left out.
  Leaving the key out is allowed, and is how a launcher that does not deal in conversations writes:
  then there is nothing to compare, and the directory, the agent and the generation are all there
  is. Left out means ABSENT: `"conversation": ""` is not a conversation id and refuses the whole
  binding, so a recipe that interpolates a room name unconditionally breaks every project that has
  no room.
* **`session_id`** is opencode's own session id — `ses_` and then up to sixty of `A–Z a–z 0–9`,
  which is what 1.18.25 mints and what attach will accept.
* **`canonical_project_dir`** is the directory the launcher says that session is canonically for, as
  an absolute path. Give it when the launcher knows it: it is compared with the directory attach
  speaks for *before anything is asked of the server*, so a binding that has drifted onto another
  worker's session is refused by its own words, at once, and without a request going out. Trailing
  slashes are ignored, and when the two names differ they are compared again with both sides resolved
  through symlinks — opencode stores and reports the directory it resolved, while attach was handed a
  path on its command line, so a project reached through a link would otherwise be refused as another
  project's for ever. Resolving can only make two names for one directory agree; it can never make
  two different directories match, and a path that cannot be resolved is compared as written.
* **`agent`** is the agent that session is *supposed* to be running. Measured against opencode
  1.18.25: a listed session carries `agent` as a plain string, and a session made before agents were
  named carries none at all — which is not a match either, because "no agent" is not "the agent you
  asked for". This is the field that would have refused the misdelivery above by name.
* **`generation`** is a whole number, zero or more, that the launcher raises every time it rewrites
  the binding. Once attach has **obeyed** a numbered binding, one carrying a **smaller** number, or
  the **same** number with a different session in it, is refused rather than obeyed: neither can be
  told apart from a stale wall writing over a newer binding, so it fails closed. Two walls that each
  believe they own the conversation would otherwise take turns retargeting it, and the one that
  wrote *last* — the loser of the race — would win. The rule for a launcher is one line: number
  every writing, and only ever count up.
* **`verified_at`** is the launcher's own record of when it last confirmed that session was there.
  attach reads it and **nothing is decided by it**: no line is refused for what it says, nothing
  waits on it, and it is compared with nothing. It is named here because the key set is closed — an
  object carrying a field this reader had never heard of is refused whole, which is the wrong answer
  to a launcher keeping its own record — and it stays the launcher's field, in the launcher's format.
  What attach believes about the session it asks the server, on the line the operator typed, and
  never from a time somebody wrote down.

The key set is **closed** — a key this attach does not know may be a *narrowing* of which session
may be spoken to, and obeying the rest of the binding while quietly dropping it would deliver his
words on a rule nobody checked — and a key that appears **twice** is refused with it. JSON keeps the
last of two keys of one name and every reader sees one, so a file whose first `session_id` line is
the one a person reads would deliver to the second; on the one file this whole flag treats as
authoritative, *what it says is not what it does* is a property that must not exist. Write each key
once.

**The floor, and why it is on the command line.** That much is memory, and **a restart is what
destroys memory**. A watcher that comes back from a crash, a redeploy or a `Restart=always` has
never seen the binding it was obeying a second earlier, so a file left over from before the last
rollover is the newest thing it has ever seen and it obeys it — which is exactly the rollback the
fence exists to refuse. Only a number carried *into* the process can refuse that, so
`--opencode-binding-generation <n>` says it: **the oldest binding this run may ever obey.** It is
**immutable** — set once, when the process starts, and nothing the process reads afterwards can
lower it — and it is set by **whoever starts the worker**, which is the same party that numbers the
bindings and therefore the only one that knows the number. A binding under the floor is refused **for
ever**, in the sense that matters here: for the whole life of that process, however many times the
file is rewritten, and a restart does not clear it because the wall says it again on the next start.
Where a floor is named, a binding that names **no generation at all** is refused too — a wall that
was given a number is a wall whose launcher numbers every writing, and one that suddenly does not is
one this cannot place.

  Two things the fence deliberately does not do, because each of them muted a wall for the life of
  the process while telling the operator about a file he has never been told exists. Where **no
  floor** was named, a binding carrying **no number at all** is not "older" — the fence then only
  orders two numbered writers against each other, and one that names no generation has made no claim
  to be newer, so a launcher that has gone back to writing unnumbered bindings is still heard. And
  the fence closes only behind a binding the **server confirmed**: a launcher that writes it a beat
  before its session is listed sees every line refused and has no reason to raise the number for the
  correction, so a binding that was read and then refused leaves the fence where it was. What the
  fence refuses is said in full in the journal, where whoever wrote the launcher looks, because the
  one sentence the operator can be given cannot carry "raise the number".

A field of the wrong kind — a relative `canonical_project_dir`, an empty `agent`, a `generation`
that is not a whole number — makes the whole binding one attach cannot read, rather than a binding
with one field quietly dropped. **The key set is closed**, and a key attach does not know refuses
the whole binding: a launcher's own bookkeeping does not belong here, because the next key anyone
adds is as likely to *narrow* which session may be spoken to as to be decoration, and obeying the
rest of the binding while quietly dropping it would deliver his words on a rule nobody checked. A
launcher that needs a new key upgrades the attach that reads it; they are two halves of one wall —
and `version` is how they find that out, instead of meeting in the middle on his typed words.
Whitespace around the object is trimmed; anything that does not begin with `{` is not this form at
all, and a bare session id — the form this flag was born with — is told apart from nonsense so that
the launcher gets the sentence about upgrading rather than the one about rubbish.

**The file itself is proved before a byte of it is believed**, because each of these is a way for
somebody who is not the launcher to choose which session the operator is steering, and none of them
can be told from the launcher's own writing once the words have been read. In this order, and the
first that fails is the answer:

1. **Where it sits.** Every directory on the way to it, from `/` down — walked twice, once over the
   path as it was given and once over what that path resolves to, because a link in the middle leads
   somewhere this process was never told about and the place it actually reads from is the one that
   matters. A directory somebody else owns is theirs to move the file out of and put their own in
   its place; a directory **anybody can write in** is anybody's to do the same, *unless* it carries
   the **sticky** bit, which is exactly the rule that stops one user moving another's file — without
   that exception `/tmp`, where a wall's private door already lives, would fail this for everyone. A
   directory that is a symlink is judged by who owns the link, since whoever can replace it chooses
   everything under it. Root is the one owner besides this user that passes: `/`, `/home` and
   `/run` are root's, so refusing them would refuse every path there is, and a box whose root is
   against you has already taken the whole wall. A directory that is simply not there yet is not a fault: that is a wall
   checked before its launcher has made the place it writes into, and the read below says "not
   written yet", which is the true sentence.
2. **What the path leads to**, before anything is opened — `lstat`, not `stat`. A **link** at the
   path is somebody else's answer to which session this worker speaks to, and it can be re-pointed
   between two lines he types without the file it names ever changing, so a link *at the path* is
   refused rather than followed. Anything that is not a regular file is refused too: a named pipe or
   a device where the binding should be would stop this single-threaded process in the kernel until
   somebody wrote to it — the event stream, every typed line and `--check` itself, for ever, with
   the process still alive holding the claim so nothing restarts it.
3. **Opened with `O_NOFOLLOW`**, and everything after that asked of the open file and never of the
   path again. Between the look above and the read below the path can be made to lead somewhere
   else, and a check on a name proves nothing about the bytes.
4. **Who wrote it.** A file this process's own user does not own is one somebody else wrote, and a
   session somebody else chose is where the operator's words would go.
5. **Who else could have.** Any **group or other** bit set at all is refused: a file another account
   can write is a binding another account can set, and one another account can read is a session id
   it has no business knowing. What the owner may do with their own file is the owner's business and
   is not looked at, so `0600` is what to write and `0400` passes too — and attach checks that much
   rather than trusting it.
6. **No larger than four kilobytes.** The longest legitimate binding is a couple of hundred bytes;
   the cap is what makes a flag aimed at the wrong file cost nothing.

What fails there is **not** said to the operator: his sentence is only that it could not be read,
because a mode and an owner are about a box he has never been told exists. The half that names the
directory, the link or the owner goes to the journal, where whoever can `chmod` it looks — and
`--check` prints it too, since whoever runs the check is that same person.

**How the launcher writes it, and why that way.** Whole, by rename — write a temporary file beside
it and rename over the top — because the binding is read at the moment a line is delivered, and a
half-written file read at that moment would be a redirection rather than a refusal. attach defends
itself against half a write as far as it can (half a session id is a valid-looking session id, so
anything that is not one whole object of the one shape is refused), but only the rename makes it
impossible. `0600`, because a file that says where the operator's words go is an instruction about
where the operator's words go, and anything on the box that may rewrite it may redirect them; it
belongs outside every repository, in a directory of the launcher's own that nobody else can write.
attach **checks** all of that now, as check 4 and check 5 above, and refuses rather than reads — but
the file is still the launcher's to write correctly, because a reader can only refuse what a writer
got wrong. Rewrite it when the session is replaced, raising `generation`; **do not delete it** — an
absent binding reads as a worker that has not said which session is its own *yet*, which is a
sentence about a boot and the wrong thing to say about a shutdown.

Written out, with nothing in it that is not in this section:

```sh
umask 077
binding=$state/opencode.binding   # the same path that was given to --opencode-binding-file
# The conversation only where the wall was started for one. An empty string is not a conversation
# id and refuses the whole binding, and this recipe is copied for plain projects as often as for
# rooms — so the key is left out rather than written empty.
if [ -n "${room:-}" ]; then names_a_room=$(printf '"conversation":"%s",' "$room"); else names_a_room=; fi
printf '{"version":1,%s"session_id":"%s","canonical_project_dir":"%s","agent":"%s","generation":%s,"verified_at":"%s"}\n' \
  "$names_a_room" "$session" "$project_dir" "$agent" "$n" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$binding.new"
mv "$binding.new" "$binding"      # whole, or not at all
```

**It may be absent while the engine is starting**, and that is the ordinary case rather than a
fault. attach opens the door and holds the claim before the engine exists (§13.5), and the launcher
cannot write a session id until the engine has made the session, so there is a window in which the
binding is not there, as long as that engine takes to boot. So an absent binding never
refuses the *start*, is never a `NOT` line in `--check`, and never queues anything: it refuses the
one line that arrives inside the window, in his own words, and the next line he types once the
binding is there goes through. Nothing is retried and nothing is held.

**The checks, in this order, and the first that fails is the answer.** Each is a refusal and never a
fallback: with the flag set, no branch below reaches the most-recent rule at all.

1. **The binding, read now.** Read at delivery time and never cached, so a launcher that rewrites
   it retargets the next line with nothing restarted — and the six checks on the file itself, above,
   run here, on every line, and not once at start-up. Absent or empty; not safe to read (the place
   it sits, the link it might be, its owner, its mode); there and unreadable; written in a form
   attach does not know; or a shape it cannot read: refused, each with its own sentence.

   **One exception, and only one.** When nothing has been *found out* — the file is not there, it
   could not be opened, it holds something this cannot make sense of, or (step 2) it names a
   conversation nothing here can yet check — and he is **replying to a question this watcher itself
   drew and still holds open**, his reply goes to the session that asked it and the rest of this
   list is not run. That session was proved against the binding when
   the keyboard went up, the question is his, and the only thing missing is the file; refusing him
   there told him to *try again in a moment* about a thing no moment of his would mend. A binding
   that reads perfectly well and names a **different** session is not this case: it has moved on,
   and the step below that asks whether he is replying to a question this conversation still holds
   open refuses the reply, as it always did.
2. **Whose binding it is**, when it says: the `conversation` it was written for against the
   conversation this attach is attached as. Before the fence, before the directory and before any
   request goes out, because a binding written for the room next door is right in every other
   particular — and a claim this attach cannot check, because nothing told it which conversation it
   is, is refused rather than passed over. Where the binding names no conversation there is nothing
   to compare and the rest of the list is all there is.

   The two refusals are not the same kind. *Written for another conversation* is the note's settled
   word and is final: the question is dropped, and a reply under an open question is refused with
   the rest. *Cannot tell whose it is* is nothing found out — the operator may grant, or the
   launcher may rewrite the note, while the wall runs — so it is held with the transient refusals of
   step 1: the question is kept and offered again, and a reply under a question this watcher itself
   drew still reaches the session that asked it. The way out it names is one the wall could take:
   a wall pointed at its secret by path is told `KICKOFF_HUB_CONVERSATION` **in place of**
   `KICKOFF_HUB_TOKEN_FILE`, never beside it, because §5 refuses the two together.
3. **Not older than the floor, and not older than what this run has already acted on.** The
   generation fence above — the floor first, because that is the half a restart cannot forget.
4. **The binding's own `canonical_project_dir`, when it gives one**, against the directory attach
   speaks for — before anything is asked of the server, because a binding written for another worker
   must not steer this conversation even if that session would resolve perfectly well here.
5. **The server is asked about the session**, with the same measured listing the guess used —
   `GET /session?directory=<the project directory>&roots=true`, one request. This step *fetches*;
   what it fetched is judged in step 7, after step 6, and the order matters: a rollover that lands
   during the round trip must read as a rollover and not as a session that has gone.
6. **The binding again, now the server has been waited on.** A listing is a round trip, and on a
   loaded server it may be the deadline's ten seconds wide. Delivering on the id read *before* that
   wait puts his words in the session a launcher's rollover has just replaced, acked as though they
   went where he meant. When the binding now names a different session the line is refused rather
   than retargeted — every check here was made about the old one, and nothing has proved them of the
   new. *Send it again* is the true instruction after a rollover, and it is why this is tested
   before membership: with the two the other way round, a rewrite racing a slow listing read as
   "not open on its server", which is a sentence about the wrong thing.
7. **In the listing at all**, and then: **not archived, not a subagent's, and not another
   project's by the session's own word.** When the named session is not in the listing, a second,
   *unfiltered* listing is made **to choose the sentence only, never to widen what is accepted**: it
   is what tells "not open on its server" from "a helper's session" from "another project's". A
   server that will not answer that one gets the plainest of the four.
   `roots=true` already drops a subagent, and it is checked again, because the cost of being wrong
   is the operator steering a session nobody is reading; the session's own `directory` is checked
   for the same reason, so acceptance never rests on the server having obeyed the `directory=` the
   listing asked for.
8. **Running the agent the binding named**, when it named one. A session that names no agent at
   all is not a match either: "no agent" is not "the agent you asked for".
9. **Is he replying to a question this conversation is still holding open?** Then the words go to
   the session that asked it — which, after every check above, is the bound session — and the
   question stays open for his tap, exactly as it does without the flag. A question asked *before*
   the binding was rewritten is refused instead: carrying his reply into the old session would steer a
   worker nobody is bound to, and carrying it into the new one would answer a question that session
   never asked.
10. **Only then** are his words posted, to that session and to no other, and the fence closes
    behind the binding that got them there.

**What he reads when one of them refuses.** The reason travels as `ack{status: refused, reason}`
(§7, offer 6) and the hub puts it in the topic he typed in, threaded under the line it refuses:
*What you typed did not reach the agent — <reason>. It will not be delivered later.* So every
sentence below is written to finish that one, in his register: no path, no session id, no status
code, and no word for a thing he has never been told exists.

| what happened | what he reads |
| --- | --- |
| the binding is not there yet, or is empty | the worker has not yet said which session to speak to; try again in a moment |
| the file is there and unreadable, or is not one it is safe to read at all | the note naming the worker's session could not be read |
| it holds something that is not one of these bindings | the note naming the worker's session is not one it can read |
| it is a form this attach does not read — a bare session id, or a `version` it does not know | the note naming the worker's session is written in a form this worker does not know |
| it does not say which form it is written in at all | the note naming the worker's session does not say which form it is written in |
| it was written for another conversation than the one this attach is attached as | the session named for this worker belongs to a different conversation |
| it names a conversation, and nothing told this attach which conversation it is | the note naming this worker's session says which conversation it belongs to, and this worker cannot tell whether that is this one |
| the binding went backwards, or is under the floor this worker was started for | the note naming this worker's session is older than the one already in use |
| the worker was started for a numbered binding and this one names no generation | the note naming this worker's session does not say how new it is |
| the note, or the session, is another project's | the session named for this worker belongs to a different project |
| the server does not list that session | the session named for this worker is not open on its server |
| it is archived | the session named for this worker has been archived |
| it is a subagent's | the session named for this worker is a helper's session, not the one to speak to |
| the note named an agent and the session names none | the session named for this worker does not say which agent it is running |
| the note named an agent and the session runs another | the session named for this worker is running a different agent from the one it should be |
| the note named an agent and a TURN of the bound session ran under another — a question only; nothing he types is refused for this | the worker is answering under a different agent from the one it should be |
| the note named an agent and nothing can ever say which agent the turn ran under — the event names no message to look up, or the message names no agent — a question or a permission prompt only | there is no way to tell which of the worker's agents asked this |
| the note named an agent and the server would not say which agent the turn ran under, for as long as the question could be kept — a question or a permission prompt only | the worker's server would not say which of its agents asked this |
| he replied under a question the bound session did not ask | the question you replied to was asked by a session this worker no longer speaks to |
| the note was replaced while his line was on its way to the server | the worker moved to another session while that was on its way, so it was not delivered; send it again |

A server that will not answer the listing at all is refused with the sentences the unbound path
already had for that — *the worker's server would not say which session is open*, *the worker's
server did not answer in time*, *the worker's server could not be reached* — because the fault is
the server's and not the note's. Every one of them is the end of that line: nothing is queued,
nothing is retried, and his words are never then sent somewhere else instead.

**Enforced both ways, by the same rule.** A binding that only governed what goes *down* would leave
the other half open: a stranger's session on the same server would put its keyboard on the
operator's phone, and his tap would answer into a session nobody bound. So with the flag set, a
question from any other session is not drawn, and neither is its permission prompt — the operator
never sees it, and the agent that asked it waits for whoever is actually watching that session — and
a tap for a question whose session is no longer the bound one is answered into nothing at all, its
record **kept** rather than forgotten, so that if the note names that session again the tap's own
question is still the one it belongs to.

**The same rule** matters as much as the same direction. Both halves run every check in the list
above, the server's answer included. Two rules — one that checked the note against the server and
one that only compared the id it holds — meant a note naming another project's directory, or a
session running an agent it should not be, drew that session's keyboard on his phone and posted his
tap into it, while the very next line he typed at it was refused. One conversation, one session, one
rule, in both directions.

It fails closed the same way the other half does: while the note says nothing attach can use, **no**
question is drawn — a question shown under the wrong conversation's name is worse than one that
waits. But a question is not a typed line: nobody is waiting at a keyboard for the answer to come
back, an **agent** is, and dropping it leaves that agent blocked on a keyboard that will never
appear, with no record to retire and nothing to try again. So the two reasons a question is not
drawn are held apart. The note saying **this is not the session** — the fence, another project,
another agent, a session the server does not list — drops it, and that is another wall's business.
The machine **not saying** — the file is not written yet because the launcher is a beat behind its
engine, nothing on this box can yet say which conversation this wall is so a note that names one
cannot be checked, the server stalled, the server answered something unreadable — **keeps** it, and
offers it again on a timer until it can be shown. The conversation is in that list and not the other
one because both ways out of it land while the wall is running: the operator grants, or the launcher
rewrites the note. Deciding it once, at the moment the question arrived, spent an agent's whole turn
on a keyboard that was never drawn and told the operator so once for however many questions were
lost. Bounded three ways, because a wall whose server never
returns must not grow a queue: eight questions at most, a minute each, and one offered again per
pass, since every attempt costs a request with a ten-second deadline and events are handled strictly
in order.

Failing closed is not the same as failing quietly, and this half has no `ack` to carry its reason.
So a question given up on for a reason **about the note** — the minute is up, or there was no room
left to keep it — is said once in the topic, as *The worker asked something and it cannot be shown
here — <reason>*, and said again only after a question has got through in between, so a
wall asking every second cannot spend the project's send ceiling on one sentence. A question from a
session that is simply not this one is another wall's conversation and is not said at all. An event
in an older shape that names **no** session cannot be matched against the note either way; it is not
drawn, and it is said once in the same words. And a tap that reached nothing is said under his
answer — *Your answer did not reach the worker — <reason>. Nothing was sent to it* — because the hub
has already put "Sent: X" on his phone, and leaving him with that and nothing else is the dead
keyboard this adapter exists to end.

**What `--check` can and cannot tell you.** It reads the binding for the things knowable without
the engine running — that the path and the number are ones attach can use, that whatever is written
there is safe to read and a shape it can read, and the three claims the binding makes that are
settled with no server at all: the generation, the project, and the conversation it was written for
— and it prints the line §13.4's table gives. That line also says whether the **reverse** half of
the fence is running, because that is a property of the note rather than of the flag: a question is
checked against the agent its turn ran under only where the note names an agent, and a launcher
writing `{version, session_id}` gets no turn-level check at all. Both are workable arrangements and
they are not the same one.
This is the one place the safety half is spelled out, naming the directory, the link or the owner
that failed it, because whoever runs the check is the person who can put it right. It does **not**
ask the server whether the session is open, because the ordinary order of starting a wall is *check,
start, write the binding*, and a check that printed "not open" for that would be a check nobody
could pass. A binding that is not there yet is an `ok` line saying what happens meanwhile, never a
`NOT`.

**The binding decides the answer the hub hears, even at a door with other voices behind it.** A
wall can have more than one producer at attach's own door (§13.3): the tool server the engine
spawns, and anything started from a shell inside the worker, which inherits `KICKOFF_HUB_RELAY`. The
door folds their answers into the one `ack` the hub reads, and it used to settle on the *first*
`accepted` it heard — so on a wall whose second voice takes typed words, a line the binding refused
was answered *accepted*, the hub put a thumb under it, and the reason reached him nowhere. The
watcher `--opencode` starts is the **carrier**: the producer his typed words are addressed to. Its
refusal outlives another producer's acceptance, and the hub hears the carrier's reason. Without
`--opencode` there is no carrier and the fold is exactly what it always was.

**Nothing about the session leaves this machine.** The id is a local fact about one wall: it is
never in a `hello`, an `instance`, an `ask`, an `ack` or a topic, and a test captures every frame of
a run with the flag set and fails if the id appears in one. **No frame carries a session.** The hub
has no field for one and learns nothing about one, which is why this section is in §13 — one
adapter's arrangement — and not in §6.

**Not the contract between the two orgs.** Said plainly, because a file is easy to build on: this
file works only because the launcher and the adapter are two processes on one box sharing one
filesystem, exactly as files are a capability of the local transport and not of the wire
(`docs/CAPABILITIES.md` offer 9, REQUIRES 3). It is a **local implementation detail of this
adapter**, and neither org should harden against the file. The durable thing is the **typed binding
it carries** — the conversation it is for, the session, the agent it is meant to be running, the
generation, and the floor the worker was started at, which is a fact about this start rather than
about the file — which a
transport that is not this machine (`docs/CAPABILITIES.md` OPEN 4) would have to carry with no
filesystem under it, and which an engine able to hold a tag of its own could answer for itself with
nobody writing a file at all. `docs/CAPABILITIES.md` OPEN 5 is where that stays open.

### 13.11 Every sentence this command can put under his line

The hub renders an `ack{status: "refused", reason}` verbatim: under a tap as
`Not taken: <label> — <reason>. The agent has not got your answer.`, and under a line he typed as
`What you typed did not reach the agent — <reason>. It will not be delivered later.` So each of
these is a sentence on his phone, not a developer's note, and they are written down here for the
reason every operator-facing line in this document is: a sentence that exists only in code is one
nobody reviews.

The four voices are the tool server the engine spawns
(`plugins/kickoff-channel/server.ts`), the door itself (`relay.ts`), the watcher `--opencode`
starts (`opencode.ts`), and `--check`. A door in front of the others forwards their reasons word
for word.

| Answering | Reason, verbatim | When |
| --- | --- | --- |
| `choice` | `that question was answered at the terminal first` | the agent answered the question itself before his thumb landed |
| `choice` | `the agent had already stopped waiting for an answer to that question` | the agent gave the question up or took it back — the ordinary ending, since `ask` returns at once |
| `choice` | `the session that asked has ended, so there was nothing left here to hand the answer to` | print mode, or the turn torn down under it |
| `choice` | `nothing on this engine can hand the agent an answer from the phone by itself; a worker started with --opencode can` | an engine with no way to put a channel message in the agent's turn |
| `choice` | `the worker no longer has that question open` | no producer behind the door holds it; or the watcher has no record of it |
| `choice` | `the worker that asked that question went away before it could be answered` | its producer's socket had gone by the time the tap arrived |
| `choice` | `that is not one of the answers the worker offered` | a tap naming an option the question never published |
| `choice` | `the worker's server would not take it` · `the worker's server did not answer in time` · `the worker's server could not be reached` | opencode's own reply endpoint refused, timed out, or was unreachable |
| `message` | `nothing on this engine can take typed words from the phone by itself; a worker started with --opencode carries them` | the same engine, for his words |
| `message` | `the session you typed at had already ended` | the turn ended between his typing and its arriving |
| `message` | `nothing is attached to the worker yet that can take typed words` | the door has no producer to hand them to |
| `message` | `everything attached to the worker went away before taking it` | every producer the words went to had gone |
| `message` | `the worker has no session open, so there was nothing to hand it to` · `the worker's server would not say which session is open` | the watcher, with nowhere to put them |
| `message` | `the worker is set to run as an agent its server does not know` | the note binds an agent the server cannot resolve. Asked BEFORE his words are posted (`GET /agent`, once per run and remembered), because a prompt naming an unknown agent is answered 204 with no message written at all: the words are gone, and an ack saying they were taken is a thumb on his line that the topic then contradicts |

Two more the tool server writes, and neither is an `ack`: the tool result an agent reads when its
run has been replaced —

> This session's place on his phone was taken by a newer run of the same project. Nothing from here
> reaches him again until the session is restarted.

— and the rendering of `ack{delivered: "no", why: "stale-generation"}` into the agent's turn,
`a newer run of this project has taken its place on his phone`.

And three lines the watcher puts in the topic on its own, where no `ack` can carry them:

* `Your answer did not reach the worker — <why>. Nothing was sent to it.` — a tap for a session the
  binding no longer lets this conversation speak to (§13.10).
* `The worker asked something and it cannot be shown here — <why>.` — a question withheld for a
  reason **about the note**, said once per spell rather than once per question. `<why>` is any row
  of §13.10's table, and **three** of them belong to this line alone, all three being a turn of the
  bound session that this side cannot place: *the worker is answering under a different agent from
  the one it should be*, *there is no way to tell which of the worker's agents asked this*, and
  *the worker's server would not say which of its agents asked this*. Those three, and no other row,
  carry a second sentence, because they are the cases where this side also tells opencode nobody is
  coming: the session was already proved to be this conversation's own, so the request is the
  watcher's to turn down, and withholding alone would leave the agent blocked on a keyboard nobody
  will ever draw. The second sentence says what actually happened rather than what was attempted —
  *"It has been turned down, so the worker is not left waiting on it"* when the server took the
  refusal, and *"The worker's server would not take the refusal, so it may still be waiting on
  it."* when it did not, which is the same server that had just declined to say which agent was
  answering. **A permission prompt is turned down only for the first of the three.** A question's
  refusal says nobody is coming; a permission's says *No*, in the operator's name, to a tool call he
  was never shown — so it is given only where the turn was positively placed under somebody else's
  agent, and never where the turn could not be placed at all. For those two the second sentence is
  instead *"Nothing here has answered it, because answering it here would be answering for you. The
  worker is still waiting."* — the prompt is withheld and the worker waits visibly rather than being
  answered for. "Once per spell" is per SENTENCE, so a wall in that state hears about a withheld
  question and a withheld permission prompt separately: whether the worker was released or is still
  stopped is the one thing he would act on, and one of those standing in for the other is a false
  report.
* `The agent could not act on what you typed: <what the server said>` — opencode's `session.error`
  for a session his words went to. One of those failures is this adapter's own doing and does not
  travel in the server's words: a prompt naming an agent the server cannot resolve is answered 204
  with no user message written at all, so it is said as
  `the worker is set to run as an agent its server does not know` rather than as the server's
  `Agent not found: "…". Available agents: …`, which is a quoted identifier and an internal roster
  on a phone.

## 14. Files

<!-- DESIGN, 5 September 2026; both halves built 6 September. The operator asked "do we
     support media files bidirectionally?" and the answer was no — text only, both ways, and a
     photo he sent into a topic was dropped before the words beside it were read. This section is
     the design a build implements and a sceptic attacks. Where it says "measured" the number was
     read off the Bot API page or the running opencode server on this box; where it says "decided"
     it is a choice with its reason beside it.

     What exists in code (6 September), phone → agent: `message{files}` down, the bridge's
     `ack{files}` up, the media tree with its modes, shelf life and cap, the three ceiling checks,
     every operator line in §14.4's "Down" table, the Claude tool server's channel message and the
     opencode watcher's file part. Agent → phone: `welcome{outbox}`, `say{file}`, `done{file}`,
     `ack.why: no-file`, the outbox tree made at admission with the media tree, the name rule
     checked before the disk is touched, the descriptor walk of §14.3 with `O_NOFOLLOW` and
     `O_NONBLOCK`, the five checks, the two send ceilings, the caption ceiling and the two-send
     case, every line in §14.4's "Up" table, and `reply`/`done` taking a file on both engines
     through the one tool server. Three things to know about the build: the hub reads the file
     into memory (fifty megabytes at most) rather than streaming it, so a flood wait can send the
     same bytes again; `as: photo` on a picture over 10 MB still goes as a document, because
     Telegram would refuse it as a picture; and the ownership check on the FILE (a root-owned file
     a wall without `--user` wrote into the hub's own directory) has no test that does not run as
     root — the walk's ownership check, one line earlier and the same comparison, is the one a
     test can reach, and the test says so. What does not exist: §14.5's mount-naming refusal from
     the opencode watcher — what the server does with a `file:` URL it cannot read was not
     measured, because measuring it means prompting a live session — and `--check` proving the
     two mounts. -->

A screenshot from his phone is the most natural steering there is; a rendered page or a chart is
the most natural reply. Until this section the wire carried words in both directions and nothing
else.

**One principle, decided: bytes never cross the wall on the wire.** The frame ceiling is 64 KiB
(§6) and a screenshot is a megabyte; every instance of an engine will run inside a bwrap or docker
wall; and the hub is on the host, writing to its own state directory and nowhere else
(`docs/CAPABILITIES.md`, REFUSES 2). So bytes cross the way the socket and the secret already cross
— **by mount, at the same path on both sides** — and the wire carries a path, a name, a mime and a
count, which fit in a frame with room to spare. No path is ever translated, because there is
nothing to translate it to.

Two directions, two trees, one rule each:

* **Down, phone → agent.** The hub fetches what he sent into a directory of its own, and the
  `message` frame carries a **path the hub minted**. A wall reads that path through a read-only
  bind mount of that directory at the same path inside. On the host — a session started by hand —
  the path is simply readable.
* **Up, agent → phone.** `say` and `done` carry the **name of a file in the conversation's outbox**,
  a directory the wrapper bind-mounts read-write into the wall at the same path. The adapter copies
  the agent's file in and sends the name; the hub reads it from its own host path, checks that it
  is what it claims to be, and uploads it through the same send accounting as words.

### 14.1 The two directories

`<state>` is the hub's state directory: `$XDG_STATE_HOME/herdr-tg`, or `$HOME/.local/state/herdr-tg`
when that is unset — **as the hub's own process sees them**, which under `deploy/herdr-tg.service`
is the operator's home with no `XDG_STATE_HOME` unless `~/.config/herdr-tg/env` sets one. The two
segments after the tree are the conversation's, and both are things a dispatcher already holds:
`<project_id>` is the id offer 8 prints (`p-` and twelve hex characters, minted from the canonical
repo path — a safe path segment by construction), and `<address>` is the address it minted, or the
single character **`-`** for the project's own voice. `-` is free for this because §4's shape
rules refuse it as a lane name, so the two can never collide. §2 had already taken it for itself,
but a convention about a variable is not a rule about the wire: `hello` carries a lane directly,
and a stranger's adapter written from §6 alone touches no variable at all.

| tree | path | who writes | who reads | a wall mounts it |
| --- | --- | --- | --- | --- |
| media | `<state>/media/<project_id>/<address>/` | the hub | the wall | **read-only**, at the same path |
| outbox | `<state>/outbox/<project_id>/<address>/` | the wall, through its adapter | the hub | **read-write**, at the same path |

**Per conversation, not per hub, decided.** A flat `media/` mounted into every wall would let a
wall read every file he ever sent to any project — a screenshot of his bank, meant for one
project, readable by a stranger's. Each wall sees only what was sent to its own conversation, and
the hub uploads from a wall only what that wall's own conversation named.

**Who makes them.** The hub makes `<state>/media/` and `<state>/outbox/` when it starts, and a
conversation's two directories **when it admits the connection — at `welcome`, not at the topic**,
because a `say` carrying a file may legally be queued before the pong (§6) and the directory has
to be there for the copy; two empty directories for a bridge that boots and exits are nothing,
where an empty topic is a scar. Every segment it creates is **mode `0700`**. A wrapper that starts
a wall has nothing to mount before that moment, so it makes the conversation's two directories
itself, `0700`, as the same uid (§14.6), and the hub then keeps only what it would have made: on
every use it walks each segment and requires a directory, not a link, owned by its own uid, mode
`0700` — anything else is refused, with the directory named in the log, until a person fixes it.
The state directory itself is `0700` on this box **by history, not by code** (it is made with
`create_dir_all` and no mode, everywhere it is made), which is why these carry the mode explicitly
instead of inheriting a promise nothing enforces.

**Files the hub writes are `0600`.** Files a wall writes are the wall's; the hub does not care what
mode they carry, because it only reads them.

**Ceilings, shelf life, cap** — the numbers, and why each is that number:

| what | number | why |
| --- | --- | --- |
| one file, down | **20 MB** | Telegram's own ceiling on `getFile` — *"bots can download files of up to 20MB"*, measured off the Bot API page. The hub cannot fetch more, so refusing at 20 is the honest number rather than a chosen one. Checked twice: against the size Telegram reports, when it reports one, and again on the stream as bytes arrive, because the report is optional and the client library reads an absent one as four gigabytes. |
| one file, up | **50 MB**, and **10 MB as a picture** | Telegram's multipart ceilings — *"10 MB max size for photos, 50 MB for other files"*, measured off the page. The hub measures the file itself (`fstat` on what it opened), never the adapter's word. An image between 10 and 50 MB goes as a document; over 50 is refused before any upload. |
| shelf life, both trees | **48 hours** | One shelf life for the state directory, not two: the ask ledger keeps a question for 48 hours because Telegram refuses edits past that, and a file he sent stays readable at least as long as the question it answered can be edited. These trees are a mailbox, not an archive — an agent that needs a file for longer copies it out. |
| cap, each tree | **256 MiB**, oldest first | Twelve files at the down ceiling, or a couple of hundred phone screenshots — more than a person sends in two days by accident, and small enough that a home disk never notices it. Oldest by the hub's own `mtime` stat, never by a name a wall chose. |

The sweep runs when the hub starts, before every write into `media/`, and before every read from
an outbox — so an idle hub keeps a stale 256 MiB for exactly as long as it is idle, which is said
here rather than hidden. **The cap bounds what the hub keeps, not what a wall can write.** A wall
has a read-write mount and can fill the disk between two sweeps; a quota on a wall is the
wrapper's, not the hub's.

**Entries are bounded too, and by a different rule.** A lane is a git worktree name, so a
dispatcher that makes and destroys worktrees would otherwise leave one directory per tree in each
of these trees for ever — nothing over the cap, and nothing the cap could ever see — and the walk
that enforces the cap runs on the path of every file he sends. So the sweep also removes a
conversation's directory, and then its project's, **when it is empty and has been empty for the
shelf life**. Empty is deliberately not enough: a wall mounts a conversation's directory, and a
mount holds the directory it was made from, so removing one under a running wall would leave that
wall writing into a directory the hub can no longer see. Forty-eight hours with nothing written or
taken away is the evidence that this is not the conversation a wall is using; when it is wrong
anyway the failure is loud rather than silent — the hub finds no such file, the words go with the
line of §14.4, and the journal names the directory — and restarting the wall, which remakes its
mounts, is the fix.

**One fetch is bounded in time as well as in bytes**: sixty seconds for `getFile` and the body
together, chosen here rather than inherited from the HTTP client, because the fetch runs where
Telegram's own dispatcher orders that conversation's updates and what it costs is what his next
line in that topic waits. A minute carries 20 MB at about 2.7 Mbit/s. Past it the honest reading
is not "slow" but "not coming", and it is `download-failed`: he is asked to send it again.

### 14.2 On the wire

Everything here is additive and optional, in the sense §6 gives the word: **absent when it has no
value, never `null`**, and ignored by a side that does not know it. §6's tables are unchanged and
remain the fields every adapter must handle; an adapter written from §6 alone is complete, and
carries no file. Types are as in §6 — a JSON **string** unless said otherwise.

**Down.**

| frame | new field | what it is |
| --- | --- | --- |
| `welcome` | `outbox?` | The absolute path of **this conversation's** outbox, `<state>/outbox/<project_id>/<address>/`, as the hub sees it and as a wall must mount it. It has to be told: an adapter never learns its project id (§5, §7 offer 8), and inside a wall its own `$HOME` is not the hub's, so nothing it holds can derive the path. Absent on every hub before this section, and **absence means this hub carries no files**: an adapter asked to send one then sends the words ALONE — a `say` with no `file`, byte for byte what it always sent — and says in its own tool result that the file did not go and why, rather than sending a field the hub would strip in silence or refusing the words along with it. Built that way rather than as v13 wrote it (refuse the whole call) because the words are worth more delivered than withheld, and the result is the one place the agent reads. |
| `message` | `files?` | An **array** of objects, one per file he sent. Today it holds exactly one, because a Telegram message carries one file; an album arrives as one message per file, and each gets its own frame, in the order he sent them. `text` is his caption verbatim, or the empty string when he wrote none — **a file with no words is still a message**, and an adapter that refuses an empty `text` must not. |
| `ack` | `why` gains **`no-file`** and **`no-file-unsaid`** | Paired with `delivered: yes` only: the words reached him and the file did not. `no-file` means the hub said why **in his topic**, and an adapter may tell its agent so. `no-file-unsaid` means it could not: his messaging app turned the file away for the moment — a flood wait, a switched-off project, a topic that is gone — and a sentence about that is another send into the same refusal, so he is looking at words with nothing to explain the gap. **Do not report `no-file-unsaid` to an agent as a reason waiting on his phone**, and do say it is worth attaching again in a minute: it is the only one of the two that mends itself, where every `no-file` is permanent for that file. §14.4 has both tables. Safe to add to a closed set for the reason `bad_lane` was: only a frame that carried a file can be answered with either, and an adapter old enough not to know the words cannot have sent one. |

One entry of `files`:

| field | type | what it is |
| --- | --- | --- |
| `kind` | `photo` \| `document` \| `video` \| `animation` \| `audio` \| `voice` | What Telegram called it. A sticker and a video note are not carried (§14.4). |
| `path` | string, **or absent** | The path the hub minted and wrote — absolute, inside `<state>/media/<project_id>/<address>/`. Present exactly when the bytes are there; **absent exactly when `why` is present.** |
| `mime` | string, optional | What the sender's client declared — **data, never a verdict on the bytes.** For a `photo` Telegram declares nothing, and the hub records `image/jpeg`, which is what the `.jpg` on Telegram's own `file_path` says every photo is re-encoded to; the build captures one real `getFile` answer to pin that rather than assume it. |
| `bytes` | **number**, optional | What the hub wrote, counted by the hub. Present only with `path`. |
| `filename` | string, optional | The name the sender's client reported, verbatim, **as data**. Never part of any path, and never written into the audit file, which is one line per record and would take a newline in it as a second record. |
| `why` | `too-big` \| `download-failed` \| `not-stored` | Present exactly when `path` is absent. **Read this as an OPEN set**: it travels down only, from a hub that may be newer than your adapter, so a word you do not know must read to your agent as one you cannot explain rather than as nothing at all. `not-stored` is not a download that broke — nothing was downloaded, because this machine had nowhere to put the bytes — and the advice is the opposite: sending it again cannot help, and whoever looks after the machine has the reason in the journal. The hub is TRYING to tell the operator, in his topic, in words; that is an ordinary send against a shared ceiling and can be shed, so **never tell your agent he has already read it.** |

**Up.**

| frame | new field | what it is |
| --- | --- | --- |
| `say`, `done` | `file?` | An **object**, `{name, mime?, filename?, as?}`. `name` is the file's name in this conversation's outbox — one path segment, under the rules of §14.3. `mime` is what the adapter says the bytes are, and it decides picture or document. `filename` is what the operator sees a document called, as data, defaulting to `name`. `as` is `photo` or `document` and overrides the mime's choice: a tall page sent as a picture is refused by Telegram — a photo's width plus height may not pass 10 000 and its ratio may not pass 20, measured off the page — and a picture is downscaled besides, so an agent that wants him to read a page at full size says `document`. `text` may be empty when `file` is present; the file is then the message. |
| `ack` — yours, answering a `message` | `files?` | A **number**: how many entries of that message's `files` you handed to your engine — the ones with a `path` as the file, the ones with a `why` as the line saying so. Send it on every `accepted` ack for a `message` that carried `files`, and never on one that did not. **An ack without it means none did** — which is what every adapter shipped before this section says — and the hub, which compares it against the number of files that reached its own disk, then tells him so in the topic. An adapter that can take his words but not his file (an engine whose server cannot read the path) refuses the whole message with a reason (offer 6), because words that describe a picture the agent cannot see are worse than a line saying so. |

**Not on `ask`, decided.** A question's message is edited when the question is retired (offer 5),
and a photo message has a caption where a text message has text: the edit the ledger performs
would be the wrong call for the message it was written against. A screenshot beside a question is
a `say` with the file, then the `ask`.

Three worked frames, byte for byte in the shape the hub sends and takes — `$STATE` standing for the
hub's state directory, which is an absolute path on the wire:

```
{"v":1,"id":"h1","t":"welcome","project":"A Title Only The Registry Knows","lane":"engineering","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20},"outbox":"$STATE/outbox/p-9f3a1c2e5b7d/engineering"}
{"v":1,"id":"h7","t":"message","msg_id":"m-4412","text":"this is what the login page looks like now","from":{"chat_id":<number>,"user_id":<number>},"files":[{"kind":"photo","path":"$STATE/media/p-9f3a1c2e5b7d/engineering/20260905-231455-9f3a1c2e.jpg","mime":"image/jpeg","bytes":1183412}]}
{"v":1,"id":"f12","t":"say","text":"the chart, rebuilt","file":{"name":"3c9e1b7a.png","mime":"image/png","filename":"latency-p99.png"}}
```

**Behind attach's door (§9, §13) — one rule, and it is not "the first answer wins".** The door
writes `welcome` to a producer as it received it, forwards a `say` or `done` with everything but
the envelope untouched, and hands a hub `ack` down with only `ref` rewritten — so `outbox`,
`file`, `no-file` and `no-file-unsaid` cross it with no change. The one frame it rebuilds is a
producer's answer about his typed words (the fold of §7, offer 6, which turns several producers'
answers into the one the hub hears). For a message with no files that fold still sends the FIRST
accepted answer, so the thumb on his message arrives at once. For a message that carried files it
sends the LARGEST count any producer reported, and it sends it as soon as no later answer could
better it — the first producer that took them all — otherwise **it waits for the rest**, which is
what the refusal half of the same fold has always done.

Why it may not simply take the first: the producers disagree during any upgrade window and in the
ordinary opencode layout, where a tool server answers over stdio the instant it reads a frame and
the watcher answers after a POST. Whichever is quicker decides what the operator is told. With the
short count winning, the hub puts *"that file did not reach the agent"* in his topic although an
agent in that same lane is looking at the path; with the long count winning it says nothing
although another agent got only the caption. Either way the topic states something the hub did not
observe, which is the one thing §14.4 exists to prevent.

**Upgrading the hub means restarting every long-running attach, not only the sessions.** A relay
reads its own code once, at process start, so a `kickoff-hub-attach` under systemd with
`Restart=always` carries the old fold indefinitely — and from the hub's side that is
indistinguishable from a tool server that is too old. The hub's line for a short count therefore
names neither (§14.4).

**Skew, both directions, and how each is proved.**

* **Old adapter, new hub.** The adapter gets `message{text, files}` and ignores `files`; the agent
  gets the caption. Its ack carries no `files`, so the hub knows, and says so in his topic (§14.4):
  the file is not silently lost, it is loudly not delivered. The bridge in the operator's own
  session is the old one and cannot restart without ending his conversation, so this direction is
  proved the way `the_real_plugin_*` proves everything — the unmodified `server.ts` against the new
  hub over a real socket, with Telegram faked.
* **New adapter, old hub.** The `welcome` has no `outbox`, so the adapter sends the words alone
  and says why in the tool result; the plugin suite runs that against a fake hub whose welcome
  names none. And a stranger's adapter that sends `say{file}` anyway is proved the way `frame.rs`
  proves the `lane` case — the frame parsed by the `say` and `done` types as they were before the
  field existed, coming out as the words — because the hub old enough to test against is the one
  being replaced.

### 14.3 Names

**Down: the hub mints every path it writes, and nothing the sender said is in it.** The name is
the moment and a random suffix — `<yyyymmdd>-<hhmmss>-<eight hex characters>`, UTC — so a listing
of the directory is arrival order, which is what oldest-first needs and what an agent listing it
sees; then an extension from this table, keyed on the declared mime, **or none**:

| declared mime | extension |
| --- | --- |
| `image/jpeg` · `image/png` · `image/webp` · `image/gif` | `jpg` · `png` · `webp` · `gif` |
| `application/pdf` | `pdf` |
| `text/plain` · `text/markdown` · `text/csv` · `application/json` | `txt` · `md` · `csv` · `json` |
| `audio/ogg` · `audio/mpeg` | `ogg` · `mp3` |
| `video/mp4` | `mp4` |
| `application/zip` | `zip` |
| anything else | *(none — the mime still travels in the frame, as data)* |

The extension exists so an agent's own tools can tell a picture from a text file without being
told. It is a courtesy keyed on a string the sender chose, not a verdict on the bytes, and the
table is short so that nothing a sender declares can name an extension the hub did not choose. The
filename Telegram reports is carried in `filename`, verbatim, and is never joined onto anything.
The hub does not sniff, decode, thumbnail or re-encode: what he sent is what is on disk.

**Up: the hub opens what the adapter named, and checks what it opened — never the name.** A wall
is the untrusted side. A wall that writes a link of either kind called `shot.png` pointing at the
operator's secrets and then says `shot.png` must get a refusal, not an upload. So `name` is refused when it
breaks **the address rules of §4** — empty, over 64 bytes, exactly `.` or `..`, containing `/` or
`\`, containing a control character — and the file is refused when, once opened, it is not what a
file in an outbox can honestly be:

1. **Opened without following a link**, under a descriptor the hub holds on the outbox directory
   (`openat` with `O_NOFOLLOW`), and never by resolving a path and then opening it — a wall can
   swap the name between the check and the open, so every check is made on what was opened.
2. **A regular file.** Not a directory, not a device, not a socket, not a FIFO — the last two would
   park the hub for ever on the first read.
3. **Owned by the hub's own uid.** Every supported wall runs as that uid (§10), including a
   uid-remapped bwrap wall, whose writes land on the host as the operator's. Any other owner is a
   file nothing in this design wrote.
4. **Inside the outbox** — the descriptor it was opened under is the conversation's own outbox,
   every segment of which was opened as a directory, not a link, and checked on the way in
   (§14.1), so a name that decodes to somewhere else cannot have been opened at all. That is what
   "canonicalised" means here: the hub never asks the filesystem to resolve a string a wall wrote.
5. **The only name these bytes have.** `st_nlink` is exactly 1. A hard link is not a symlink, so
   `O_NOFOLLOW` does not see it, and it passes all four checks above — it IS a regular file, it IS
   owned by whoever owns the inode, and its directory entry IS under the descriptor the hub opened
   — while sharing its bytes with a name anywhere else on the same filesystem, which makes
   "inside the outbox" a rule about the NAME after all. `link` needs no read permission on its
   source, so a file a wall may name through a read-only mount but must not read is linkable here
   and would be uploaded. An adapter copies bytes in, and a copy has exactly one name, so nothing
   this design writes is refused by it.
6. **Under the ceiling**, by the hub's own `fstat`, after the open.

Any of the six is `no-file`; the words still go, and the operator's line says which (§14.4). The
hub hands the bytes to Telegram from the descriptor it checked, never by path: the upload
library's own path-taking constructor follows links and names the upload after the last path
segment, and both are exactly what this list forbids.

**What our adapters do**, so a stranger's can meet the same bar without copying it: the tool server
copies the agent's file into the outbox under a name it mints — eight hex characters and the
source's extension — and puts the source's basename in `filename`. The agent's own path is never
the `name`: it can be long, it can be anything, and where it came from is the agent's business.

**Picture or document**, for the hub's choice of call: `image/jpeg`, `image/png` and `image/webp`
go as a picture when under 10 MB and `as` does not say otherwise; everything else, and
`as: "document"`, goes as a document. A picture Telegram refuses for its dimensions is `no-file`
with a line saying to send it as a document — the hub does not try twice on its own, because a
second try is a second send from a budget he is sharing with every other conversation.

### 14.4 When it fails — what the agent reads, and what the operator reads

**Never a silent drop.** In every case the words go, and both sides are told the file did not — the
agent in the ack, the operator in one line in the topic, threaded under the message it is about so
two files a second apart cannot be confused. The operator's sentences below are the exact ones, in
the register the hub already uses for refused words (*"What you typed did not reach the agent — ….
It will not be delivered later."*); the agent's are the ack's vocabulary, and the sentence an
adapter makes of them is its own (§11, item 2).

**Down** — a file he sent:

| what happened | the frame | in his topic, under his message |
| --- | --- | --- |
| Telegram reported a size over 20 MB — on the message, or in the `getFile` answer | `files:[{kind, filename?, why:"too-big"}]`, no `path`; his words still in `text` | *That file did not reach the agent — it is 31 MB, and the most the bot may fetch is 20 MB. It will not be fetched later.* |
| the stream passed 20 MB when no size was reported | the same, `why:"too-big"`; what was written is removed | *That file did not reach the agent — it was still coming at 19.9 MB, and the most the bot may fetch is 20 MB. It will not be fetched later.* — the number the hub itself counted, which is the only one anybody has here |
| `getFile` refused BECAUSE it is too big | `why:"too-big"` | *That file did not reach the agent — Telegram says it is over the 20 MB the bot may fetch. It will not be fetched later.* — no number, because nothing here measured one: reading back the size on the message would name a figure under the ceiling as the reason it is over it |
| `getFile` refused otherwise, the download broke, or neither finished inside the fetch deadline | `why:"download-failed"` | *That file did not reach the agent — the download from Telegram failed. Send it again.* |
| this machine had nowhere to put the bytes — the conversation's media directory is not a directory, not the hub's, or not `0700`; the file could not be created | `why:"not-stored"`; **nothing is asked of Telegram at all** | *That file did not reach the agent — this machine had nowhere to put it. Sending it again will not help; whoever looks after this machine has the reason.* No retry, because every file he sends will meet the same directory, and the cause is a journal line naming it |
| a sticker or a video note | nothing goes down: neither carries words | *That did not reach the agent — it takes photos, documents, voice notes, videos and audio, not stickers or video notes.* |
| the ack came back without `files`, or short | *(the frame went, with `files`)* | *That file did not reach the agent — the worker here is too old to take files, and it got only your words. Sending it again will not help until it has been started fresh.* It names no cause and no remedy he can carry out from a phone, deliberately: a short count means the tool server is old **or** the relay in front of it is (§14.2), the hub cannot tell them apart from one number, and "restart the session" sent him round for ever against an attach service that was the real one |
| the adapter refused the message | *(offer 6, unchanged)* | *What you typed did not reach the agent — `<reason>`. It will not be delivered later.* |

A message that is not a file at all — a location, a contact, a poll — is as it was before this
section: not relayed, and nothing said. The reactions of `docs/RATE-PROBE.md` §3 apply to a file
as to typed words: the eyes when the hub has it, the thumb when the adapter has answered.

**Up** — a file the agent attached:

| what happened | the ack | in his topic |
| --- | --- | --- |
| `name` breaks the §4 rules, or the file is a link, not regular, not the hub's, or not in the outbox | `yes`, `why: no-file` | the words, and under them in the same message: *(The file the agent attached did not come through: it was not a file the bot may send.)* |
| over 50 MB by the hub's own measure | `yes`, `why: no-file` | *(The file the agent attached did not come through: it is 61 MB, and the most the bot may send is 50 MB.)* |
| Telegram refused the upload | `yes`, `why: no-file` | *(The file the agent attached did not come through: Telegram would not take it as a picture; ask for it as a document.)* — or, for any other refusal, the reason Telegram gave, in its words: the API's own `description`, unwrapped from the client library's two layers of it, stripped of control characters and clipped to one line's worth |
| the file was shed, switched off or had no topic AFTER his words landed, or the line above could not itself be sent | `yes`, `why: no-file-unsaid` | **nothing** — whatever refused the file refuses a sentence about it just as fast, and the audit already says `shed`. This is the one case where an adapter must not tell its agent the reason is on his phone, and the one worth attaching again in a minute |
| the upload went out and could not be confirmed | `unseen` | nothing — it may be there, and it is never retried (offer 3) |
| the words themselves did not go | `no`, with the existing `why` | nothing |

**Sends, and what they cost.** Telegram takes a caption of up to 1 024 characters on a picture or
a document (measured off the page; `max_text` is 3 500). So a `say` with a file whose `text` fits
is **one send** — the file, with the words as its caption — and one token from the conversation's
share of the budget in `docs/RATE-PROBE.md` §1. Longer words are their own message and the file
follows under it: **two sends, two tokens**, and the ack is for the pair — `yes` only when both
landed. When the hub refuses the file before uploading, the words go alone with the line appended,
in one send. When Telegram refuses the upload the words never landed either, so they go again
alone with the line appended — a second send, which is the cost of finding out; when the words had
already gone as their own message, the line goes alone under them. A file shed by the budget, or
refused because the topic is gone, after words that landed is `yes` with **`no-file-unsaid`** and
no line: what refused the file refuses a line too, and the audit already says `shed`. The same word
is used when the line itself was shed or clipped, because the ack is the agent's only evidence
about what is on his phone and the difference between the two is exactly whether there is anything
there to read. Whether a picture
and a document are charged against the twenty-a-minute ceiling like a text is **assumed, not
measured**: they are sends, so they take a turn like any other, and `docs/RATE-PROBE.md` should
list the measurement as still owed.

### 14.5 What each engine receives

The adapter decides how a path reaches an agent's turn; the hub decides only that it is a path.
Both of ours, measured rather than guessed:

**Claude.** The tool server hands the operator's words into the turn as a channel message
(`deliver()`), whose content is a string. A path in that string is enough: the agent reads the
file with its own tools, which is the whole point of mounting the directory at the same path. So
the message is the caption, then one line per file naming the path, the mime, the size and the
reported filename — or, for a file that did not come through, one line saying so and why. The
exact words are the adapter's (§11, item 2), and the ack carries `files: 1` the moment the
notification is written, at the same honesty as its `accepted` today.

**opencode.** The v1 prompt body the watcher already uses has a file part, read off the running
server's own OpenAPI at `/doc` (opencode 1.18.25) and not guessed:

```
POST /session/{id}/prompt_async
{"parts":[{"type":"text","text":"<his caption>"},
          {"type":"file","mime":"image/jpeg","url":"file://<the path in the frame>","filename":"<the reported name>"}]}
```

`FilePartInput` requires `type`, `mime` and `url` and forbids unknown keys; `filename` is optional;
`source` is optional and is best omitted, because its own shape requires fields the watcher has no
honest value for. **The server, not the model, reads the file**: for a `file:` URL it reads the
path from disk and rewrites the part as a `data:` URL for the model (a `text/plain` file is run
through the server's own Read tool instead). So the path must be readable by **the opencode server
process inside the wall** — exactly the same-path rule, and nothing else. A `mime` the frame does
not carry is sent as `application/octet-stream`. A path the server cannot read is a prompt that
fails, which the watcher already turns into `ack{status: refused, reason}` under a deadline
(§13.9); the reason names the mount — *the engine could not read the file at `<path>`; is the
media directory mounted in the wall?* — and that is one line in his topic, never a silent drop.

**Up is the same under both engines.** The agent's `reply` and `done` tools gain one optional
parameter, a path to a file, and the tool server does the copy and the naming of §14.3 whichever
engine started it. The watcher carries nothing up.

### 14.6 A wall, worked — the two extra bind mounts

§10 mounts two things; a wall that carries files mounts four. `STATE` is the hub's state directory
as the hub sees it (§14.1), `PROJECT_ID` is from `herdr-tg projects --json`, and `ADDRESS` is the
address the dispatcher minted — the same value it puts in `KICKOFF_HUB_ADDRESS`, or `-` when it
sets none:

```
(umask 077 && mkdir -p "$STATE/media/$PROJECT_ID/$ADDRESS" "$STATE/outbox/$PROJECT_ID/$ADDRESS")
docker run --rm --init \
  --user "$(id -u):$(id -g)" \
  -v "/run/user/$(id -u)/kickoff:/run/user/$(id -u)/kickoff" \
  -v "$repo/.kickoff/hub.token:/run/secrets/hub.token:ro" \
  -v "$worktree:/workspace" \
  -v "$STATE/media/$PROJECT_ID/$ADDRESS:$STATE/media/$PROJECT_ID/$ADDRESS:ro" \
  -v "$STATE/outbox/$PROJECT_ID/$ADDRESS:$STATE/outbox/$PROJECT_ID/$ADDRESS" \
  -e KICKOFF_HUB_PROJECT_DIR=/workspace \
  -e KICKOFF_HUB_ADDRESS="$ADDRESS" \
  -e KICKOFF_HUB_TOKEN_FILE=/run/secrets/hub.token \
  <image> kickoff-hub-attach --opencode http://127.0.0.1:9700 --run opencode serve --port 9700 --hostname 0.0.0.0
```

Under bwrap the two lines are `--ro-bind "$STATE/media/$PROJECT_ID/$ADDRESS" "$STATE/media/$PROJECT_ID/$ADDRESS"`
and `--bind "$STATE/outbox/$PROJECT_ID/$ADDRESS" "$STATE/outbox/$PROJECT_ID/$ADDRESS"`; bwrap
refuses a source that does not exist, which is what the first line is for. The `umask` is there
because `mkdir -m` sets the mode on the last segment only, and the hub checks every segment.

Line by line, why each is what it is:

* **the same path on both sides.** The frame carries the host path, and the hub cannot know a wall
  exists, let alone what it mounted where; a wall with a different `$HOME` inside still gets the
  host's string in the frame, exactly as it gets the host's socket path (§10). Under
  `bwrap --unshare-user --uid 0` this is the socket's story again: the path in the frame is the
  operator's, so mount it at the operator's path.
* **the conversation's directories, never the trees.** Mounting `$STATE/media` whole would let this
  wall read every conversation's files; mounting `$STATE/outbox` whole would let it write into
  another conversation's outbox, from which the hub would happily upload under that conversation's
  name.
* **media read-only.** The hub is the only writer, and a wall that could write there could plant a
  file and then be told, by the hub, to read it.
* **outbox read-write, and the hub still trusts nothing in it.** §14.3's five checks are what make
  a read-write mount from an untrusted side safe; the mount does not.
* **the same uid.** §10 already requires it for the socket; the ownership check requires it again,
  and a wall as another uid writes files the hub refuses to send.

What happens when each is missing:

| missing | what you see |
| --- | --- |
| the media mount | Claude: the agent is told a path and its read fails with "no such file"; the ack still says `files: 1`, because the notification was written and the adapter cannot see inside the engine's tools. opencode: the server's read fails, the prompt fails, the watcher refuses with the reason above, and he reads one line. `--check` does not test this mount today. |
| the outbox mount | the tool server's copy fails and the tool result says so, before the wire; nothing reaches the hub. |
| the outbox, made as the wrong uid or with a wider mode | the hub refuses every file from that conversation, with a log line naming the directory; the words go, with the line appended. |
| the directories, before the wall starts | bwrap refuses to start; docker makes the missing host directory **as root**, which the hub then refuses as not its own — so make them first, as the first line does. |

### 14.7 Not here, said plainly

* **Nothing a sender says names a path, and the hub never builds Telegram's download URL itself.**
  That URL carries the bot token, and the client library the hub uses redacts it from every error
  it returns — and, measured while building this, drops the whole URL from the error when the
  token does not look like one to it, so a misconfigured token is not the one that leaks. A
  hand-rolled fetch would put the token in the first log line of the first failure. `file_path`
  on its own carries no token and may be logged; a test pins that neither the journal nor the
  audit ever carries the token after a failed fetch.
* **No thumbnails, no sniffing, no re-encoding, no unpacking.** What he sent is what is on disk;
  what the agent wrote is what he gets.
* **No cleanup driven by a topic going.** Topic cleanup is its own slice. The sweep does take a
  conversation's directory once it has been empty for the shelf life (§14.1), so entries are
  bounded; nothing watches a topic to do it sooner.
* **Not on `ask`** (§14.2), and not on `beat`.
* **Not measured:** whether a picture and a document are charged against the send ceiling like a
  text; whether a real full-page screenshot is refused by `sendPhoto` for its dimensions or merely
  downscaled. Both are cheap to measure in a throwaway topic and neither changes a frame.
* **`--check` does not yet prove the two mounts.** It proves the socket and the secret; a line per
  mount belongs beside them and is the first thing to add when the build lands.
* **A hard link is refused, and it never was an escalation.** §14.3's fifth check is there because
  the guarantee this document sells — that the hub sends only files written into the outbox for it
  — was not the one the code held, not because a link reached anything a wall could not already
  read. `link` is made in the wall's own namespace, so the reach it adds is a file the wall may
  NAME but not read; the symlink case, which resolves in the hub's namespace and so could reach
  anything at all, was refused from the start.
* **The file-level ownership check is untested without root.** §14.3's third check is made twice —
  on every directory of the walk and on the file — and a test can only move the hub's idea of its
  own uid, which the walk refuses first. The file's check exists for a wall started as root that
  writes a root-owned file into a directory that IS the hub's; proving it needs a root-owned
  regular file inside a hub-owned `0700` directory, which no test on this box can make.

---

## Changing this file

Bump the version in the header comment and say what moved. A variable may be added at any time. A
variable may not change meaning without a new name, because the whole point of one namespace is that
a name means one thing.
