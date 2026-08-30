# Slice 3 — adversarial review, 2026-08-30

> Verdicts: **BROKEN / FIX-FIRST / FIX-FIRST** · 7 blockers · 16 majors.
> Three reviewers, each given the live service, the real herd, and a mandate to break it.
> Every finding below carries a reproduction the reviewer ran. **The service was stopped when
> this landed** — several of these are live-fire.

This is the first adversarial pass slice 3 has had. Slice 1 had three rounds and they found six
blockers between them, including a shell script that passed all seven proof gates and a parser bug
that let a real write ship green. Slice 3 shipped — and was pushed to a public repo — without any
of that. That ordering was the mistake.

## Lens 1: the write path — BROKEN

Slice 3 can type the wrong thing into the right terminal, and the guard that is supposed to stop new write paths is blind to any directory named `target`. Four independent D3-class defects are confirmed by reproduction, three of them live right now: a Yes/No dialog answered \"no\" presses Enter and confirms Yes while the bot says \"✅ Sent.\"; a button labelled \"Reject\" confirmed \"Allow always\"; and a live unaudited `send_text` passes fmt, clippy and all 232 tests.

### BLOCKER (4)

#### `$REPO/crates/herdr-tg/src/permission.rs`

A TWO-OPTION dialog is never recognised, so the operator's "no" is typed as text and the Enter that follows confirms the highlighted option. permission.rs:147 requires EXACTLY ONE background value to occur exactly once among the options. With two options — one selected, one not — both backgrounds occur exactly once, `unique.len() == 2`, and `parse` returns None. bot.rs:638 then falls through to the text path (`deliver::deliver`), which sends `send_input(text)` followed by `send_keys(["Enter"])`. The module's own header says this fallthrough is "the dangerous path"; the `parse` doc at line 110-114 contradicts it and calls a false negative merely "a worse reply experience". Yes/No is the most common confirm shape there is.

**Reproduction**

```
Test a7 in $SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/attack.rs against a fake pane that swallows text and confirms on Enter:
$ cargo test --bin herdr-tg attack -- --nocapture
dialog recognised as a choice? false
keys that reached the pane: ["Enter"]
the dialog confirmed: Some("Yes")
rung = Submitted, needs_attention = false
what the operator is told: ✅ <b>Sent.</b>
assertion `left != right` failed: the operator replied "no" and the dialog confirmed Yes

And the two-option parse itself:
two-option, selected=0 -> None
```

**Fix** — Do not derive the selection from "exactly one unique background". Derive it from "the background that differs from the modal (most common) background", which is well-defined for n>=2. Add a fixture-backed test for a 2-option dialog. Separately: make the fallthrough fail CLOSED — if the ANSI read contains an option row (`is_option_row` matched) but the prompt could not be parsed, refuse the text path and tell the operator to answer at the keyboard, rather than sending text + Enter into a focused modal.

#### `$REPO/crates/herdr-client/tests/no_live_write_call_site.rs`

The D3 write guard is blind to every path containing a directory named `target`. `SKIP_DIRS = ["target", ".git"]` (line 53) is matched against ANY directory name at ANY depth, not just the workspace build directory. `crates/herdr-tg/src/target/` is therefore never walked by rule 1 (name anywhere in a non-client member) or rule 2 (call anywhere outside cfg(test)). The per-member "contributed at least one file" checks are satisfied by the crate's other files, so the scan is not detected as vacuous. This is the same shape as the previous round's finding — a real write ships with every gate green — and it needs no lexer trick, survives `cargo fmt`, and is clippy-clean. A module named `target` is not exotic in a codebase that already has a `Target` enum in routing.rs.

**Reproduction**

```
Copied the repo to $SCRATCH/atk/clean, added ONE file, crates/herdr-tg/src/target/mod.rs, containing `let _ = client.send_text(pane, text).await?;` outside #[cfg(test)], plus `mod target;` in main.rs. Then:
$ cargo fmt --check                       -> cargo fmt --check          : GREEN
$ cargo clippy --workspace --all-targets -- -D warnings -> clippy -D warnings errors  : 0
$ cargo test --workspace                  -> cargo test --workspace     : 232 passed, 0 failed
(232 is the pristine baseline, verified in the untouched repo by the same command.)
```

**Fix** — Anchor the skip to the workspace build directory only: skip a directory named `target` when its parent contains a Cargo.toml (or simply skip exactly `<root>/target` and `<member>/target`), never on bare name at arbitrary depth. Add a self-test that plants a file under `crates/*/src/target/` and asserts the walk sees it — the guard already has `the_guard_itself_detects_a_planted_call_site` for the lexer; it needs the same for the walk.

#### `$REPO/crates/herdr-tg/src/bot.rs`

A permission button is resolved by POSITION against a freshly-parsed dialog, and the label the button displayed is never checked. bot.rs:379-389 turns the callback payload `c|<pane>|<idx>` into the 1-based string `one_based` and hands that to `deliver::choose`, which at deliver.rs:281 calls `prompt.match_option("3")` on a dialog it re-read a moment ago. Nothing carries or verifies the option TEXT. Telegram buttons stay tappable forever, so a tap on an older ask — or on the current ask after the pane moved to a different dialog with a different option order — confirms whatever now sits at that index. The bridge only names the option it actually chose in the confirmation, i.e. after the keys are irreversible.

**Reproduction**

```
Test a8 in $SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/attack.rs — push carried [Allow once, Allow always, Reject] so the "Reject" button is index 2 -> choose("3"); the pane now shows [Reject, Allow once, Allow always]:
$ cargo test --bin herdr-tg attack -- --nocapture
keys: ["Right", "Right", "Enter"]
confirmed: Some("Allow always")
told: Ok("chose \"Allow always\" — the dialog closed")
assertion `left != right` failed: a button labelled Reject confirmed Allow always
```

**Fix** — Put the label in the callback payload (or in a side table keyed by message id) and have `choose` take the intended LABEL, not an index: re-parse, find that exact label in the current options, and refuse with "that prompt has changed" if it is absent or ambiguous. `match_option` already does exact-then-unique-prefix matching, so this is a signature change, not new logic.

#### `$REPO/crates/herdr-tg/src/deliver.rs`

The two-writer race grants, and the read-back reports it as a clean success. `Prompt::keys_to` (permission.rs:50) emits a RELATIVE move computed from the selection observed in the read. If the operator moves the selection at the keyboard between that read and the keys landing, the move lands somewhere else — and because neither the code nor the harness wraps, moving toward index 0 silently under-shoots. permission.rs:43-44 asserts "nothing else drives this pane", which is exactly false for the product's core scenario (operator at the laptop, phone in hand). `choose`'s verification (deliver.rs:302) only asks whether a dialog is still parseable, which is true of ANY confirmation, so the bridge says "✅ Reject." after confirming Allow once.

**Reproduction**

```
Test r1 in $SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/race.rs — bridge's ANSI read shows selection=2, operator has since arrowed back to 0, operator asks for "Reject":
$ cargo test --bin herdr-tg target::race -- --nocapture
keys sent      : ["Enter"]
harness confirmed: Some("Allow once")
bridge detail  : chose "Reject" — the dialog closed
operator sees  : ✅ <b>Reject.</b>

The cheap guard, proven in test r2 over all 9 (true, observed) combinations:
absolute homing (Left x n-1, then Right x target) lands on the target for every combination of observed and true selection
```

**Fix** — Send an ABSOLUTE sequence instead of a relative one: `Left` x (len-1) to clamp to index 0, then `Right` x target, then Enter — one `send_keys` call, no extra RPC, and it relies on exactly the same no-wrap/clamp assumption `keys_to`'s own doc already depends on. Then re-read before the Enter and verify the highlighted label equals the intended one (two more RPCs on a local socket) if you want the race closed rather than merely made harmless.

### MAJOR (6)

#### `$REPO/crates/herdr-tg/src/permission.rs`

One line of ordinary agent prose disables dialog recognition for the whole pane. `parse` at line 116 takes the FIRST line matching `is_option_row`, and `is_option_row` matches any line containing both "select" and "confirm" (case-insensitive) — or "↑/↓". The real dialog's option row is near the bottom of the pane, so any earlier transcript line containing those two words wins, yields fewer than two options, and returns None. The pane then takes the text path (blocker #1's route). The agent writes that prose, so this is influenceable by whatever is running in the pane.

**Reproduction**

```
Test a3 in $SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/attack.rs, prepending one line to the real captured fixture:
$ cargo test --bin herdr-tg attack -- --nocapture
with one line of agent prose above it -> None
assertion failed: one line of ordinary agent prose above the dialog made the dialog invisible
(control: the unmodified fixture parses)
```

**Fix** — Scan candidate rows from the BOTTOM up and take the last match, and require the row to yield >= 2 non-hint SGR runs before accepting it as the option row — i.e. iterate `ansi.lines().rev()` and keep the first line that produces a valid prompt rather than the first line that merely mentions the words.

#### `$REPO/crates/herdr-tg/src/permission.rs`

`background_of` (line 205-215) reads a truecolor FOREGROUND as a background. It scans each ESC-separated SGR fragment for the substring "48;" — so `38;2;248;250;252m` (a stock light foreground; 248, 148 and 48 in the R or G slot all do it) matches at "248;" and returns the phantom background "48;250;252". The captured real dialog emits fg BEFORE bg, so the phantom is found first and wins over the genuine `48;2;...` background. When two options share such a foreground and a third does not, the "exactly one unique background" rule points at the wrong option, and `keys_to` then computes the move from a selection that is not the real one.

**Reproduction**

```
Tests a4/a5 in $SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/attack.rs, an opencode-shaped row where "Allow once" (truly selected, real yellow bg) and "Allow always" both use fg #f8fafc:
$ cargo test --bin herdr-tg attack -- --nocapture
truly selected = 0, parser says selected = 2
operator tapped Reject; keys ["Enter"]; the harness confirms "Allow once"
assertion `left == right` failed: tapping Reject confirmed Allow once
```

**Fix** — Parse the SGR fragment properly instead of substring-searching: split the parameters on ';' and only treat a parameter list as a background when the FIRST parameter is exactly 40-49 or 48/100-107 — never on a substring hit anywhere in the sequence. `background_of`'s existing unit test only covers well-formed inputs and passes either way.

#### `$REPO/crates/herdr-tg/src/bot.rs`

The dialog check fails OPEN on a read error. bot.rs:635-637: `let is_dialog = match client.read_visible_ansi(&pane).await { Ok(r) => permission::parse(&r.text), Err(_) => None }`. A transient socket error, a timeout, or a herdr hiccup turns a live permission dialog into "not a dialog", and the operator's reply goes down the text + Enter path. The whole point of the module is that this path is the catastrophe.

**Reproduction**

```
Read of bot.rs:635-637 plus the confirmed consequence of the text path against a modal (test a7 above: keys ["Enter"], dialog confirmed "Yes", operator told "✅ Sent."). No test in the suite covers the Err arm — `grep -rn "read_visible_ansi" crates/herdr-tg/src` shows the only production call sites are bot.rs:635 and notify.rs's recheck, which has the same `Err(_) => Vec::new()` shape.
```

**Fix** — On a read error, do not write at all: return `voice::nothing_sent(Reason::HerdUnreachable)`. A reply the operator can retry is strictly better than a keystroke into an unknown screen.

#### `$REPO/crates/herdr-tg/src/routing.rs`

A topic keeps routing into a pane whose agent has exited, and the reply is then EXECUTED by the shell. `Routing::resolve`'s liveness test (routing.rs:131) is `snapshot.panes.iter().any(|pane| pane.pane_id == *p)` — pane exists, nothing about an agent. `ensure_all_topics` only creates topics for panes with an agent, but nothing ever unbinds one. An agent quitting leaves a shell at a prompt in the same pane; the operator opens the still-existing topic, replies, and the bridge sends `send_input(text)` then `send_keys(["Enter"])` — send_input does not execute lines, but the separate Enter submits the line the shell now holds.

**Reproduction**

```
Test in $SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/topic.rs, a snapshot whose only pane has no `agent` and no `display_agent`:
$ cargo test --bin herdr-tg target::topic -- --nocapture
resolve -> Pane { pane: PaneId("wA:p1"), why: Topic }
assertion failed: the reply is aimed at a bare shell: send_input + Enter EXECUTES it

And the wire shape that then runs, from $SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/wire.rs:
WIRE  {"id":"2","method":"pane.send_input","params":{..."text":"please rebase onto main first\nthen ship it"}}
WIRE  {"id":"4","method":"pane.send_keys","params":{..."keys":["Enter"]}}
```

**Fix** — Make `resolve`'s liveness predicate require `p.agent.is_some() || p.display_agent.is_some()`, and return `Target::Gone` otherwise — PLAN.md's failure table already says a dead target must produce a picker, not a write. The snapshot is already in hand at the call site.

#### `$REPO/crates/herdr-tg/src/routing.rs`

Topic and sticky bindings are bare pane-id strings with no session anchor, and herdr's id space is session-scoped. `topics: BTreeMap<i32, String>` (line 64) and `sticky: BTreeMap<i64, String>` persist across bridge restarts by design, and are never pruned. herdr allocates workspace ids w1, w2, ... and pane ids p1, p2, ... from counters that restart when herdr restarts, so after a herdr restart or a reboot the same strings name different terminals — and `alive()` happily confirms them. The topic's title still shows the old workspace label. `PaneInfo` already decodes `terminal_id` (a random per-terminal id) and `cwd`, so a stable anchor is free.

**Reproduction**

```
Live state on this machine (read-only):
$ python3 -c "..." ~/.local/state/herdr-tg/routing.state.json
sticky : {'-<chat-id>': 'wE:p1', '<chat-id>': 'wA:p1'}
topics : {"9": "w9:p1", "17": "wA:p1", "20": "wB:p1", "23": "wC:p2", "25": "wE:p1"}

Live id shape showing session-scoped monotonic allocation (w9..wF, per-workspace p-counters with gaps from closed panes):
$ herdr api snapshot | python3 -c "..."
  w9 ['w9:p1', 'w9:p4', 'w9:p5']
  wA ['wA:p1']  wB ['wB:p1']  wC ['wC:p2']  wE ['wE:p1', 'wE:p4']

The anchor that is already available and unused:
w9:p1 | terminal_id= term_65a1dfd2070331 | cwd= $HOME/Projects/bliz-monorepo
```

**Fix** — Store `{pane_id, terminal_id}` (or cwd) in the topics and sticky maps and require BOTH to match at resolve time; a mismatch is `Target::Gone`, which already produces the picker. Cheapest interim: stamp the routing state with the herd's identity at write time and clear `topics`/`sticky` when it changes.

#### `$REPO/crates/herdr-client/tests/no_live_write_call_site.rs`

`code_only` has two more fail-open siblings of the fixed char-literal bug, both of which blank a real call from the scanner's view. (1) A raw string containing an odd number of `"` (r#"say "hi"#) leaves the scanner inside a string span, hiding everything up to the next quote. (2) A `/* ... */` block comment containing one `"` does the same — `code_only` only understands `//`. Separately, `workspace_members` locates the member list with `manifest.find("members")`, which matches inside `default-members`; a manifest declaring `default-members` first makes rule 1 scan the wrong, shorter list.

**Reproduction**

```
Three probes appended to a scratch copy of the guard ($SCRATCH/atk/herdr-tg):
$ cargo test -p herdr-client --test no_live_write_call_site atk_ -- --nocapture
input :     let doc = r#"say "hi"#; c.send_text(&pane, "rm -rf ~\n").await?;
code  :     let doc = r#      hi                        rm -rf ~\n
reach : None
input :     /* 6" of rope */ c.send_keys(&pane, &keys).await?; let z = "";
code  :     /* 6
guard would scan members = ["crates/herdr-client"]   (from a manifest listing default-members first)
```

**Fix** — Consume `r#*"` ... `"#*` and `/* ... */` as whole tokens before the plain-string arm, exactly as the char-literal arm was added, and extend `the_guard_itself_detects_a_planted_call_site` with both shapes. For the manifest: anchor on a line that starts with `members` after trimming, or reject a manifest containing `default-members` outright.

### MINOR (3)

#### `$REPO/crates/herdr-client/src/keys.rs`

`Key::parse` (line 101) accepts anything non-empty, non-whitespace and newline-free. SLICE-3-PROBE P2's stated decision for slice 3 — "the `Key` newtype accepts the `+` chord form only, and rejects `c-c` even though herdr would take it" — is not implemented, and config.rs:134-139 wraps the parse in an error message that tells the operator the rule IS enforced. A `submit_key = "C-c"` in herdr-tg.toml therefore passes startup validation, and `C-c`/`c-c` is precisely the form herdr special-cases and accepts — so every reply from the phone would interrupt the agent instead of submitting, while the read-back sees the pane change and reports "✅ Sent."

**Reproduction**

```
$SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/keycfg.rs:
$ cargo test --bin herdr-tg target::keycfg -- --nocapture
Key::parse("M-b") -> Ok("M-b")
Key::parse("Home") -> Ok("Home")
Key::parse("PageUp") -> Ok("PageUp")
Key::parse("\u{7}") -> Ok("\u{7}")
Key::parse("🙂") -> Ok("🙂")
Key::parse("a b") -> Ok("a b")
assertion failed: SLICE-3-PROBE P2: the Key newtype must reject the `-` chord form including c-c
```

**Fix** — Implement the probe's grammar in `Key::parse`: split on '+', require every leading segment to be one of ctrl/alt/shift/super and the final segment to be a single char or a name from the probe's accepted list; reject anything containing '-'. Failing that, at minimum reject `-` forms in `Config::load` so the error message stops being a lie.

#### `$REPO/crates/herdr-tg/src/bot.rs`

The button path writes without a guaranteed audit record, and logs the wrong thing. bot.rs:386-388 discards the result of `audit.sent(...)` with `let _ =`, whereas the text path at bot.rs:671-676 refuses to type at all when the audit log cannot be written ("this bridge does not type into a terminal without a record of it"). The button path also records `"[button] option 2"` — the index, not the label, and not the option that was actually confirmed. PLAN.md's trustworthiness rule is "every keystroke into a pane is audit-logged".

**Reproduction**

```
Read of bot.rs:386-388 versus bot.rs:671-676. The two call sites differ only in whether the Result is inspected:
  bot.rs:386  let _ = ctx.audit.sent(&at, chat_id, &p, &format!("[button] option {want}"));
  bot.rs:671  if let Err(e) = audit.sent(&at, chat_id, &pane, text) { ... return "Not sent: ..." }
```

**Fix** — Apply the text path's rule to the button path: on an audit write failure, answer the callback with a refusal and do not call `choose`. Log the option LABEL alongside the index once the label is carried in the payload (blocker #3's fix).

#### `$REPO/crates/herdr-tg/src/deliver.rs`

Three honesty gaps in the confirmation ladder. (1) `choose`'s success test at deliver.rs:302 is `permission::parse(&now).is_none()`, which is also true when the dialog is merely unparseable — so a 3-option dialog replaced by a 2-option one reports "the dialog closed". (2) `Rung::Acted` is never constructed anywhere in the crate, so the ladder's documented top rung and its "✅ Sent — it picked it up." wording are unreachable. (3) crates/herdr-client/src/proto/request.rs:151-157 still carries the slice-1 banner "NO LIVE CALL SITE ... no code path outside #[cfg(test)] constructs one, and the binary exposes no subcommand that reaches HerdrClient::send_text / send_keys / send_input", which slice 3 made false.

**Reproduction**

```
$ grep -rn "Rung::Acted" crates/herdr-tg/src/
crates/herdr-tg/src/voice.rs:215,224,354,408,439   (docs, match arm, tests)
crates/herdr-tg/src/deliver.rs:25,499,503          (docs, ordering tests)
crates/herdr-tg/src/notify.rs:68                   (doc)
-- no construction site outside tests; deliver.rs:65-69 carries #[allow(dead_code, reason = "constructed by the push loop")] and the push loop does not construct it.

And two-option unparseability, from test a2 above: two-option, selected=0 -> None
```

**Fix** — Verify the choice by re-reading and asserting the dialog's option row is gone (no `is_option_row` match), not that `parse` failed. Either wire `Acted` from the notifier's blocked->working edge as documented, or delete the rung and the wording that promises it. Rewrite the request.rs banner to describe the audited path that now exists.

### What held

- The sealed trait holds against a real out-of-workspace crate. Built $SCRATCH/atk/foreign against the client crate: `impl Request for Evil` with `const METHOD = concat!("pane.send","_text")` -> E0277 (`Evil: client::sealed::Sealed` not satisfied); `proto::request::PaneSendTextRequest` -> E0603 module private; `client::sealed::Sealed` -> E0603; `ReadSource::Recent` -> E0603 enum private. No second door found: every request type is `pub(crate)`, the only `impl From` in the crate is `From<&str>` for the id newtypes, there is no Deref/AsRef/Borrow impl anywhere, and no public fn signature takes a `ReadSource`.
- The wire shape is what deliver.rs claims. Drove `deliver::deliver` with a multi-line reply against a spy Unix socket ($SCRATCH/atk/herdr-tg/crates/herdr-tg/src/target/wire.rs): pane.read(source:visible) -> pane.send_input{text:"...\nthen ship it"} -> pane.read -> pane.send_keys{keys:["Enter"]} -> pane.read. `pane.send_text` never appears on the operator-text path, and every read is `source:"visible"`.
- Rule 1 of the write guard is genuinely strict inside the directories it does walk — it tripped on my own probe module the moment it merely mentioned `send_input` in a doc comment and in a trait-impl signature.
- `match_option` refuses ambiguity rather than resolving it: "allow" against [Allow once, Allow always, Reject] returns None; "0" and "4" are bounds-rejected; "" and "yes please" return None.
- `keys_to` never wraps and refuses an out-of-range target (`keys_to(3)` on 3 options -> None).
- The Telegram gate fails closed: an empty allowlist admits nobody including 0, i64::MAX and i64::MIN, and both `on_message` and `on_callback` check it before anything else touches the herd.
- HTML injection from pane content is escaped, and oversized messages truncate on a line boundary with balanced <b>/<code> tags (a split tag would 400 and read as the bot going silent).
- The local model does not substitute for the agent's words in the cases I tried. `voice::asked("Run: rm -rf /srv/db-backups ? [y/N]", gist="tidy up some temporary files")` kept the raw tail in a <pre> block, and a two-line "Drop table users?\ny/n" kept both lines — `covered_by`'s len>3 word filter stops the paraphrase from swallowing short, punctuation-heavy asks. The gist is never read by routing, choosing or delivery.
- The audit record for a text reply is written BEFORE the write and the write is refused when it cannot be written (bot.rs:671-676) — the button path is the one that skips this.
- Reads are structurally pinned to `visible`: `read_visible`, `read_visible_ansi` and `read_visible_tail` all construct `ReadSource::Visible`, and the guard's rule 4 keeps the enum crate-private (it does not yet pin `read_visible_ansi(` by name, which is a small gap, not a hole).

## Lens 2: routing correctness — FIX-FIRST

Two live paths put the operator's words into a pane they were not looking at, and both are armed on the running service right now: `topics`/`pushes` are global maps rather than per-chat, so a swipe-to-reply in the DM (chat <chat-id>) routes into a forum pane; and a two-option Allow/Deny dialog is structurally invisible to `permission::parse`, so the reply is typed as text and the submit key confirms whatever is highlighted — replying "no" grants it. Six more confirmed defects follow, including one transient `agent.list` error permanently swallowing an ask, and a mirror that emitted zero relays over 116 s of a real working agent pane.

### BLOCKER (2)

#### `$REPO/crates/herdr-tg/src/routing.rs:61-69,124-173`

`topics: BTreeMap<i32,String>` and `pushes: BTreeMap<i64,String>` are keyed on the bare Telegram id with no chat scoping, but message ids and topic ids are PER-CHAT counters. `resolve()` never consults `chat` for rules 0 and 1. Both allowlisted chats (<chat-id> and -<chat-id>) therefore share one id space, and every entry in `pushes` was written by the forum group (in forum mode `say_in_topic` returns before the flat branch). A swipe-to-reply in the private DM — the standard mobile gesture — hits rule 1, the rule the module documents as 'the only rule that needs no memory and cannot go stale', and routes into another conversation's pane. This is D3's catastrophic failure reachable with one thumb.

**Reproduction**

```
Ran the real `Routing::resolve` against the operator's OWN live state file (~/.local/state/herdr-tg/routing.state.json, copied unmodified):
```
DM reply-to message # 20 -> Pane { pane: PaneId("wB:p1"), why: ReplyTo }
DM reply-to message # 23 -> Pane { pane: PaneId("wC:p2"), why: ReplyTo }
DM reply-to message # 26 -> Pane { pane: PaneId("wE:p1"), why: ReplyTo }
DM reply-to message #242 -> Pane { pane: PaneId("wC:p2"), why: ReplyTo }
thread_id  20 claimed from the DM -> Pane { pane: PaneId("wB:p1"), why: Topic }
General-topic message -> Pane { pane: PaneId("wE:p1"), why: Sticky }
```
Allowlist confirmed from the running unit: `tr '\0' '\n' < /proc/3279411/environ` -> `HERDR_TG_ALLOWED_CHAT_IDS=<chat-id>,-<chat-id>`, `HERDR_TG_FORUM_CHAT_ID=-<chat-id>`. The live `pushes` map holds ids 4..242, a range any DM passes through. Test file: $SCRATCH/attack/tests/a5_crosschat.rs (3/3 pass).
```

**Fix** — Key both maps on `(chat_id, id)` and pass `chat` into rules 0 and 1. Additionally gate rule 0 on `chat == forum_chat_id` — a `message_thread_id` from a non-forum supergroup is a reply-thread root, not a pane topic. Separately decide what a General-topic message (thread_id None inside the forum) should do; falling through to sticky is the same class of surprise, and the live sticky for that chat is wE:p1.

#### `$REPO/crates/herdr-tg/src/permission.rs:138-150`

The selection is found by 'exactly one background occurs exactly once'. With TWO options — Allow/Deny, Yes/No, the commonest permission shape there is — the selected and the unselected background each occur once, `unique.len() == 2`, and `parse` returns None. The pane is then treated as prose: `notify::recheck` reports `options: []`, `push_ask` renders the '📌 Send my replies here' button (inviting a text reply), and `route_and_deliver`'s `if let Some(prompt) = is_dialog` falls through to `deliver::deliver`, which calls `send_input_text` then `send_submit_key` unconditionally (deliver.rs:174,195). On a focused dialog the text goes nowhere and the Enter confirms the highlighted option. Replying 'no' to an Allow/Deny prompt grants it — verbatim the failure permission.rs's own header says it exists to prevent. The conservative fallback is the dangerous one.

**Reproduction**

```
Synthetic option rows rendered in the exact SGR shape of the captured tests/fixtures/opencode-permission.ansi row, verified faithful first (the 3-option form parses to ["Allow once","Allow always","Reject"], selected 0 — identical to the real capture):
```
test the_synthetic_row_is_faithful_to_the_captured_dialog ... ok
test a_two_option_permission_dialog_is_never_recognised_as_a_dialog ... ok
```
That test asserts `parse(...).is_none()` for ["Allow","Deny"], ["Yes","No"] and ["Allow once","Reject"], and asserts the 3-option form still parses — so it is the arity, not my render. Test file: $SCRATCH/attack/tests/a4_callback.rs.
```

**Fix** — Two changes. (1) Resolve the selection against the MODAL background rather than a uniqueness count, so arity two works. (2) Make the fallback fail closed: `is_option_row()` already detects an option row cheaply — if that matches but `parse` cannot resolve a selection, refuse to send text+submit and tell the operator to answer in the pane. Never press the submit key on a screen that looks like a control.

### MAJOR (6)

#### `$REPO/crates/herdr-tg/src/notify.rs:372,562,640-648`

`recheck` collapses 'the ask resolved' and 'I could not tell' into the same None: `let agents = client.agents().await.ok()?;`. The spawned debounce task sends that None and exits. There is no retry — the outer loop only re-runs the snapshot replay after a resubscribe, and the `resubscribe_check` timeout only breaks when the agent-pane SET changes. So one transient error on the `agent.list` at the instant the debounce expires drops a real, standing ask forever. The None arm at :562 also leaks the pane from `pending` (a later status edge clears it, so that part is one-shot). `finished()` at :430 has the identical shape.

**Reproduction**

```
Real `notify::watch` driven against a scripted herdr mock ($SCRATCH/mockherdr.py) that fails `agent.list` for one 3.5 s window:
```
[mock   8.0s] pane is now blocked (seq 7)
[mock  14.1s] agent.list -> ERROR (staged transient failure)
  | [  0.00s] BEAT Asked      pane=m1:p1 seq=5
  | [  1.60s] BEAT Resumed    pane=m1:p1
  | [ end  ] watcher stopped     <-- 46 more seconds, pane still blocked, nothing pushed
pushed: {"pushed":{}}
```
Baseline run with no staged failure pushes seq 7 at t+12.6 s, so the only difference is the one failed call. Client DEFAULT_REQUEST_TIMEOUT is 10 s (client.rs:59) and every RPC dials a fresh connection, so this is a single slow/failed call, not a herdr outage (a real outage also fails session.snapshot, which does hit the retry loop).
```

**Fix** — Make `recheck` return a tri-state (StillBlocked | Resolved | Unknown). On Unknown, re-arm the timer with bounded backoff rather than reporting a resolution. Send the pane id alongside the None so `pending.remove` runs on every path.

#### `$REPO/crates/herdr-tg/src/mirror.rs:55-61`

`observe` relays only when two consecutive 4 s reads (bot.rs:828 MIRROR_TICK) are BYTE-IDENTICAL after `strip_chrome`. A real opencode pane paints a braille spinner inside the running tool-call line, which `strip_chrome` keeps — rule 3 filters the literal "esc interrupt" and lines starting `ctrl+`, not a spinner inside a content line. The screen therefore never settles while the agent is working, and the mirror — the feature the operator explicitly asked for so that walking away is a non-event — is silent for exactly the period they are away.

**Reproduction**

```
Sampled the operator's live wB:p1 and w9:p1 at the real MIRROR_TICK (4 s), source: "visible" only, 30 frames each, then fed them through the real `strip_chrome_public` + `Mirror`:
```
wB:p1: 30 frames over 116s | consecutive-identical pairs: 0 | relays: 0
w9:p1: 30 frames over 116s | consecutive-identical pairs: 29 | relays: 0
```
The only diff between consecutive cleaned wB:p1 frames:
```
  - "   ⠴ sleep 590; command -v strace >/dev/null && echo \"TAPS DONE\" || …"
  + "   ⠙ sleep 590; command -v strace >/dev/null && echo \"TAPS DONE\" || …"
```
Test files: $SCRATCH/attack/tests/a2b_live_mirror.rs and a2c_longmirror.rs.
```

**Fix** — Do not settle on raw equality. Normalise volatile glyphs (spinner frames, elapsed-time and token counters) before comparing, or settle on the prose subset only — `worth_relaying`'s `looks_like_prose` filter already exists and the spinner line is not prose. A stability window over the prose projection would relay mid-session without the firehose.

#### `$REPO/crates/herdr-tg/src/mirror.rs:116`

`new_since` anchors on the previous screen's last substantial line and locates it with `rposition` — the LAST occurrence. When that line reappears BELOW new content (a retried command, a repeated log line, a redrawn footer), the anchor jumps past the new text and everything between the two occurrences is dropped. `observe` then advances `self.seen` to the new screen anyway (mirror.rs:69, before the `fresh?` at :71), so the skipped text is baselined out. Losing an agent's words is the failure this module's own doc calls the one that matters.

**Reproduction**

```
```
test a_reprinted_line_swallows_everything_the_agent_said_between ... ok
test the_mirror_loses_a_whole_paragraph_to_a_repeated_line ... ok
```
The first asserts `new_since(prev, now) == None` when `now` is `<anchor>\n<a whole sentence>\n<anchor>`. The second drives it through the public API at real cadence, with a repeated line taken verbatim from the live wB:p1 capture, and asserts two full sentences never come out.
Not theoretical on this herd — over the 60 live frames captured above:
```
screens examined: 60 | screens whose anchor line is NOT unique: 30
wB_p1[00] anchor occurs 2x: "pending ($(date +%H:%M))\""
```
Test files: $SCRATCH/attack/tests/a2_mirror.rs and a2d_anchor_live.rs.
```

**Fix** — Anchor on a multi-line suffix (join the last 2-3 substantial lines) so a single repeated line cannot match, and prefer the FIRST occurrence at or after the previously-known offset rather than `rposition`. Do not advance `seen` when the diff produced nothing.

#### `$REPO/crates/herdr-tg/src/bot.rs:359,422`

The identity gate is per-CHAT, not per-user: `on_message` checks `msg.chat.id.0` and `on_callback` checks `q.message.chat().id.0`. Neither looks at `msg.from` / `q.from`. The allowlist on the running service contains a supergroup, so every current and future member of that group — and anyone the operator ever invites — has a keyboard attached to the operator's terminals and can tap permission buttons. PLAN.md calls this gate 'the equivalent of Collie's COLLIE_TRUSTED_USER', which is a USER.

**Reproduction**

```
`tr '\0' '\n' < /proc/3279411/environ` on the live `herdr-tg serve` process -> `HERDR_TG_ALLOWED_CHAT_IDS=<chat-id>,-<chat-id>`. The negative id is a supergroup, and it is the same id the audit log records as the source of real keystrokes:
```
<chat-id>.957Z	sent	chat=-<chat-id>	pane=wE:p1	bytes=26	text=Yes it worked really well!
<chat-id>.109Z	outcome	pane=wE:p1	rung=Submitted	detail=the pane changed after Enter …
```
Writes only happen after `Gate::admit`, so the group id is definitively admitted. `grep -n 'msg.from\|q.from' crates/herdr-tg/src/bot.rs` returns nothing.
```

**Fix** — Add `allowed_user_ids` and require BOTH: chat on the chat allowlist AND `msg.from`/`q.from` on the user allowlist. Keep it fail-closed the same way (empty user list = nobody) and log the rejected user id so the operator can read their own out of journalctl.

#### `$REPO/crates/herdr-tg/src/routing.rs:95-109`

A topic->pane binding is created once and never unbound, revalidated, renamed, or expired. `topic_for` matches on the pane-id STRING alone; `ensure_all_topics` (bot.rs:802) skips any pane that already has a topic. So if a pane id is ever reused by a different session, the dead agent's topic — still titled with the old workspace's name — silently becomes a live keyboard into someone else's agent, and no fresh topic is created. `pane.moved` is not handled at all despite PLAN.md's failure row promising silent sticky migration; the only mention in the crate is a render string at render.rs:157.

**Reproduction**

```
```
test a_recycled_pane_id_silently_inherits_a_dead_agents_topic ... ok
```
The test binds topic 42 -> wC:p1, shows `resolve` correctly returns `Gone` while the id is unused, then shows it returning `Pane { pane: wC:p1, why: Topic }` the moment a different session holds that id, and that the binding round-trips through disk as `{"topics":{"42":"wC:p1"}}`.
Reachability, probed on a throwaway `herdr --session probe` (never the live herd): pane numbers do NOT recycle within a workspace (closed w1:p2/p3, next splits were p4 and p5) and `next_public_pane_number` is persisted per workspace. But session.json carries NO global workspace-id counter (top-level keys are only version, workspaces, active, selected, sidebar_width, sidebar_section_split, collapsed_space_keys), closing a workspace deletes its whole record including that counter, and a fresh workspace starts at p1. The live server log shows the allocator is derived from the surviving set rather than persisted: after the 2026-08-26T23:00:45 restart that restored 1 workspace, the next id was `w4` (not w1 or w2) and the internal pane counter restarted at `pane_id=1`. Three herdr restarts appear in that log in four days. I could not create a workspace in the restarted headless probe ("ghostty error -2"), so the exact allocator rule is inferred from those observations, not directly proven. Test file: $SCRATCH/attack/tests/a1_topic_reuse.rs.
```

**Fix** — Store an identity alongside the pane id (workspace label + identity_cwd, or PaneInfo.terminal_id accepting that it rotates on a herdr restart) and treat a mismatch as `Target::Gone` rather than a hit. Handle `RosterEvent::PaneMoved` to migrate the binding. Close or rename a topic when its pane leaves the herd, and bound the `topics` map the way `pushes` is bounded.

#### `$REPO/crates/herdr-tg/src/bot.rs:381-402`

Callback data is `c|<pane>|<index>`; the option LABEL the operator tapped is not carried. bot.rs converts the index to a 1-based string and `deliver::choose` resolves it POSITIONALLY against a freshly-parsed dialog. If the agent moved to a different prompt between the push and the tap, the tap selects whatever now sits at that position — a button labelled 'Reject' can grant a permission. The allowlist is checked on callbacks, but nothing checks that the dialog is still the one the button was drawn for, and there is no confirmation step.

**Reproduction**

```
```
test a_stale_reject_button_grants_a_broader_permission_on_the_next_dialog ... ok
detail   : chose "Allow all" with Right Right Enter, but the dialog is still on screen
keys sent: ["Right", "Right", "Enter"]
```
The push rendered [Allow once][Allow always][Reject]; tapping Reject sends `c|pane|2` -> "3"; the current dialog is [Allow once][Allow always][Allow all][Reject], so "3" is Allow all. Real double-taps do happen — the live audit log has pairs 5 s apart (<chat-id> + <chat-id> on w9:p1, <chat-id> + <chat-id> on wB:p1). Test file: $SCRATCH/attack/tests/a4_callback.rs.
```

**Fix** — Put the label (or a short hash of the whole option list) in the callback data and refuse when the re-parsed dialog's option set does not match. `Prompt::match_option` already resolves a label exactly and refuses ambiguity — feed it the label instead of the index.

### MINOR (2)

#### `$REPO/crates/herdr-tg/src/bot.rs:386-388,398`

The append-only audit log — the accountability mechanism PLAN.md leans on — writes a `sent` record BEFORE the attempt, then writes nothing at all on the `Ok(Err(why))` branch where `choose` deliberately sent no keys ('that pane is no longer showing a choice', 'I don't know which option…'). The log therefore asserts a keystroke was sent for events where none was, and leaves no terminal record either way.

**Reproduction**

```
Against the operator's live ~/.local/state/herdr-tg/keystrokes.audit.log:
```
sent records: 18 | with no outcome/failed record after them: 7
  DANGLING: <chat-id>.834Z	sent	chat=-<chat-id>	pane=w9:p1	bytes=17	text=[button] option 0
  DANGLING: <chat-id>.986Z	sent	chat=-<chat-id>	pane=wB:p1	bytes=17	text=[button] option 0
  … 5 more
```
```

**Fix** — Rename the pre-write record to `attempted` and emit a terminal record on every branch, including `Ok(Err(_))` (`refused`, with the reason). A record that can dangle cannot be read as an audit trail.

#### `$REPO/crates/herdr-tg/src/notify.rs:513-524,546-556`

Two debounce leaks. (a) The snapshot replay that runs at startup and after every resubscribe pushes blocked/done panes with ZERO debounce, contradicting 'an ask that resolves inside the window never notifies'; under Restart=always / RestartSec=5 this fires on every restart for any ask younger than the window. (b) A debounce timer left over from an already-resolved ask fires against whatever the pane is doing when it wakes, short-circuiting the window for the NEXT ask.

**Reproduction**

```
Driven live against a throwaway probe session with debounce = 6 s:
```
[T+3] BLOCKED (resolves at T+5)   [T+5] WORKING   [T+7] BLOCKED (the real ask)
  | 2026-08-30T01:44:16 INFO subscribed to every agent-status edge panes=2
  | [  0.00s] BEAT Asked pane=w1:p4 seq=8     <-- replay, no debounce at all
  | [  9.14s] BEAT Asked pane=w1:p5 seq=14    <-- 2.1s after the ask, not 6s
```
The T+3 timer fired at T+9 and reported the T+7 question.
```

**Fix** — Give the replay the same debounce as an event-driven ask (or require the pane to have been blocked for >= debounce, using the age of state_change_seq). Tag each spawned timer with the seq it was armed for and drop its result if the pane's seq has moved on.

### What held

- `Gate` fails closed: an empty allowlist admits nobody (0, ±1, i64::MIN/MAX all refused), and a callback with no message yields chat_id 0, which is not admitted.
- `Target::Gone` really is returned on all three rules when the pane has left the herd — no rule silently re-points to a live pane, and `Target::None` never guesses a most-recent pane.
- `state_change_seq` is a sound dedupe key. Probed on a throwaway session: herdr keeps its own per-pane monotonic counter (idle→blocked→working→blocked gave 1,2,3,4), it IGNORES a caller-supplied `seq` in pane.report_agent (sent seq:1 and seq:2, observed 7 and 8), and it does not reset on pane.release_agent, on re-report, or on a different agent name. The 'seq that resets' attack is dead.
- The persisted `Seen` correctly suppressed a re-push across a bridge restart, and an ask that resolved inside the debounce window never buzzed — both driven live against the probe socket, not just unit-tested.
- A flapping pane did not spawn a timer per flap; when two timers did overlap, the second push was correctly deduped on (pane|ask, seq).
- `snapshot.agents` and the agent-bearing `snapshot.panes` agreed on the live herd, so `Seen::retain_alive` did not drop a live pane's key and force a duplicate buzz — the divergence I went looking for is not present.
- The mirror's relayed-hash correctly suppressed a redrawn paragraph, and my attempt to make a scroll RE-SEND an already-sent paragraph FAILED: the anchor handled it and only the genuinely new line came out.
- `deliver::choose` refuses a stale index that is out of range on a shorter dialog and sends no keys at all; `Prompt::match_option` refuses the ambiguous prefix 'allow' and bounds-checks numeric replies including '0'.
- herdr does not recycle pane numbers within a workspace's lifetime (closed w1:p2 and w1:p3; the next splits were p4 and p5) and `next_public_pane_number` survives a server restart — so the reuse hazard is confined to workspace-letter reuse, not everyday pane churn.
- `fit()` truncation is char-safe on multibyte input and never splits an HTML tag; `escape_html` neutralises hostile pane titles and relayed prose.
- The write surface really is confined: the only `PaneIo` impl naming herdr's write RPCs is in deliver.rs, operator text goes through `send_input` and never `send_text`, and the confirmation ladder never claims a rung it did not observe.
- The summarizer posts to http://127.0.0.1:8090 by default, so the gist adds no new off-box destination for pane content beyond the Telegram cloud D4 already accepted.

## Lens 3: what leaves the machine — FIX-FIRST

Pane content has already left the machine: 13 live ask excerpts from the operator's real herd went to a hosted provider (api.z.ai) via the gist gateway, and nothing in herdr-tg can detect or prevent a recurrence — the current safety is an unverified string match against another project's config file. Separately, the new mirror is the only unclamped message path in the bot: `fit()` is never applied to a pushed message and `worth_relaying` has a floor but no ceiling, so D4's \"never full pane dumps\" cap that `excerpt_from` enforces at 900 chars is bypassed entirely (5164 chars at this operator's real 61x86 geometry, over Telegram's limit, silently dropped) — while the test named `a_busy_pane_cannot_become_a_transcript` stays green because it only guards the ask path.

### BLOCKER (1)

#### `crates/herdr-tg/src/summarize.rs`

The gist has ALREADY sent 13 live pane excerpts off the machine to a hosted provider (api.z.ai), and nothing in herdr-tg can detect or prevent it. `Summarizer` fires at a URL it never validates, sends a task-class string it never verifies resolved, and reads only `/choices/0/message/content` from the reply — it ignores the `model` field that would tell it who answered. The local gateway's rule is `resolveTaskClass`: unknown or absent class -> `default` chain -> `glm-5.3-flash` -> `https://api.z.ai/api/coding/paas/v4`. Three independent routes reopen this: (a) the class header missing or misspelled, (b) `HERDR_TG_SUMMARIZER_CLASS=bulk` — a value the crate's own test sets — whose chain is `[local-qwen3, glm-5.3-flash]`, i.e. a hosted FALLBACK, (c) `HERDR_TG_SUMMARIZER_MODEL` pinning a provider and bypassing the chain entirely (the journal shows it was pinned to glm-5.3-flash on 4 of the last 9 service starts). The current default is local only because the string "autocomplete" happens to match a key in a DIFFERENT project's JSON file that herdr-tg never reads.

**Reproduction**

```
$ python3 -c "..." over ~/.llm-gateway/usage.jsonl + journalctl --user -u herdr-tg
journal 'pushing an ask' events: 74  window 2026-08-29 18:39:19+00 .. 2026-08-30 01:18:45+00
herdr-tg rows total: 151   hosted: 46
HOSTED gateway calls from project=herdr-tg within 8s of a live 'pushing an ask': 13
  gateway 2026-08-29T23:53:08.052Z class=default  glm-5.3-flash in=261tok  <->  ask pushed 2026-08-29T23:53:05.948Z
  gateway 2026-08-30T00:00:19.746Z class=default  glm-5.3-flash in=322tok  <->  ask pushed 2026-08-30T00:00:17.715Z
  gateway 2026-08-30T00:04:11.412Z class=default  glm-5.3-flash in=318tok  <->  ask pushed 2026-08-30T00:04:09.293Z
  ... 10 more, input 248-322 tokens each (= a ~900-char pane excerpt)

$ python3 -c "json.load(open('$HOME/Projects/llm-gateway/llm-gateway.json'))['routing']"
{"agentic-coding":["glm-5.3","openrouter-glm"], "bulk":["local-qwen3","glm-5.3-flash"],
 "default":["glm-5.3-flash"], "autocomplete":["local-coder"]}
providers: glm-5.3-flash -> https://api.z.ai/api/coding/paas/v4 ; local-coder -> http://127.0.0.1:8081/v1

$ sed -n '52,62p' $HOME/Projects/llm-gateway/src/router.ts
/** Resolve the effective task class from X-Task-Class header. Unknown -> "default" chain (per spec). */
  if (v in routing) return { taskClass: v, unknownClass: false };
  return { taskClass: "default", unknownClass: true };

$ journalctl --user -u herdr-tg -o cat | grep gist | sort | uniq -c
      4   INFO herdr_tg::bot: a one-line gist ... model="routed by class" class="autocomplete"
      4   INFO herdr_tg::bot: a one-line gist ... model=glm-5.3-flash

(Since the current build started 01:12:53Z all 4 gist calls went to local-coder — the exposure is closed by default today, not by construction.)
```

**Fix** — Make the destination provable from inside herdr-tg rather than inferred from another project's config. Three cheap, independent gates: (1) in `from_env`, refuse a `HERDR_TG_SUMMARIZER_URL` whose host is not loopback unless an explicit `HERDR_TG_SUMMARIZER_ALLOW_REMOTE=1` is set — fail closed, no gist; (2) parse the response's top-level `model` field and drop the gist (and log once at WARN) if it is not on a small local allowlist, so a chain fallback is caught after the fact instead of never; (3) drop `HERDR_TG_SUMMARIZER_MODEL` entirely, or gate it behind the same allow-remote flag, since its only documented effect is to bypass routing. Add a test that a non-loopback endpoint yields `None` from `from_env`.

### MAJOR (4)

#### `crates/herdr-tg/src/bot.rs`

The mirror bypasses D4's cap and is the only unclamped message path. `fit()` — the 4096-char clamp — is called in exactly one place, the `/status` render at line 458. Every pushed beat goes through `say_in_topic`, which sends `body` straight to `send_message` with no clamp. On the ask path that is safe because `excerpt_from` caps at 12 lines / 900 chars, which the module doc calls "D4's mitigation in code". The mirror path has no ceiling at all: `mirror::worth_relaying` has a 40-char FLOOR and nothing above it, and `new_since` returns the ENTIRE visible screen whenever the anchor line has scrolled away. So one settled tick after a scroll relays every prose line on the screen — literally the full pane dump D4 says never happens. Two consequences: the accepted-risk contract is broken, and any body over 4096 is rejected by Telegram, logged as one `tracing::error!` and dropped, so the topic silently loses exactly the longest thing the agent said. The test named `a_busy_pane_cannot_become_a_transcript` guards only `excerpt_from` and stays green.

**Reproduction**

```
$ grep -n 'fit(' crates/herdr-tg/src/bot.rs
458:            Ok(snap) => fit(render::herd_telegram(&snap)),
569:fn fit(html: String) -> String {        # + 4 hits in #[cfg(test)] only
(say_in_topic, lines 275-315, calls send_message(forum, body) with no fit())

$ probe realdim   # harness #[path]-includes the real mirror.rs/voice.rs/notify.rs, repo unmodified
synthetic screen: 61 rows, max col 86, 5164 chars      # == the operator's REAL geometry, from session.snapshot
mirror -> voice::said body: 5164 chars
Telegram limit 4096 exceeded: true

$ probe bigdump
mirror relay chars    : 8809
voice::said body chars: 8809
OVER LIMIT            : true
lines relayed         : 60
same screen through the ASK path (excerpt_from): 901 chars, 7 lines

$ ./target/debug/herdr-tg status --json | python3 -c "...viewport_rows..."
w9:p1 rows= 61 ... wA:p1 rows= 61 ... wC:p2 rows= 61      # 61 x 86 = 5246 chars of headroom

Measured volume of the live service (read-only, from its own journal):
$ journalctl --user -u herdr-tg -o cat | grep relaying | awk -F'chars=' ...
ALL-TIME relays: 30   chars to Telegram: 28077   mean 936   max 2471
busiest 10-min bucket: 20 relays, 19881 chars   (~120 KB/hour of pane prose)
```

**Fix** — Call `fit()` on `body` inside `say_in_topic` (both the forum and the flat branch) — that alone stops the silent drop. Separately, give the mirror the ceiling the ask path has: cap `worth_relaying` at the same EXCERPT_LINES/EXCERPT_CHARS budget and cut from the front with the same leading '…', so "never full pane dumps" is one constant shared by both paths rather than a property of one of them. Then move `a_busy_pane_cannot_become_a_transcript` to assert it of the mirror too — the current test's name promises a property the mirror does not have.

#### `crates/herdr-tg/src/config.rs`

No workspace scope is configured, so the bridge mirrors EVERY workspace on the box, not one. `Config::load(None)` returns `workspace: None`; `snapshot_for(client, ctx.workspace.as_deref())` then skips `narrow_to_workspace` and the mirror loop iterates every agent pane in the herd. The unit runs `herdr-tg serve` with no `--config`, and no `herdr-tg.toml` exists anywhere on the machine, so this is the live configuration, not a hypothetical one. D2 is "one bot per workspace"; the default is all workspaces, i.e. the scope fails open. Confirmed in the journal: four unrelated projects' panes have already been relayed into Telegram, including wE:p1 — the coordinator's own pane, which is where this review session runs.

**Reproduction**

```
$ tr '\0' ' ' < /proc/3279411/cmdline
$HOME/.local/bin/herdr-tg serve
$ ls ~/.config/herdr-tg/*.toml $REPO/herdr-tg.toml
ls: cannot access ...: No such file or directory   (both)
$ journalctl --user -u herdr-tg -o cat | grep -c workspace
0
$ journalctl --user -u herdr-tg -o cat | grep relaying | sed -E 's/.*pane=(w.):.*/\1/' | sort -u | tr '\n' ' '
w9 wA wC wE       <- 4 distinct workspaces mirrored
$ ./target/debug/herdr-tg status
  w9:p1  bliz-monorepo   ... wA:p1  omarchy-lab ... wC:p2  llm-gateway ... wE:p1  herdr-tg
$ journalctl ... | grep 'relaying pane=wE:p1' | wc -l  ->  5 relays, 7566 chars
```

**Fix** — Make the scope explicit and fail closed the way the chat allowlist does: if `workspace` is absent from both the TOML and a `HERDR_TG_WORKSPACE` env var, refuse to start with the same shape of error `HERDR_TG_TOKEN` gets ("one bot per workspace — set workspace = ... or pass --workspace"). A bridge that streams terminals to a cloud chat should never infer "all of them" from a missing field. If an all-workspaces mode is genuinely wanted later, it should be `workspace = "*"`, typed by a human.

#### `crates/herdr-tg/src/bot.rs`

The operator's real Telegram user id is committed and PUSHED to the public repo, and the de-identification guard structurally cannot see it. `<chat-id>` appears three times in bot.rs's tests and is byte-identical to the live entry in HERDR_TG_ALLOWED_CHAT_IDS. `fixtures_are_deidentified.rs` — widened one commit ago — still only enumerates `crates/*/tests/fixtures`; it never looks at src/, docs/, scripts/, PLAN.md or git history, and its five detectors are all structural shapes (home path, ses_ id, UUID, user@host prompt, ~/path) with no detector for a username, a hostname, or a chat id. So the widening moved the boundary from one fixture directory to all fixture directories; it did not widen the class of thing being looked for, and the class that leaked here is one it has never looked for. The operator's username and both machine names are public for the same reason.

**Reproduction**

```
$ git grep -n '<chat-id>' origin/main
origin/main:crates/herdr-tg/src/bot.rs:904:        for probe in [0, 1, -1, <chat-id>, i64::MAX, i64::MIN] {
origin/main:crates/herdr-tg/src/bot.rs:911:        let g = gate(&[<chat-id>, -<chat-id>]);
origin/main:crates/herdr-tg/src/bot.rs:912:        assert!(g.admit(<chat-id>));

$ python3 -c "parse ~/.config/herdr-tg/env, print only booleans"
HERDR_TG_ALLOWED_CHAT_IDS: len=24 matches_repo_<chat-id>=True     # 9 + 1 + 14 = the whole value

$ git grep -I -n -i 'user' origin/main
PLAN.md:3,21,25,39,172,174 (<host>, user-box)
crates/herdr-client/src/client.rs:209:  ... The socket is `srw------- user:user`, so filesystem
crates/herdr-client/tests/support/mod.rs:14: ... gateable on user-box (D6).
docs/SLICE-1.md:4,87,760,1016,1088,1265,1305

$ python3 sweep_git.py    # the guard's own five detectors, re-run by hand over EVERY blob
commits scanned: 20  distinct blobs: 157
distinct findings: 12   -- all placeholders (/home/testuser, user@host:~, ~/Projects, ...)
(i.e. the shape detectors are clean; the chat id and the hostnames are invisible to them)
```

**Fix** — Two separate moves. (1) Content: replace `<chat-id>` in bot.rs with an obviously-synthetic id, and treat the published one as burned — a Telegram user id is permanent and the repo history already carries it, so a history rewrite plus a force-push is the only real removal; decide whether that is worth it, but do not leave it believing the guard covered it. (2) Guard: give it the two things it lacks — scan the whole worktree (or at minimum src/, docs/, scripts/, *.md) rather than fixture directories, and add the identity needles `scripts/scrub-fixtures.py` already derives at runtime (getpass.getuser(), socket.gethostname(), basename($HOME)) plus a 9-10-digit-bare-integer heuristic for chat ids. The python check already knows how to derive identity without hardcoding it; the Rust test is the one that runs on every commit and it is the one that cannot.

#### `crates/herdr-tg/src/voice.rs`

`looks_like_prose` is a typography test, not a redaction test, and the mirror uses it as the only thing standing between an agent's screen and Telegram. It rejects a line for starting with $ + - # > | /, containing :: or @@, or carrying a file.ext:line reference, then checks letter density and punctuation density. A secret in a sentence satisfies all of that. 10 of 12 crafted secret-bearing lines pass, including a bare `ANTHROPIC_API_KEY=sk-ant-...` assignment (it passes because it has only two underscores and one equals sign — `STRIPE_SECRET_KEY=sk_live_...` is rejected only because it happens to have more underscores, which is accident, not policy). On the operator's real herd, 73% of cleaned pane lines classify as prose and are therefore relay-eligible. D4's stated mitigation for this is "agents redact secrets in asks" — but the mirror is not an ask: it fires unprompted every 4 seconds on prose the agent never intended to send anywhere.

**Reproduction**

```
$ probe prose      # calls the real crate::voice::looks_like_prose, unmodified
 PROSE  api key in a sentence        I set the key to sk-ant-api03-9f2a7bC1dEf4... and the calls work now.
 PROSE  .env narrated                I read the file and it has GITHUB_TOKEN set to ghp_16C7e42F... right there.
 PROSE  token in an error message    The request failed with 401 because the bearer token eyJhbGciOiJIUzI1... has expired.
 PROSE  plain env assignment line    ANTHROPIC_API_KEY=sk-ant-api03-9f2a7bC1dEf4Gh6Ij8Kl0Mn2Op4Qr6St8Uv0Wx2Yz
  code  env line, bare               STRIPE_SECRET_KEY=sk_live_51H8xYz...
10/12 secret-bearing lines classified as PROSE (=> relayed verbatim to Telegram as chat text)

MIRROR WOULD SEND:
I set the key to sk-ant-api03-9f2a7bC1dEf4Gh6Ij8Kl0Mn2Op4Qr6St8Uv0Wx2Yz and the calls work now.
That unblocks the deploy for the rest of the afternoon.

$ probe filterhold ~/.cache/tmp/hreview/samples   # 360 real screens, 4 real agent panes, 6 min
real screens scanned; non-blank cleaned lines: 12666, classified PROSE: 9265 (73%)
distinct PROSE lines carrying a path / host / key-shaped token:
  1. bash ~/v042-taps.sh — installs strace+time (push gate) + you bless the leak-
  pasted literal key currently loads silently). Then the map gets re-verified by
  poisoned git remote recovered, tracked API key removed, two battery suites
```

**Fix** — Add a redaction pass between `worth_relaying` and `voice::said` — it belongs in the relay, not in the prose classifier, because the two questions are different ("is this a sentence" vs "does this sentence contain a credential"). A short, boring pattern set covers the realistic cases: high-entropy runs of >=20 base64/hex chars, the common vendor prefixes (sk-, sk-ant-, sk-proj-, ghp_, gho_, xoxb-, AKIA, eyJ), `KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL\s*[=:]\s*\S+`, and `scheme://user:pass@`. Replace the match with `[redacted]` rather than dropping the line, so the operator still sees the sentence and can tell something was removed. Same pass should run on `excerpt_from`, which has the same hole but a much smaller window.

### MINOR (3)

#### `crates/herdr-tg/src/routing.rs`

`routing.state.json` and `pushed.state.json` are world-readable (0644) while the audit log next to them is deliberately 0600. Both `Routing::save` and `notify::Pushed::save` use `std::fs::write`, which creates at 0666 & ~umask — the unit's UMask is 0022, so 0644 by construction on any box, not an accident of this one. The temp file written before the rename has the same mode. The files carry the operator's Telegram user id, the forum supergroup id, the pane->topic map and the pane->message-id map. No pane content, but the audit module's own reasoning ("Mode 0600 at creation — this holds the operator's own words") applies to a chat id just as much: it is the identity the whole allowlist is built on.

**Reproduction**

```
$ ls -la ~/.local/state/herdr-tg/
-rw-------  1 user user 2835 keystrokes.audit.log
-rw-r--r--  1 user user  108 pushed.state.json
-rw-r--r--  1 user user 4418 routing.state.json
$ stat -c '%A %n' ~/.local/state/herdr-tg
drwxr-xr-x $HOME/.local/state/herdr-tg
$ umask ; systemctl --user show herdr-tg -p UMask
0022
UMask=0022
$ head -c 200 ~/.local/state/herdr-tg/routing.state.json
{ "sticky": { "-<chat-id>": "wE:p1", "<chat-id>": "wA:p1" }, ...
$ sed -n '194,201p' crates/herdr-tg/src/routing.rs
        let tmp = path.with_extension("state.json.tmp");
        std::fs::write(&tmp, body)?;      # <- default perms, no OpenOptions::mode
```

**Fix** — Write both state files the way `audit::append` already writes its log: `OpenOptions::new().create(true).write(true).truncate(true).mode(0o600)` on the temp file before the rename (rename preserves the mode), and `std::fs::set_permissions(dir, 0o700)` after `create_dir_all`. The audit module already has the exact code; this is lifting it two directories over.

#### `crates/herdr-tg/src/config.rs`

`Config` and `Summarizer` both derive `Debug` and print their credentials verbatim, and the test that claims otherwise does not test it. `the_token_is_reachable_only_through_its_accessor` asserts `c.token() == "t"` — it checks that the accessor works, not that the accessor is the only route. The derived Debug is a second read of the credential that the module's stated rationale ("a method rather than a public field: it makes every read of the credential a visible call site that a reviewer can grep for") explicitly exists to prevent, and it is exactly the read a grep for `.token()` will not find. Nothing formats either struct today, so this is latent, not live — but `Ctx` holds the `Summarizer` and one `tracing::debug!(?cfg)` added by a future maintainer puts a live bot token in the journal.

**Reproduction**

```
$ probe token      # constructs the real config::Config via Config::load(None)
Config Debug output:
  Config { token: "<chat-id>:AAH1a2B3c4D5e6F7g8H9i0J1k2L3m4N5o6P", allowed_chat_ids: {1},
           workspace: None, socket: None, submit_key: Key("Enter"), forum_chat_id: None }
  token present in Debug output: true

Summarizer Debug output:
  Summarizer { endpoint: "http://127.0.0.1:8090/v1/chat/completions", task_class: Some("autocomplete"),
               model: None, key: "lg_live_THIS_IS_THE_GATEWAY_KEY", timeout: 4s }
  gateway key present in Debug output: true

$ grep -rn 'derive(Debug' -A2 crates/herdr-tg/src/config.rs crates/herdr-tg/src/summarize.rs
config.rs:59:#[derive(Debug, Clone)]  pub struct Config { token: String, ...
summarize.rs:35:#[derive(Debug, Clone)] pub struct Summarizer { ... pub key: String, ...
```

**Fix** — Hand-write `impl fmt::Debug` for both, printing `token: "<redacted>"` / `key: "<redacted>"` and everything else unchanged. Then make the existing test actually assert the property its name claims: `assert!(!format!("{c:?}").contains("t"))` with a distinctive token value. Two structs, ten lines, and the invariant stops being a comment.

#### `scripts/scrub-fixtures.py`

`--check` silently skips every fixture that is not `.json` or `.ndjson`, so the only check that knows the operator's real username and hostname never runs on the two fixtures that are verbatim screen captures. `fixture_files()` filters on `p.suffix in {".json", ".ndjson"}`, so pointing it at `crates/herdr-tg/tests/fixtures` inspects one file out of three and prints "CHECK CLEAN" — `tui-pane.txt` and `opencode-permission.ansi` are never opened. Those two are precisely the files captured off a real screen, and `tui-pane.txt` is the file that carried a real session id one commit ago. The Rust guard does read them (any extension, read_to_string), but the Rust guard has no identity needles; the python check has the needles and cannot see the files. Both are clean today — by hand, not by gate.

**Reproduction**

```
$ python3 scripts/scrub-fixtures.py --check --fixtures crates/herdr-tg/tests/fixtures
  clean gist-cases.json
scrub-fixtures: CHECK CLEAN
rc=0

$ python3 -c "import scrub-fixtures as m; print([p.name for p in m.fixture_files(...)])"
herdr-tg dir -> ['gist-cases.json']                # tui-pane.txt and opencode-permission.ansi absent
client   dir -> ['snapshot.json','pane_read.json','events-mixed.ndjson','errors.ndjson','pong.json','herdr-schema-p20.json']
needles: ['$HOME', 'user', '<host>']        # the needles exist; the files never reach them

$ ls crates/herdr-tg/tests/fixtures/
gist-cases.json  opencode-permission.ansi  tui-pane.txt
```

**Fix** — Drop the suffix filter from `fixture_files()` for the CHECK path (keep it for the SCRUB path, which legitimately only rewrites line-delimited JSON) — a file it cannot parse should still be string-scanned, the way `read_frames()` already returns None for check-only and says so. And have `capture-fixtures.sh` run `--check` over every crate's fixture directory, not just the client's, so the python needles cover the same set the Rust test does.

### What held

- Git history is credential-clean. Scanned all 157 distinct blobs across all 20 commits reachable from origin/main for Telegram-token, sk-*/ghp_*/lg_* shapes: zero hits. `scan-secrets` agrees ("no secrets found (all scope)").
- The five shape detectors in `fixtures_are_deidentified.rs` genuinely work. I re-implemented them in Python and ran them over every blob of every commit, not just fixture directories: the only matches anywhere in history are the intended placeholders (/home/testuser, user@host:~, ~/Projects, ~/.config). No real home path, session id, UUID, shell prompt or tilde path has ever been committed.
- The bot token cannot reach the log at any level, including RUST_LOG=trace. `tracing-log` is absent from the dependency graph (`cargo tree -i tracing-log` -> no such package), so the `log`-crate records from teloxide-core/reqwest/hyper — which carry the token inside the Bot API URL — are dropped before they reach the subscriber. teloxide-core 0.13's only URL-adjacent log calls are in the opt-in Trace adaptor, which is not used.
- The audit log is correct where it matters. Mode 0600 at creation by explicit `OpenOptions::mode`, verified on the live file; append-only with `O_APPEND` and no held handle; a multi-line, tab-bearing reply cannot split one record into two (escape() is unambiguous and its own test proves \\n != literal backslash-n).
- Nothing but the audit log records the operator's words or pane content on disk. routing.state.json and pushed.state.json hold pane ids, Telegram message ids and chat ids only. The journal at RUST_LOG=info logs char COUNTS (`relaying pane=w9:p1 chars=1141`), byte counts (`answered a reply ... bytes=16`) and pane ids — never text. The llm-gateway's usage.jsonl ledger records tokens/latency/cost, never the prompt.
- D3 holds for the gist: no LLM sits on the reply path. `Summarizer::one_line` has exactly one call site in the whole crate — `push_ask`, agent->operator. The mirror does not call it either, so the continuous prose stream never reaches the model; only the 900-char ask excerpt does.
- The gist fails open and stays off by default. Unreachable gateway, non-2xx, malformed JSON, empty content, and an over-long/multi-line/markdown/answering reply all yield None and the push still goes; no `HERDR_TG_SUMMARIZER_KEY` means the whole gate is absent, and it never scavenges another tool's credential file.
- Every agent-authored string reaching Telegram is HTML-escaped through one function (`render::escape_html`, covering & < >), gist included — I could not get `<script>` through `voice::asked` or `voice::said`.
- `%h/` (the broken-TMPDIR scratch tree), `*.audit.log`, `*.state.json`, `*.state.json.tmp`, `herdr-tg.toml` and `opencode.json` are all correctly gitignored; none is tracked at origin/main.
- I tried to make the mirror relay a non-agent pane and could not: the loop filters on `agent.is_some() || display_agent.is_some()`, and the six shell panes in the live herd were never read. `prime()` also correctly suppressed the startup screenful in every replay.
- 232 tests pass on the current tree with the corrected TMPDIR (the 7 transport failures are the inherited `%h/...` TMPDIR, not the code).
