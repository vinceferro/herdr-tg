<!-- AUDIT, 2026-09-02. Five auditors probed the hub for what breaks when one enrolled project
     becomes fourteen; every finding was then handed to an independent sceptic briefed to refute
     it, and eight were dropped. Nothing in this document is built. Nothing here changed a line of
     production code, sent a message to Telegram, enrolled a project, or touched the service. -->

# Can the hub serve the herd?

## 1. The answer

**Not yet — but the distance is short, and it is four small changes rather than a redesign.** The
multiplexer's shape is right: identity, routing and double-answer are sound at fourteen for the same
reason they are sound at one, and none of the eighteen gaps below is a rewrite.

**The first thing to bite is not the hub at all — it is the door into it.** `herdr-tg enroll` mints
the secret and writes it into the repo *before* the check that warns you about it runs, and ten of
your thirteen adopted repos plus the `herdr-tg` lane worktree have no rule that would stop git
committing that file — in trees whose own coordinators are authorised to commit and push unattended.

Reproduce it yourself, in one line:

```
for r in ~/Projects/*/; do git -C "$r" check-ignore -q .kickoff/hub.token 2>/dev/null || echo "$r"; done
```

## 2. What is already right — where NOT to spend

Short, because this is the part you can stop reading about.

* **Identity holds.** `SO_PEERCRED` closes a foreign uid silently before anything else is read
  (`hub.rs:543`), the socket is 0600 inside a 0700 directory (`hub.rs:450-456`), and the secret
  resolves to a project by hash with no name on the wire. A hub secret that leaks off the box is not
  usable off the box — which is why the enrolment row below is a rotation problem, not a break-in.
* **Nothing can misroute between projects.** A topic resolves to a project by registry lookup only,
  and topic ids come back from Telegram and are never chosen. The pane-era files still sitting in the
  state directory have **no reader anywhere in the tree** — a grep for every one of their keys
  returns a single doc comment — and an old `t|`/`c|` button fails the callback match and lands on
  "I don't recognise that button." (`bot.rs:334`).
* **The disconnected paths fail closed, in plain words.** A tap at a project that is not there:
  "That project is not connected right now, so there is nobody to tell." (`hub.rs:202`). Text typed
  at one: dropped rather than queued, and it says so (`bot.rs:389`). Neither ever waits.
* **A question cannot be answered twice.** Authorisation is the `answered` field, not the record's
  presence (`hub.rs:161-172`), and a tap on a menu drawn for a session that has since restarted is
  refused with a reason (`hub.rs:632`).
* **One claim per project, and it is exclusive.** A dead pid is evicted, a live one is refused rather
  than swapped out (`hub.rs:709-719`).
* **The budget is correctly implemented and honestly documented.** 18/min under Telegram's 20
  (`queue.rs:25-27`). It is per chat because Telegram's ceiling is per chat, so **the herd's whole
  capacity is about eighteen messages a minute whatever N is** — fourteen projects at one message a
  minute each is lossless; eight at three a minute each is not. That is a number to size the herd
  against, not a defect to fix.
* **There is still no keystroke path**, and the deletion is pinned against the shipped tree.

## 3. The gaps, ranked

Every row traces to a file, and every row survived a sceptic briefed to kill it.

### Blocks the herd

| What breaks | When it bites | Smallest fix |
|---|---|---|
| `enrol` mints the secret, saves the registry and writes `<repo>/.kickoff/hub.token` (`registry.rs:344-346`) — and only then does `gitignore_gap` run and *print* a warning, exit 0 (`cmd/enroll.rs:20` vs `:41`). Ten adopted repos and the lane worktree would commit it; two of those remotes are public. | The first command you type to go from one project to fourteen, eleven times. The consequence is the only irreversible one in this audit: a secret in a public git history cannot be untracked, and the coordinator in that tree commits and pushes without you. | Canonicalise, run the check **before** `registry.enrol`, and make the "git would commit it" branch a refusal that writes nothing — with an opt-out flag, because `enroll` is also the rotation path. Leave the "could not ask git" branch a warning, or a machine without git cannot enrol. |
| The hub holds 256 KiB of a bridge's frames before the pong (`hub.rs:1109`); the bridge is allowed to queue 64 × 64 KiB = 4 MiB (`server.ts:356`). A 16× mismatch. On overflow the buffered frames are dropped with **no ack and no refusal frame** (`hub.rs:1130-1144`), and the bridge's recovery rebuilds its lost set from `pending` only, never from what it already flushed (`server.ts:560`). The bridge reconnects, re-flushes the rest, overflows again. | Any reconnect carrying more than 256 KiB of backlog — about 64 ordinary agent messages. One hub restart at N=14 hands fourteen bridges a backlog at once. Measured against the real bridge: 64 messages, 1.28 MB, destroyed across three refused connections, zero corrections to the agent, nothing on the phone. | Two halves, both needed. Hub: either raise the pre-pong bound to what the bridge may legally hold, or ack each buffered frame `no` before `writer.abort()`, the way the oversize path already does. Bridge: rebuild `lost` from `inFlight` too. |

### Degrades at N

| What breaks | When it bites | Smallest fix |
|---|---|---|
| `messages_for` filters on project and ask id but never on `instance` (`hub.rs:308`), and the bridge's ask ids restart every process — deterministically at `a3`, because the counter is shared with frame ids. So a new session answering its own first question retires the dead session's first question too, stamping a never-answered question with someone else's outcome and then deleting its record. | Every session that dies with its opening question still open — which is the likeliest question to be open. Fourteen self-cycling coordinators accumulate one per abandoned run. | Add `&& r.instance == instance` at `hub.rs:308` and thread `instance` through `retire` (it is already in `handle`'s scope). Then retire the evicted instance's open questions at `hub.rs:719` with an honest note, or they sit on your phone forever. |
| A tap is marked answered (`hub.rs:649`) *before* the caller delivers (`bot.rs:308`). A delivery that fails burns the question permanently — nothing ever clears `answered` — and the operator is told first "that project is not connected" (false; it was merely behind) and then, on retry, "That one has already been answered. I have not sent anything." (also false). | A full outbox, which the code documents as expected behaviour. Reproduced with the real hub, no race needed; the keyboard also stays live and can never answer. | On `sent == false`, treat it as a withdrawal: retire the keyboard with a note saying it was not sent, and forget the record, so the agent's re-ask mints a fresh menu. |
| The wire defines three delivery values; the bridge branches on two. `if (frame.delivered !== 'no') break` (`server.ts:771`) makes `unseen` silently mean success. Meanwhile the hub writes no ledger record for an unseen ask (`hub.rs:1331`), so if the message *did* land, the keyboard is live, refuses every tap, and can never be retired. | Any network error on a send (`surface.rs:126-132`). Rate scales with the herd. The agent's only record says the operator was asked and an answer is coming; no correction ever arrives. | One line: `if (frame.delivered === 'yes') break`, plus an `unseen` sentence through `deliver()`. Do **not** touch the hub — refusing to guess a message id is the fix for a shipped defect. |
| `MAX_PACE_WAIT` / `MIN_GAP` = 10s / 1s = 10 (`hub.rs:67`, `:882-909`). Because a claim is exclusive and each read loop is sequential, the permit queue is at most N deep — so below eleven live CONNECTIONS the deadline can never shed anything, and at eleven it starts. Nothing tells the operator. **N is no longer the number of projects**: since 2 September a worktree is a connection of its own, so eleven is reached by one repo's lanes on an ordinary dispatch day. It is also the LOOSER of the two limits — see the row below. | The eleventh live connection. A synchronised burst sheds N−10 messages with the per-minute bucket still full. `HUB-DESIGN.md:103` promises "one `throttled` line… never a silent loss"; that half was never built. | Not a per-project sub-bucket — that redistributes and saves nothing. One throttle line per project per cooling-off window, or one line in General, so you can learn the herd is over the ceiling. |
| The per-chat ceiling is 18/min (`queue.rs:27`) and a brand-new conversation now costs **two** of it before the agent has said anything — one to make the topic, one to greet it — and a third for its first message. So the binding figure is about **six new conversations a minute, shared across the whole forum**, and it is spent by worktrees arriving rather than by projects being enrolled. The agent is told honestly (`too_fast`); the operator is told nothing at all. | A dispatch morning. Twelve worktrees arriving inside one minute cannot all open, and the ones that cannot are invisible to him rather than merely late. | Same as the row above — one throttle line per cooling-off window, in General or in the topic. Not by dropping the greeting: a topic with no message in it is one Telegram does not list at all. |
| A Telegram 429 falls through to the catch-all and is classified a permanent refusal (`surface.rs:134`); the seconds survive only as a string in an audit line; and nothing anywhere feeds a flood-wait back into the budget — `take()` is the only writer (`hub.rs:893`). The hub keeps sending into the wall. | Once the chat is actually pushed past 20/min, which is what the row below makes possible. Bounded (~18 lost messages per flood-wait, each honestly acked) but self-sustaining while it lasts. | Carry the wait as a value rather than a string and drain the chat bucket by it. Match both 429 shapes — `retry_after()` returns `None` for the one the repo already has a fixture for. |
| `send_into` says "Every hub-owned write goes through here" (`hub.rs:851`). Six writes do not — `create_forum_topic` was routed through the budget on 2 September, because a topic per worktree made it fire a dozen times a day instead of once in a project's life. What is left: the retirement edit (`hub.rs:1433`, `hub.rs:1478`), the tap confirmation and callback answer (`bot.rs:339`, `:344`), the relay error reply (`bot.rs:403`), the command reply (`bot.rs:448`). | The one that scales is `hub.rs:1478`: it runs in each connection's own read loop, takes no permit and honours no gap, and there are fourteen of them. Fourteen agents timing out a backlog of asks is a concurrent unpaced burst the ceiling cannot see. | A `Budgets::spend(chat)` that takes a token and cannot refuse — routing a retirement through `send_into` would let a shed leave a live keyboard. Apply at `hub.rs:1478` first. Fix the comment either way; it is what let seven accumulate. |
| **Built 5 September** — `the_project_list_calls_a_project_connected_only_while_its_bridge_is_on_the_socket`, and `projects_json_says_connected_only_from_the_live_claims_map` for the inventory. As audited: `/projects` promised "which projects are enrolled, and which are connected" (`bot.rs:76`) and rendered `p.topic_id` (`bot.rs:428`) — a binding, not a connection. `topic_id` is permanent after the first connect, so every project that has ever run reads the same. It also never calls `reread()`, so a project enrolled while the hub runs is **absent from the list entirely** until its bridge connects. | The moment there is more than one project. It is your only fleet view, and the only way to learn the truth is to type at each topic in turn. Nothing else reports presence: the greeting fires once in a project's life, and a disconnect posts nothing. | `registry.reread()` in the digest, plus a `connected_ids()` over `claims` — and rewrite the comment at `hub.rs:747-748`, which argues against a production liveness read for the *delivery* path, not for a snapshot a human reads. Cheap fallback: change `bot.rs:76` so it stops promising. |
| Nothing records a bridge arriving or leaving. `hub.rs` holds one `tracing::info!` in the whole file — the eviction at `:717` — the ordinary disconnect logs at `debug` (`hub.rs:1217`, one level under the unit's `RUST_LOG`), and a clean `bye` logs nothing at all. A *refused* arrival is recorded, in both the ledger and the journal. | At N=14 the audit can answer "did anything try to come back" and cannot answer "when did that session go away". You reconstruct the day from memory. | `debug!` → `info!` at `hub.rs:1217`, a line on the clean-EOF path, and an audit record on successful `claim` and `release` through the writer that already exists. |
| **Built 5 September** — `a_project_switched_off_at_the_terminal_loses_its_live_connection_now`, `re_enrolling_a_switched_off_project_does_not_switch_it_back_on`, `the_switch_reaches_a_bridge_whose_backlog_has_filled_the_hubs_queue_at_once`. As audited: nothing could switch a project off. `enabled: true` was hardcoded in `registry.rs`'s enrol path and there was no setter, no CLI verb and no command — the only lever is hand-editing the state file. Re-running `enroll` (the documented answer to a leaked secret) silently switches a hand-disabled project back on. And `enabled` is read only at `hello`, so even set false it cannot quiet a bridge that is already connected. | When one of fourteen is loud and everyone shares the 18/min. The answer today is a JSON editor, and it does not stop a live bridge. | Carry `enabled` forward in `enrol` the way `topic_id` already is (`registry.rs:333`), add terminal-only `disable`/`enable`, and make disabling drop the live claim — otherwise you tap off and the flood continues. |
| A bridge that loses the claim race gets a refusal on the wire and one audit line (`hub.rs:1051-1065`) and **nothing in the topic**. `HUB-DESIGN.md:101` specifies the topic post; it was never built. | Two overlapping sessions in one repo — it happened four times on this box in seven seconds with one project enrolled. The losing agent *is* told, in its own turn; no human is reading that turn. | Post into an **already-bound** topic only — never through `topic_for`, which would create and greet a topic for a connection that never proved it was live — and name no pid. |
| Command replies are unthreaded (`bot.rs:416` → `bot.rs:446`), so `/projects` and `/help` typed inside a project's topic are answered in the forum's General. The sibling relay path threads carefully (`bot.rs:406`). | When you live inside topics, which is what fourteen projects means. The answer is not lost — General is a visible row — it is simply not where you were looking. | Give `reply` a `thread: Option<i32>` and set `.message_thread_id(...)`, exactly as `bot.rs:406` does. |
| **Built 5 September** — `a_command_aimed_at_another_bot_is_ignored_rather_than_relayed`, and `a_line_whose_first_word_carries_an_at_sign_is_his_words_not_another_bots_command` for the guard the fix needed. As audited: `Command::parse(text, "herdr_tg")` in `bot.rs` hardcoded a bot username the bot does not have. Any `@`-suffixed command therefore fails to parse and falls through to the relay branch — so `/projects@<the real bot>` is delivered verbatim into a coding agent's turn and recorded in the ledger as a message you meant to send. | Whenever group habit makes you type the suffix. Nothing calls `setMyCommands`, so there is no menu that would insert it for you — it takes typing it by hand. | Carry `me.username()` (already fetched at `bot.rs:105`) into `Ctx` and pass it at `bot.rs:369`. Only once the real name is used does dropping a `WrongBotName` become correct. |

### Cosmetic

| What breaks | When it bites | Smallest fix |
|---|---|---|
| The containment guard is one-directional (`registry.rs:307`): it asks whether the new repo is inside an existing one, never the reverse. Enrolling child-then-parent is accepted and rebuilds exactly the nested state the refusal exists to forbid — two topics for one working tree, decided by where `claude` was started. | Only once a monorepo package and its root are both adopted. None of your fourteen is nested today. | Add the symmetric `find`, with **its own** error variant — the existing sentence would print "mono is inside mono/packages/foo" and tell you to enrol the child. |
| `AckWhy::TooFast` is documented as "The project's own rate budget refused it" (`frame.rs:86`). There is no per-project rate budget; the reason is the shared chat bucket or the pacing deadline. | Never, for the operator — the bridge already renders it correctly as chat-wide. It misleads whoever writes the next bridge. | One line of comment. |
| Both project listings iterate a `BTreeMap` keyed by a hash of the repo path (`registry.rs:247`), so fourteen rows come out in an order that is neither alphabetical, nor by activity, nor scannable. | Reading a fourteen-row list on a phone. Nothing fails; there is no per-row action to mis-hit. | Sort by title before the loop at `bot.rs:427` and `cmd/enroll.rs:52`. |
| Three pane-era files sit in the state directory with no reader, no writer and nothing that cleans them (one of them 0644, which is the mode a previous review killed that era over). They name eight topic ids in your forum from the deleted screen-scraper. | At fourteen, the topic list is thirteen live topics interleaved with eight dead ones that look identical from a phone. | Read the eight ids out of the file *first*, delete or retitle those forum topics, then delete the files. Do not pin this with a test — the state directory is a developer's home, and such a test is red on your box and vacuously green everywhere else. |

## 4. The smallest first slice

The least work that lets you enrol a second project and trust it. Four changes; the last is one line.

**1 — the enrolment door fails closed.** Move `gitignore_gap` ahead of the mint in
`crates/kickoff-channel/src/cmd/enroll.rs` and make the "git would commit it" branch return `Err` with
nothing written, behind an opt-out flag so a rotation is still possible. Leave the "could not ask
git" branch a warning.

> `a_repo_whose_git_would_commit_the_secret_is_refused_before_the_secret_exists`

**2 — a retirement stays inside its own session.** `crates/kickoff-channel/src/hub.rs`: add `instance` to
the filter in `messages_for`, thread it through `retire` and its caller, and retire the evicted
instance's open questions where the eviction already happens.

> `an_answer_from_one_session_never_rewrites_the_question_another_session_left_open`

**3 — the list stops lying about who is running.** `crates/kickoff-channel/src/bot.rs` and
`crates/kickoff-channel/src/hub.rs`: `reread()` the registry in `projects_digest`, and render from the
claims map rather than from `topic_id`. With one project you knew; with two you do not.

> `the_project_list_calls_a_project_connected_only_while_its_bridge_is_on_the_socket`

**4 — the third delivery value reaches the agent.** `plugins/kickoff-channel/server.ts:771` becomes
`if (frame.delivered === 'yes') break`, with an `unseen` sentence saying an answer may never come;
add the case to `plugins/kickoff-channel/test-against-a-fake-hub.ts`, which has never covered it.

> `a_send_the_hub_could_not_confirm_tells_the_agent_that_an_answer_may_never_come`

While you are in `bot.rs` for 3, the failed-delivery burn (§3, second row under *Degrades at N*) is four lines in
the same handler, and the unthreaded command reply is one.

Everything else — the pre-pong bound, the eleven-project pacing cliff, 429 backpressure, the
unmetered writers — is real and can wait for the fourth project, not the second.

## 5. What this audit did not check

* **The live round trip still has not run.** Every finding stops at the Bot API. Nothing was posted,
  tapped, or rendered on a real phone; every operator-facing string here was read, not seen.
* **Whether Telegram charges an edit or a topic creation against the same 20/min group ceiling.**
  That probe is `HUB-DESIGN.md:384`, raised and never run. Two severity claims in §3 depend on the
  answer and are stated as unproven.
* **Nothing ran at fourteen for real.** No fourteen bridges dialled the live hub. The N figures come
  from probes, from the real bridge against a fake hub, and from the repo's own tests with one
  constant changed. The service was never started, stopped, or reconfigured.
* **Nothing was profiled.** The ledger is one global mutex whose save does blocking file I/O while
  held; the contention at fourteen was reasoned about, not measured. Memory, file descriptors and
  the unbounded growth of the on-disk ask ledger are all unexamined.
* **The forum's actual topic list was never read** — that needs an API call. The eight dead pane-era
  topic ids come from a local file; whether those topics still exist is unverified.
* **`herdr-client` and the four read-only subcommands were not audited.** They do not touch the hub.
* **The watchdog was read, not exercised.** It alarms when the hub dies and never for one project.
* **Lanes.** `INTERFACES.md` leaves "is a lane its own topic?" open, and a lane worktree is one of the
  trees that would commit the secret. Nothing here answers the lane question.
* **Seams ② and ④** — the opencode adapter and the launcher — are not built, so there was nothing to
  audit.
* **Concurrent enrolment.** Two `herdr-tg enroll` at once takes a lock that was read but not raced.
