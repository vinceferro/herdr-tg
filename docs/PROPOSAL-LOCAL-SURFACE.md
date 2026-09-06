<!-- PROPOSAL, 6 September 2026. Nothing was built to write this. Three designs were drafted against
     HEAD f5ecb11 and three judges read the code each one cited; this is the winner with the grafts
     the judges named, and with the four places the judges found the designs had misread the code
     corrected. Every file:line below was read at f5ecb11; every "I did not check" is literal. Paths
     are repo-relative; `<state>` is the hub's state directory (`crates/herdr-tg/src/lock.rs:39`),
     and `~` stands for the home directory. No secret, chat id, user id or home path appears here. -->

# The local surface

**The laptop sees the same conversations the phone sees, joined to the panes herdr already shows,
and answers a question through the one ledger the phone answers through. No new socket, no new
frame, no new mount, and nothing in this repo learns a herdr write method.**

---

## 1. The wish, and the two views it names

The operator's words, verbatim: *"I would love for telegram and local herdr to be connected to the
same fleet of agents."* Earlier in the same conversation: would rooms *"be available thru herdr
locally as well"*.

Read plainly: from his laptop — herdr, a terminal — and from his phone — Telegram — he wants to see
and steer the **same** agents, not two disjoint views. Today they are disjoint. `herdr-tg status`
prints herdr's snapshot and reads nothing of the hub's (`crates/herdr-tg/src/cmd/status.rs:14-32`,
`render.rs:23-45`); the phone shows the hub's topics and knows nothing of a pane. On this box on
6 September herdr showed seven workspaces and eleven panes, the hub showed five conversations with
three connected, two of the enrolled projects had no herdr workspace, and several herdr workspaces
were not enrolled. Two lists, no join.

There are two layers in the wish, and they must be named apart because they have different owners,
different sources of truth, and different failure modes:

- **The process view** — which agents run, where, and what their screens say. herdr's world. Its
  truth is `session.snapshot` (`crates/herdr-client/src/client.rs:287`): a pane has a `cwd`, a
  `foreground_cwd`, an `agent_status` of idle/working/blocked/done/unknown, and sometimes an
  `agent_session` (`crates/herdr-client/src/proto/model.rs:217-256`). herdr carries **no structured
  question** — only status words and screen bytes — which is why the screen-scraper died
  (`docs/SLICE-3-REVIEW.md`).
- **The conversation view** — what an agent said, what it asked, and what he answered. The hub's
  world. Its truth is the claims map (one live connection per address, `hub.rs:1399-1401`,
  `hub.rs:1458`) and the tap ledger (`AskRecord`, `hub.rs:492-548`; `AskLedger`, `hub.rs:812`),
  keyed by the Telegram message a keyboard sits on (`hub.rs:818-824`).

The laptop already has the process view and the phone already has the conversation view. What the
wish asks for is each side getting the other's — and the rule this proposal holds to is that a view
crosses over **as a read**, and steering crosses over **only through the ledger**. That is the
"same fleet": one record of what was asked and what was answered, whichever screen he happened to
be looking at.

---

## 2. The design

### 2.1 The shape, and why this shape

The hub stays the only process that holds the claims and the ledger. It gains **one more file it
writes for a process that is not the hub** — a mirror of the conversation view — on the exact
discipline `hub.connected.json` already has (`crates/herdr-tg/src/presence.rs:1-27`: a file, not a
query, because every socket connection presents a secret proving one project and a secret-less
query would be a new frame kind; written whole under the hub's pid; believed by a reader only when
the lock's holder is alive, is a herdr-tg, and wrote it, `presence.rs:148-166`).

`herdr-tg status` reads that mirror beside herdr's snapshot and prints one table. Answering from the
laptop is one verb, `herdr-tg answer`, that hands the hub a **selection** — conversation, address,
ask id, option id — through the same terminal-to-running-hub channel `herdr-tg disable` already
uses: a file in the private state directory that the hub's watch loop polls every second
(`hub.rs:2210-2229`). The hub resolves it through the same steps a phone tap goes through
(`hub.rs:1934-2017`), delivers the same `choice` frame (`crates/hub-proto/src/frame.rs:472-476`),
and retires the same keyboard on the phone (`Surface::retire_buttons`, `hub.rs:378-384`).

Why not a second socket on the hub: it is a second admission surface with a thinner trust root than
the first (no secret, uid only), and the hub has exactly one socket today for a reason that took a
review round to learn (`hub.rs:12-19`). Why not put the fleet inside herdr panes and join there: a
walled room and a systemd unit (`deploy/kickoff-hub-attach@.service`) have no pane, herdr's p20
schema has no method that adopts a process it did not spawn (`crates/herdr-client/tests/fixtures/herdr-schema-p20.json`
lists `workspace.create{cwd, env, focus, label}` and no `pane.run`), and the laptop-side answer in
that design is the terminal, which reaches the ledger only through `ask_resolved` — and that path
does not write the authorisation (§2.4). Both are folded into §4.

### 2.2 Wire, files, verbs — exactly

**hub-proto: unchanged.** Zero new frames, zero changed fields. One existing frame stops being
thrown away: `beat{state, note?}` (`frame.rs:350-354`; `BeatState` = working|idle|blocked|done,
`frame.rs:259-264`) is today acked and discarded (`hub.rs:4403`). The hub will keep the last beat
per claim in memory and put it in the mirror. Said honestly: the only beat any adapter sends today
is `{state: 'idle'}` on opencode's `session.idle` (`adapters/kickoff-hub-attach/opencode.ts:668-673`,
forwarded by `relay.ts:770`); `plugins/kickoff-channel/server.ts` sends none. So on day one the
beat is a liveness tick with one word, not a state column. An adapter that wants a state column on
the laptop maps its engine's events onto the other three words; that is the adapter's change, and
it is optional.

**herdr protocol: unchanged, read only.** `session.snapshot` via `HerdrClient::snapshot`
(`client.rs:287`), which `status` already calls. Fields used: `PaneInfo.cwd`, `foreground_cwd`
(`model.rs:245-247`), `WorkspaceInfo.worktree{repo_root, checkout_path, is_linked_worktree}`
(`model.rs:333-339`), `agent_status` (`model.rs:49`), and in slice 4 `PaneInfo.tokens`
(`model.rs:255`). Nothing else.

**New file A — the mirror.** `<state>/hub.mirror.json`, 0600, temp-and-rename, rewritten whole and
**unlinked when a rewrite fails** — the exact discipline of `Presence::write` (`presence.rs:87-104`,
and the comment there is the reason: temp-and-rename leaves an old file in place on a full disk, and
that file carries this hub's pid, so every check the reader makes holds and it goes on naming what
has since left). Read only through `vouched_for`'s three checks (`presence.rs:156-166`).

```
{ "hub_pid": N, "at": <secs>,
  "conversations": [
    { "id": "p-…", "address": "engineering" | null,
      "title": "<the registry title>",
      "connected": true, "instance": "<hello.instance>", "pid": <claim.pid> | null,
      "beat": { "state": "idle", "note": null, "at": <secs> } | null,
      "asks": [ { "ask_id": "a3", "text": "<the clipped text>", "at": <secs>,
                  "options": [ { "option_id": "y", "label": "Overwrite" }, … ],
                  "answered": null | "y" } ] } ] }
```

Everything in it is state the hub already holds: `Addr` (`hub.rs:1458`), `Claim.pid` and
`Claim.instance` (`hub.rs:1399-1401`), and the `AskRecord` fields `project, lane, ask_id, options,
text, instance, answered` (`hub.rs:492-548`). **Not in it, by rule:** chat id, topic id, message id,
user id, the repo path, `hello.repo`. The ledger's own key `(chat, msg)` (`hub.rs:818-824`) never
leaves the hub. A question is named outside by `(conversation, address, instance, ask_id)` — which
is precisely the tuple `AskLedger::messages_for` already indexes (`hub.rs:918-920`), for the reason
its comment gives: ask ids repeat across sessions and across lanes, so nothing shorter names one
question. A test pins the rule: `the_mirror_names_no_chat_and_no_person`.

Rewritten at every point the hub already touches presence or the ledger: `note_who_is_connected`
(`hub.rs:2124`, called with the claims lock held), `record` (the ask site, `hub.rs:4318-4372`),
`mark_answered` / `mark_unanswered` / `forget` (`hub.rs:879-903`), retirement (`hub.rs:5126-5158`),
and — coalesced to at most one write a second — on a beat. **Regenerated whole from the ledger and
the claims map every time, never patched**: it is a derived copy, and a derived copy that is patched
incrementally is the one that drifts.

**New directory B — the answer drop.** `<state>/answers/`, 0700 via `private_state_dir`
(`crates/herdr-tg/src/conversations.rs:437`). `herdr-tg answer` writes `<nonce>.json` =
`{"conversation":"p-…","address":"engineering"|null,"ask_id":"a3","option_id":"y","at":<secs>}` at
0600 by temp-and-rename. The hub's registry-watch loop (`hub.rs:2210-2229`: a stat every second, a
re-read on change) gains a second directory to sweep. For each request the hub writes
`<nonce>.result` = `{"outcome": "sent" | "already_answered" | "no_record" | "not_connected" |
"restarted" | "not_an_option" | "not_delivered", "label": "…"}` and unlinks the request. The
command waits up to three seconds for the result and otherwise says *"the hub has not answered yet
— check `status`"*, never *"not sent"*: an answer that was applied and reported as not applied is
the lie that gets a question answered twice by a human hand. A request older than sixty seconds is
refused unread — a drop a dead command left behind must not answer a question that was re-asked
under the same id by the next session (`hub.rs:912-917` is why the id alone never suffices).

Why a file and not a frame: `presence.rs:11-13` — a secret-less query over the hub socket is a new
frame kind. Why a file and not a second socket: §4.

**New verb.** `answer <conversation-or-title>[/<address>] <ask_id> <option_id>` under `enum Cmd`
(`crates/herdr-tg/src/main.rs:76-77`). The conversation accepts the id or the registry title, as
`status --workspace` accepts an id or a label (`cmd/status.rs:49-58`). **It answers only by option.
It never carries text.** Typed steering from the laptop is refused on purpose: the agent's own
terminal is a keystroke away from the laptop, and a typed line toward an agent from a file is the
shape this repo has deleted twice.

**Changed verb.** `status` reads the mirror after the snapshot and renders a second block rather
than a wider row (`render.rs:23-45` today): one line per herdr pane as now, with a seventh column
`conversation` = the joined title or `—`; then one line per conversation with no pane (`hub only:
<title> · connected · 1 open question`); then, under each conversation, its open asks as
`? a3  Overwrite it?  [y] Overwrite  [n] Keep`. `--json` keeps the snapshot envelope byte for byte
(`cmd/status.rs:27-29` promises it as the proof surface) and adds nothing; a new `--hub-json` prints
the mirror verbatim.

**Reused, in the hub.** `resolve_tap` (`hub.rs:1934-2017`) is refactored — not copied — into one
core `resolve(answerer, chat, msg, option)`. `Answerer::Phone{user}` runs `chat_is_allowed`
(`hub.rs:1893`) and `standing_of` (`hub.rs:1909-1930`) exactly as today. `Answerer::Keyboard` runs
neither, because `standing_of` refuses a `None` user and a zero user by design (`hub.rs:1912`,
`config.rs:249-251`) and the keyboard has no user: its standing is the 0700 directory, the same uid
gate the hub's own socket trusts first (`hub.rs:12-15`). Everything after standing is one function
for both: record exists → the option is one the bridge wrote down (`NotAnOption`) → not already
answered → a claim holds `record.addr()` → `claim.instance == record.instance` (`Restarted`) → a
**fresh re-read under the ledger lock, then `mark_answered`, fail-closed if the write fails**
(`hub.rs:1997-2017`, and the comment at 1997 is why it is here and not after delivery). Then
`deliver(addr, Choice{msg_id, ask_id, option_id})` (`hub.rs:2029`; `try_send`, never blocks), then
`answered_from_phone` (`hub.rs:5001`) with its note parameterised — *"answered from your phone —
{label}"* / *"answered at the keyboard — {label}"* — and `withdraw_undelivered` (`hub.rs:5058`)
when delivery reported false. The keyboard path finds the ledger key by
`messages_for(addr, claim.instance, ask_id)` (`hub.rs:918`) and refuses unless it returns exactly
one message.

**The join key, day one.** For a pane, take `foreground_cwd` else `cwd`. Find its main working tree
— herdr's `worktree.repo_root` when `is_linked_worktree` is set, else `git rev-parse
--git-common-dir` from the CLI: `cmd/enroll.rs:313,330,387` already shell to git **in the CLI and
never in `serve`**, and this keeps that line. Then `repo_key` (`conversations.rs:72-75`) → `linked`
(`conversations.rs:228-240`) → the seed's id. For a linked worktree the lane is git's worktree name
(`.git/worktrees/<name>`, the derivation the bridge uses — memory: *a lane takes git's worktree
name*), matched against the `lane_topics` keys (`registry.rs:92`). Two rules the path join must
keep or it becomes a guess:

1. **A seed with rooms is refused, never guessed.** A room's `repo` is its seed's
   (`docs/CAPABILITIES.md:131`: *"join on `project_id`, never on the path alone"*), so a pane in
   that repo could be the seed or any of its rooms; `status` prints *"several conversations here"*
   and joins nothing until slice 4 stamps the key.
2. **Two panes in one repo both join the same seed**, and `status` shows a conversation with
   several panes, marking which carries an agent by `agent_status`. A shell `cd`'d out of the repo
   unjoins, and that is correct.

**The join key, later (slice 4).** Kickoff stamps the pane it starts with the hub's own key:
`pane.report_metadata{pane_id, source: "kickoff", tokens: {kickoff_hub: "<conversation>[/<address>]"},
ttl_ms}` — the schema allows sixteen names matching `^[A-Za-z0-9_-]{1,32}$` with string-or-null
values, `ttl_ms` in `1..=86400000`, and `applies_to_source` (`herdr-schema-p20.json`,
`PaneReportMetadataParams`); herdr-client already decodes it as `PaneInfo.tokens` (`model.rs:255`).
The value is the same `KICKOFF_HUB_CONVERSATION` / `KICKOFF_HUB_ADDRESS` kickoff already holds
(`docs/ATTACHING.md:234-235`). Nothing new is minted. `status` prefers the stamp over the path. It
lives in display metadata and **not in the environment** — `HERDR_PANE_ID` reaching a bridge three
levels down is incident 1 of `docs/ATTACHING.md:566-569`, and a join key that rides the environment
would be the same incident with a different name. `ttl_ms` so a crashed launcher cannot leave a
stale stamp on a reused pane; a null value on stop clears it. A stamp naming a conversation the
registry does not know is printed as *"not a conversation here"*, never acted on.

**Guard widening.** `WRITE_NAMES` (`crates/herdr-client/tests/no_live_write_call_site.rs:94`) names
three methods, and the scanner walks every `.rs` under the workspace root (`no_live_write_call_site.rs:17-20`)
matching bounded identifiers (`no_live_write_call_site.rs:1309-1325`). Add `report_agent`,
`report_agent_session`, `report_metadata`, `clear_agent_authority`, and `agent.prompt` — spelled
in full, because the bare word `prompt` is already in `render.rs:205` and `summarize.rs:35` and
would red the tree on day one. `report_*` and `clear_agent_authority` appear today only in
`crates/herdr-client/src/proto/model.rs`, the defining crate, so the widening is green on day one.
This is what makes the one herdr write in the whole design kickoff's forever: this repo will not be
able to spell it.

### 2.3 Answered once — the walk

1. An agent in lane `engineering` of project P sends `ask{a3, "Overwrite it?", [y, n]}`. The hub
   sends it to the topic first (`say_as`, `hub.rs:4313-4315`) and writes the record **only for a
   message that exists**: `AskRecord{project P, lane engineering, a3, options, instance I, pid,
   answered: None}` keyed `(forum, msg)` (`hub.rs:4318-4372`; the comment at 4318 says why — a
   record for a message nobody can see, or against a guessed id, is the "Reject confirmed Allow
   always" defect). The mirror is rewritten: P/engineering has one open ask `a3`.
2. **Phone:** the message with two buttons. **Laptop:** `herdr-tg status` shows the herdr pane in
   that worktree joined to *P · engineering*, and under it `? a3 Overwrite it? [y] Overwrite [n]
   Keep`. (`blocked` beside it comes from herdr's own detection of the pane, if herdr can see it;
   from a beat only once an adapter sends one, §2.2.)
3. He runs `herdr-tg answer P/engineering a3 y`. The request lands in `answers/`; within a second
   the hub reads it, resolves `(P, engineering, live claim's instance I, a3)` to exactly one
   `(chat, msg)`, and runs the shared core: record present, `y` is written down, `answered` is
   `None`, a claim holds P/engineering, `claim.instance == I`, `mark_answered` under the lock.
   `deliver(Choice)` → true. The phone message is edited to *"Overwrite it? — answered at the
   keyboard — Overwrite"* through `retire_buttons`, and only when that edit succeeds is the record
   forgotten (`hub.rs:5013-5020`: *"the buttons are gone, therefore the record may go", never the
   reverse*). Mirror rewritten: `a3` gone. Result file: `sent`.
4. **The race, both directions.** He taps `n` on the phone in the same second: `resolve_tap`
   re-reads the record under the ledger lock after the keyboard path's `mark_answered` and returns
   `AlreadyAnswered` (`hub.rs:2001-2008`) — toast *"that has already been answered"*, nothing
   delivered. Tap first, keyboard second: the keyboard path reads `answered: Some(n)` and the result
   is `already_answered` with the label *Keep*. Both writers serialise on `self.ledger.lock()`;
   there is one file, `asks.json`, and one field, `answered`, which the record's own comment calls
   *the authorisation* (`hub.rs:535-548`).
5. **Delivery fails** (outbox full, bridge gone). `withdraw_undelivered` (`hub.rs:5058-5092`) does
   what it does for a phone tap, and this must be stated as the code has it, not as two of the three
   drafts had it: on a **successful** edit it retires the buttons with *"not sent — nothing here
   could be reached"* and **forgets** the record (`Withdrawal::Retired`, `hub.rs:5076-5079`); only
   when the edit itself **fails** does it `mark_unanswered` so the still-live keyboard can be tapped
   again (`hub.rs:5080-5088`). So the question is closed on both surfaces, not re-opened — the
   mirror drops `a3`, the result is `not_delivered`, and the agent's next `ask` is a new record. One
   truth either way.
6. **A question the phone could not carry** — an option id over the button limit, refused at
   `hub.rs:4281-4310`; or a send that was shed — is never recorded, so the laptop cannot answer it
   either. Consistent, and said in the topic. **A question with no options** is a typed-answer
   question (`hub.rs:4311-4315`); it appears in the mirror as open, and `answer` refuses it — the
   phone, or the agent's own terminal, is where it is answered.
7. **Answered at the agent's own terminal.** The adapter sends `ask_resolved{how: answered}`
   (opencode: `opencode.ts:635-645` on `permission.v2.replied`; Claude: the `ask_resolved` tool).
   `retire` (`hub.rs:5097-5123`) looks up `messages_for` and `retire_each` (`hub.rs:5126-5158`)
   takes the buttons off and forgets the record. Slice 0 of §3 changes one thing here, because
   all three judges found it: **`retire_each` never calls `mark_answered`**. Today, between a
   keystroke in the pane and the `ask_resolved` arriving, and forever after if the Telegram edit
   fails (`hub.rs:5145-5157` logs and leaves the record with `answered: None`), the phone remains a
   second answerer. After slice 0, `retire` on `AskEnd::Answered` marks the authorisation **before**
   the edit, so a racing tap hits `AlreadyAnswered` instead of delivering. The keyboard path then
   inherits the same guarantee for free.

### 2.4 The hard lines

| line | check |
| --- | --- |
| **No path from Telegram to a keyboard.** | Nothing here reads a pane or sends to one. `answer` targets the hub's own directory; `deliver` goes down the hub-proto socket to an adapter that already has a `choice` path (`bot.rs:699-708` today). `there_is_no_way_from_telegram_to_a_keyboard.rs` and `no_live_write_call_site.rs` stay, and the second is widened. |
| **The hub calls no herdr write method.** | The hub binary still never opens herdr's socket. `status` calls `snapshot` only (`client.rs:287`). The one herdr write in the whole design, `pane.report_metadata`, is issued by kickoff from kickoff's process on kickoff's pane, and after the guard widening this repo cannot spell it. |
| **Inbound selects, never names.** | An answer drop carries four identifiers that must each match a record the hub wrote; a conversation it does not know, an option it did not write down, or an address it never held is refused with a result naming which. No text field exists to name anything. A pane stamp names a conversation the registry must already know, and is data for a read-only table. |
| **The hub allocates nothing.** | No topic, no unit, no id: the mirror is a projection of state the hub keeps, under `<state>` (REFUSES 2, `docs/CAPABILITIES.md:196-197`). No `Command` enters `hub.rs` or `bot.rs` (grep is empty; `bot.rs:47,137,284` are teloxide's `BotCommands`). Standing at the keyboard is the uid gate the socket already trusts first; no allowlist is read or written. |
| **REFUSES 6 — the hub never learns what a lane, a room or a proof is** (`CAPABILITIES.md:205-207`). | The mirror carries `address`, the hub's own word (`Addr`, `hub.rs:1458`). The join to a git worktree happens in the CLI, not in `serve`. |
| **Writes only under `<state>`.** | The mirror and `answers/` are both there. |
| **Answered once.** | §2.3, steps 4, 5 and 7. |

### 2.5 Ownership

| owner | changes |
| --- | --- |
| **hub (this repo)** | slice 0: `retire` marks before it edits. Slice 1: `mirror.rs` on the `presence.rs` pattern; last beat kept per claim; `status` join and render; guard widened. Slice 2: `answers/` sweep in the watch loop; `resolve_tap` → one core with `Answerer`; the `answer` verb. Slice 3: `watch` narrates. |
| **adapter** | nothing required. Optional, for a state column on the laptop: the opencode watcher maps `permission.v2.asked` / `question.v2.asked` → `beat{blocked}`, the prompt it posts → `beat{working}`, beside the `idle` it sends today (`opencode.ts:668-673`). The Claude tool server sends no beat and need not. |
| **kickoff** | slice 4 only: when it starts a room inside a herdr pane, `herdr pane report-metadata` with `tokens.kickoff_hub` from the two variables it already holds; `ttl_ms` set; cleared on stop. |
| **herdr** | nothing. §5 lists what its author should be asked before slice 4 hardens. |
| **operator** | runs `status` and `answer`; configures nothing. |

### 2.6 The wall

**No new mount.** The mirror is read by `status` on the host; `answers/` is written by `answer` on
the host; the hub reads both on the host. A wall keeps exactly the mounts `docs/ATTACHING.md`
already lists: the `/run/user/<uid>/kickoff/` **directory** and the token file (`ATTACHING.md:1110-1115`),
the repo, and the conversation's own `media/` read-only and `outbox/` read-write
(`ATTACHING.md:2020-2023`, *"per conversation, not per hub"*). Nothing of this design must be
mounted into a wall — and `<state>` whole must never be: a wall that could write `answers/` could
answer another conversation's question. A test pins the rule the way `docs/ATTACHING.md:1113`'s
table already pins the socket: `nothing_a_wall_mounts_can_reach_the_mirror_or_the_answers` —
`<state>/hub.mirror.json` and `<state>/answers/` share no ancestor with any path §14 tells a wall to
mount below `<state>/media/<id>/<address>/` or `<state>/outbox/<id>/<address>/`.

**The process view of a walled room** is honestly thin, and this design does not pretend
otherwise. herdr's schema shows no way to adopt a foreign pid, so a walled room appears in herdr
only if its entry process runs inside a pane herdr spawned; inside the wall, herdr's own claude hook
and opencode plugin cannot reach `~/.config/herdr/herdr.sock` unless it is bind-mounted, and mounting
that socket hands the wall every herdr write method — so it is not mounted, and herdr's status for
that pane is screen-guessed. That is exactly the case the mirror is built for: the pane's row shows
the hub's `connected`, its open asks and its beat, joined by the stamp kickoff set from the host
side. A room under systemd with no pane at all has only the conversation view, and `status` says
*"hub only"*.

**Multi-box** (`herdr --remote`, `live_handoff`): per box. A laptop on box B sees box B's hub. Not
designed here.

---

## 3. The slices, in order, each with the test that proves it

RED before GREEN on every one; a regression test that never failed proves nothing.

**Slice 0 — the terminal race, hub only.** `retire` on `AskEnd::Answered` (`hub.rs:5097-5123`)
calls `mark_answered` before `retire_each` edits, so a phone tap racing an `ask_resolved` is refused
rather than delivered, and a failed edit leaves a keyboard that answers *"already answered"* rather
than one that answers. Touches neither herdr nor kickoff, nor any adapter. Test:
`a_question_answered_at_the_terminal_stays_answered_when_its_keyboard_will_not_come_off` — a fake
`Surface` whose `retire_buttons` fails, an `ask_resolved{answered}`, then a tap: today it delivers a
`Choice`; after, `AlreadyAnswered`. Half a day.

**Slice 1 — the mirror and the joined `status`.** `mirror.rs`; the last beat per claim; `status`
reads the mirror through `vouched_for`, joins by path with the two refusal rules, renders the second
block; `--hub-json`; guard widened. Touches neither herdr nor kickoff. Tests:
`the_mirror_names_no_chat_and_no_person` (serialise a hub with a claim and an open ask; assert the
bytes contain no chat id, topic id, message id or user id); `a_mirror_a_dead_hub_wrote_is_read_as_unknown`
(the `presence.rs` test shape); `a_mirror_that_cannot_be_rewritten_is_unlinked_not_left_stale`;
`a_pane_in_an_enrolled_repo_joins_its_seed` and `a_seed_with_rooms_is_refused_not_guessed`;
`a_pane_in_a_lane_worktree_joins_its_lane_not_its_project` (needs a real linked worktree in a temp
repo; the `WorkspaceInfo.worktree` field is absent on every live workspace here, so this is the
only place the lane join is exercised); and the widened guard itself reds when any of the five new
names is spelled in a scratch file. Two days.

**Slice 2 — `answer`.** The `answers/` sweep; `resolve_tap` becomes one core with two standings;
the verb; the result file. Tests: `a_question_answered_at_the_keyboard_cannot_be_answered_on_the_phone`
and `a_question_answered_on_the_phone_cannot_be_answered_at_the_keyboard` (both orders, and both
in the same tick under one ledger lock);
`an_answer_naming_an_option_that_was_never_written_down_is_refused`;
`an_answer_for_a_session_that_has_restarted_is_refused`; `an_answer_left_behind_for_a_minute_is_refused_unread`;
`an_answer_nobody_received_is_reported_not_delivered_and_the_question_is_closed_on_both_sides`.
Two to three days.

**Slice 3 — `watch` narrates the conversation view.** The graft from the operator-door draft, as a
file rather than a socket: the hub appends one line per event to `<state>/hub.events.jsonl` —
`{at, conversation, address, kind: said|asked|answered|retired|typed|marked|connected|released,
ask_id?, option_id?, by: phone|keyboard|terminal?, reached: <the audit word>}`, written at the same
seams the audit already writes (`hub.rs:1074-1100` for `sent`/`outcome`, the `Ack` arm at
`hub.rs:4383-4393` for the mark, `note_who_is_connected` at `hub.rs:2124`) — and `herdr-tg watch`
prints those lines beside `pane.agent_status_changed`. No message text in the file; the ask text is
in the mirror already, and a transcript on disk is `docs/PROPOSAL-CONVERSATION-RECORD.md`, a
separate decision. Test: `watch_prints_a_question_and_its_answer_beside_a_panes_status_change`.
One day. Optional; the operator may not want a tail.

**Slice 4 — the stamp, kickoff's.** Kickoff stamps `tokens.kickoff_hub` on the pane it starts;
`status` prefers the stamp to the path and prints *"not a conversation here"* for a stamp the
registry does not know. Test in this repo: `a_stamped_pane_joins_by_the_stamp_and_never_by_its_path`
(a fixture snapshot with a token naming a `c-` room whose `repo` is a seed's). Test in kickoff's
repo: the launcher's stamp round-trips through `herdr pane report-metadata` and clears on stop.
Blocked on §5 questions 3 and 4.

---

## 4. The two options the operator must choose between

The design above is the same under both; the choice is **the channel the laptop's answer travels
on**, which is where the three judges split (two chose the file, one the socket).

**Option 1 — the drop (recommended).** `answers/` under `<state>`, swept by the loop that already
sweeps the registry. One socket stays one socket; no new admission surface; nothing outside
`<state>` is written; no new mount rule to teach a wall. Its cost: a poll of up to a second and a
three-second wait that can end in *"not answered yet"* while the answer was in fact applied; and the
hub learns only a uid from the 0700 directory, not the pid `SO_PEERCRED` gives a socket
(`hub.rs:5394-5400`), so the audit line for a keyboard answer is thinner than for a tap.

**Option 2 — the door.** A second Unix socket at a **sibling** of the agents' directory — never
inside `/run/user/<uid>/kickoff/`, because a wall mounts that whole directory
(`ATTACHING.md:1113`) — on which the hub streams the mirror's events live and accepts a `choice`
resolved through the same core with `Answerer::Keyboard`, peer uid and pid read off the connection.
Its cost: a second admission surface with no secret behind it, a second local vocabulary beside
hub-proto, one more runtime directory outside `<state>`, and one more rule a wall must never break.

**Recommendation: Option 1**, for one reason: it adds no surface that trusts anyone. Both options
end in the same `mark_answered` under the same lock, so the difference is entirely what is exposed
to get there — and the `Answerer` core is written so that, if the poll proves annoying in use, the
door is a transport swap and not a redesign.

The losers, in a sentence: the operator-door design as a whole lost on surface (a second socket,
seven new payload shapes, a signature change and a `user_id: 0` sentinel on a wire that says "both
numbers", `frame.rs:454-469`); the fleet-in-herdr-panes design lost because a walled or systemd room
has no pane for it to join, its `pane.run` is not a p20 method, and its laptop answer runs through
`retire_each`, which forgets and never marks (`hub.rs:5126-5158`).

---

## 5. What only herdr's author can answer

Nothing in slices 0–3 depends on an answer. Slice 4 depends on 3 and 4.

1. Is there any way to register or adopt a process or pty herdr did not spawn — a room in bwrap?
   The p20 schema shows none.
2. Is the peer check on `~/.config/herdr/herdr.sock` uid-only? Could a walled process with the
   socket bind-mounted call `pane.report_*`, or `pane.send_keys` into any pane? (This proposal
   assumes the worst and mounts nothing.)
3. `pane.report_metadata` from a foreign `source` (`kickoff`): are `tokens` rendered in the TUI, and
   does a foreign source disturb the agent-state authority of the installed integrations
   (`applies_to_source`, `pane.clear_agent_authority` semantics)? Does a `null` token value clear it?
4. `WorkspaceWorktreeInfo.checkout_path` versus git's worktree name: does herdr expose the
   `.git/worktrees/<name>` name anywhere, and for a linked worktree is `repo_root` the main tree?
5. For `opencode attach <url>` in a herdr pane, does the opencode integration plugin report status
   and session into *that* pane, or only when the server itself was started in the pane?
6. Is `agent_session.value` for claude the Claude Code session uuid, and is it stable across
   `--resume`?
7. `pane.process_info` (`{shell_pid, tty, foreground_processes[{pid, name, cwd, argv}]}`, not
   modelled by the client): is it cheap enough to poll, and is `cwd` the process's real `/proc` cwd?
   A pid-ancestry join from `Claim.pid` (`hub.rs:1399`) would be possible on it; this proposal does
   not build one.
8. Is there a structured, read-only way to see *what* a `blocked` pane is blocked on, or only screen
   text? (herdr carries no structured ask; this is why the scraper died and why the conversation
   view cannot be sourced from herdr.)
9. Is there a global agent-status event without the stale `pane_updated` replay on connect
   (`crates/herdr-client/src/proto/event.rs` header), so `watch` need not subscribe per pane?
10. Does a `--remote` herdr session change any of the above for a fleet across boxes?
11. Would the plugin system (`plugin.pane.open`, manifest hooks) let a plugin render an external
    feed — the mirror — in a pane, so the conversation view could live inside herdr's own UI?

I did not check: the herdr source (not on this box); what `herdr pane run` calls underneath; whether
herdr's screen detection classifies an `opencode serve` pane; `hub-link.ts`'s decoder beyond the
absence of a strict mode; whether git's worktree name equals the checkout basename.

---

## 6. What this proposal refuses

1. **Typed words from the laptop toward an agent, through the hub.** `answer` carries four
   identifiers and no text. The laptop already has the agent's terminal.
2. **A herdr write from anything in this repo.** Not `report_metadata`, not `agent.prompt`, not the
   three the guard already forbids. The guard is widened, not reasoned around; the stamp is
   kickoff's.
3. **The hub opening herdr's socket at all**, even to read. The process view is joined in the CLI.
4. **A second Surface.** `Surface` (`hub.rs:325-437`) stays Telegram-shaped and singular
   (`Hub<S>`, `hub.rs:1592`); the laptop is a reader of files and a writer of one selection, not a
   surface of record.
5. **A join by guess.** A seed with rooms is *"several conversations here"*; a stamp the registry
   does not know is *"not a conversation here"*; a pane outside any enrolled repo is `—`. Never a
   best match.
6. **Any mount of `<state>` into a wall**, and any join key carried in the environment.
7. **The hub learning what a pane, a worktree or a room is.** It writes `Addr`; the CLI does the
   rest.
8. **A transcript on disk.** The mirror holds open questions and the events file holds identifiers
   and outcomes; what was *said* stays where it is until `docs/PROPOSAL-CONVERSATION-RECORD.md` is
   decided on its own.
9. **Starting anything.** `docs/HUB-AND-KICKOFF.md:133-138` Q2 stays open and untouched.
