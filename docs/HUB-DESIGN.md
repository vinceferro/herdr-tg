<!-- PROPOSAL, not a decision. Synthesised 2026-08-31 from four competing architectures
     scored by three judges. Published for the operator at:
     https://claude.ai/code/artifact/9a699dbd-a026-47ea-abd0-721184b4793f
     Nothing here is built. The first commit against it is slice 1, and slice 1 deletes nothing. -->

# herdr-tg → the hub

**Decision: build "The Topic List Is The Product" as the spine.** Graft Deadman's failure mechanisms, Single Door's slice order, Thin Hub's zero-ceremony naming.

Where the judges split, I decided:

- **Buildability judge said build #4, failure judge said #2, operator judge said #3 — I took #3** because the topic list is the thing you actually look at, and #2 and #4's best parts are mechanisms that graft onto any spine while #3's are not.
- **#3 wanted a hand-authored display name; I took #1's derived-from-repo-path name** because fixing a topic title should not require a laptop and a JSON edit.
- **#2 and #3 both wanted the hub to absorb kickoff's restart/degradation alarms; I refused** because the alarm that tells you the system is broken must not depend on the system.
- **#1 and #4 chose `--dangerously-load-development-channels`; I killed it** — see Risk 1, I found the blocker in the binary.

---

## 1. What it is, in five lines

One bot. One forum. One topic per project.
herdr-tg holds the only Telegram connection and listens on one Unix socket.
Each kickoff worker runs a small bridge that talks to that socket instead of Telegram.
The ANSI parser and the typing path are deleted — messages are structured end to end.
Your phone's topic list becomes the herd view, for free.

---

## 2. Components, and who owns what

| Component | Owns | Does not own |
|---|---|---|
| **herdr-hub** (this repo, `herdr-tg serve`) | the bot token, the single long-poll, the chat allowlist, the forum + topics, routing, the tap ledger, the audit log, the outbound rate budget, the summarizer | starting projects, supervising anything, Mission Control writes |
| **kickoff-channel** (bridge, ~300 lines TS, ships in claude-kickoff's plugin) | one project's session: MCP stdio to Claude Code, NDJSON to the hub | Telegram, any token, any allowlist, any LLM |
| **claude-kickoff** | project identity, preflight, the autonomy grant, process-group lifecycle, Mission Control | Telegram |
| **llm-gateway** | model routing, per-project keys and budgets | anything on the relay path |
| **herdr-tg-watchdog** (~15 lines shell + systemd timer) | telling you the hub is dead | everything else |

The watchdog shares no code and no runtime with the hub. That is the point.

---

## 3. The socket protocol

**Transport.** `AF_UNIX` / `SOCK_STREAM` at `/run/user/<uid>/kickoff/hub.sock`. Directory 0700, socket 0600. No TCP ever.

Path is derived on both sides, not configured. I checked: `XDG_RUNTIME_DIR` is **not** in `_WORKER_ENV_KEEP` (scripts/kickoff:6265), so it does not survive the `env -i` boundary. `HOME` and `USER` do. Derive `/run/user/$(id -u)`.

**Framing.** NDJSON. One JSON object per line, LF-terminated, UTF-8, 64 KiB max per frame. Three rules from herdr-client, each of which cost a real debugging session:

1. The trailing newline is a type-level invariant in the encoder, not a caller's job. Omitting it made herdr hang forever — 5.01s, zero bytes, connection still open.
2. Read exactly one line. Never to EOF.
3. Restore the `Take` limit after every complete line. A lifetime ceiling ends a stream in silence that reads as a disconnect that never happened.

Over 64 KiB is a protocol error: `refused`, then close. Never truncate.

**Envelope.** Every frame: `{"v":1,"t":"<kind>","id":"<frame-id>", …}`. `id` is opaque, per-connection, monotonic.

**Version skew is first-class** — hub and bridge ship from different repos. Major `v` mismatch on `hello` → refuse, naming `kickoff pull`. Unknown frame kind → log and ignore. Unknown field in a known kind → ignore.

### Bridge → hub

| kind | payload | buzzes |
|---|---|---|
| `hello` | `{project_id, token, instance, repo, pid}` — first frame, once | — |
| `say` | `{text, hint?: "prose"\|"output"}` | no |
| `ask` | `{ask_id, text, options?: [{option_id, label}]}` | **yes** |
| `ask_resolved` | `{ask_id, how: "answered"\|"withdrawn"\|"timeout", outcome?}` | no — it **edits** |
| `done` | `{text}` | **yes** |
| `beat` | `{state: working\|idle\|blocked\|done, note?}` | no |
| `ack` | `{ref, status: "accepted"\|"refused", reason?}` | — |
| `bye` | `{reason}` | no |
| `pong` | `{id}` | — |

### Hub → bridge

| kind | payload |
|---|---|
| `welcome` | `{project, topic_id, limits:{max_frame, max_text, frames_per_min}}` |
| `refused` | `{reason}` then close. Closed set: `unknown_project`, `bad_token`, `already_claimed`, `version_skew`, `not_enabled` |
| `message` | `{msg_id, text, from:{chat_id,user_id}, in_reply_to_ask?}` — the operator's words, **opaque** |
| `choice` | `{msg_id, ask_id, option_id}` — resolved against the written-down record |
| `ack` | `{ref, delivered: "yes"\|"no"\|"unseen", why?}` |
| `ping` | `{id}` |

**Four things that carry weight:**

`ack` has three values, not two. `unseen` means "went out and could not be checked". Telegram has no idempotency key, so a send that times out may or may not have landed. A message carrying a keyboard is **at-most-once** — on an ambiguous failure the hub does not retry, because two live menus for one question on buttons that stay tappable forever is a manufactured misfire. This is `audit.rs`'s three-way vocabulary, kept.

**Every frame is acked exactly once.** `why` is a closed enum: `too-fast`, `clamped`, `no-topic`, `telegram-refused`. Today a rejected send is one `tracing::error!` and a drop — that already lost 5,164 characters of a real agent's longest message. Backpressure now reaches the only party that can act on it.

`ask_resolved` retires a stale keyboard. The hub edits the original message, strips the buttons, appends `✓ answered at the terminal — No`. No pane-reading design could ever do this: a screen cannot tell you a question stopped being asked. This is the single biggest phone win in the design.

`ask_id` is real evidence. `still_the_same_question` currently leans on herdr's `state_change_seq` and refuses when absent. A bridge-minted id makes it a fact.

### Identity and authentication — four layers, all fail-closed

1. **`SO_PEERCRED`.** Read uid off the connection. Wrong uid → close, no reply. Mode 0600 implies it; reading it back means the check survives a permissions mistake and yields the pid.
2. **Registered id + minted token.** `herdr-tg enroll <repo>` mints 32 random bytes into `<repo>/.kickoff/hub.token` (0600, gitignored) and stores only `sha256` in the hub's registry. Compared constant-time. uid proves "some process of this human", not "the bridge of project X" — and an agent in project A can spawn processes.
3. **The name comes from the registry, never from the wire.** `hello` carries no name. The hub resolves token → project and uses that for the topic title, the routing key, the audit subject. A bridge cannot claim a name, so it cannot impersonate a topic — and `escape_html` not covering a Telegram API field stops mattering.
4. **Single claim.** One live connection per project. A second `hello` for a live project is **refused**, not a takeover — a takeover is what bridge-murder felt like from the inside. If the incumbent's pid is gone from `/proc`, evict and admit. If it is alive, refuse **and post one line into that topic naming both pids**, because a silently deaf worker is the failure that cost this system a whole reaping mechanism.

**Backpressure.** The hub enforces a per-chat token bucket (1/s, 18/min) and `fit()` on every send path. Beyond a sustained per-project rate it coalesces into an edited status message; past a hard ceiling it sends one `throttled` line and pauses that project's `say` frames. Never a silent loss.

---

## 4. Bridge: start, supervise, stop

**Started by Claude Code**, as the session's channel MCP server. Nothing new is supervised, spawned, or reaped.

The bridge is TypeScript on bun, same shape as the official telegram plugin. That is deliberate: its argv is `bun … server.ts`, which matches `bridge_present()`'s existing `*bun*server.ts*` arm verbatim (supervisor.sh:569). The ~500-line never-up escalation belt needs **zero edits** and keeps meaning the same thing.

```
kickoff up → supervisor.sh → session-run.sh → exec claude --channels plugin:kickoff-channel@kickoff-local
  → Claude Code spawns bun .../kickoff-channel/server.ts
  → bridge connects to /run/user/<uid>/kickoff/hub.sock, sends hello, gets welcome
```

**Hard rule for the bridge:** no `setsid`, no `disown`, no daemonising. It must stay in `SESSION_PGID` or `stop_session` cannot reap it and `bridge_present` cannot see it.

**Stopped by** `kill -TERM -- -$SESSION_PGID`. The bridge writes `bye{reason:"refresh"}` best-effort with a 200 ms budget, then exits with the group. A bridge that hangs saying goodbye turns a clean restart into a SIGKILL.

On a clean `bye` inside a 90-second grace window the hub says **nothing**. A phone that buzzes on every context refresh is worse than useless.

### The door — this is the part I changed from every proposal

I read the gate in `claude 2.1.250`.

**`--dangerously-load-development-channels` is dead.** It renders an interactive confirmation — `WARNING: Loading development channels` / `I am using this for local development` / `Yes, I accept` / `No, exit`. A worker whose stdin is `tail -f /dev/null` can never answer it. It would hang looking alive. That is worse than the deafness it was meant to avoid. Two proposals made it plan of record.

**`server:` specs are also dead.** The binary says it plainly: `server: entries need --dangerously-load-development-channels`.

**The door is the plugin allowlist**, and the machinery already exists on this box:

- `~/.claude/plugins/known_marketplaces.json` already registers `kickoff-local` → `~/kickoff-versions/<pinned-core>/plugin`
- `kickoff@kickoff-local` is already installed at project scope for herdr-tg and eight others

So: add **one plugin**, `kickoff-channel`, to the existing `kickoff-local` marketplace. `kickoff adopt` installs it the way it already installs `kickoff`. Then, in `/etc/claude-code/managed-settings.json`:

```json
{
  "channelsEnabled": true,
  "allowedChannelPlugins": [
    { "marketplace": "kickoff-local",           "plugin": "kickoff-channel" },
    { "marketplace": "claude-plugins-official", "plugin": "telegram" }
  ]
}
```

Two things to get exactly right. The schema is `array of {marketplace, plugin}` **objects** — I read the zod: `allowedChannelPlugins: C(m({marketplace:i(), plugin:i()}))`. A string array silently fails. And an org list **replaces** the default allowlist, so the official telegram plugin must be re-listed or every not-yet-migrated project goes deaf on the same day.

**Per-repo opt-in** is one line in `.kickoff/instance.env`:
```
CHANNEL_SPEC=plugin:kickoff-channel@kickoff-local
```
`CHANNEL_SPEC` is already on the frozen whitelist. Rollback is deleting that line.

### The liveness transfer is staged, not done at once

kickoff's never-up belt stays untouched in slice 1. In slice 4 the hub gains its own alarm — a project whose supervisor lock is held but which has not connected in 120 s produces one message — proven against a deliberately broken bridge. Only then does the belt shrink.

**Delete a watchdog when something else demonstrably alarms on the same condition, never when the thing it watches changes shape.**

Free finding while I was in there: `bridge_present()` and `bridge-reap.sh:_br_cmd_matches()` are **already out of sync today** — the reaper is missing the `*opencode-telegram*` arm. Add the test pinning the two lists together in the same commit. That gap exists whether or not you build this.

---

## 5. Project → topic

**The key is the project id**, minted once at enrolment from the canonical repo path. Never a counter. Counters get recycled, and a recycled id silently inheriting a dead agent's topic is already a confirmed defect here.

**Title = repo basename**, sanitised to `[A-Za-z0-9._-]`, clipped to 48 chars. `herdr-tg`, `llm-gateway`, and the other repos on this box. Collisions between distinct paths get a 6-hex suffix. Overridable at the terminal, never over the wire. Zero naming decisions for you.

`icon_color` = `hash(project) % 6` over the six permitted values. Six projects colour-coded, free, stable forever.

**First run.** Created on first *live* connection, not on `hello`. Then **greeted immediately** — a topic with no messages is invisible in Telegram's topic list, so a silently-created topic is the same as no topic to the person looking for it.

**Restart.** Same project → same topic → same history. **No message.** Eleven projects recycling on a `kickoff pull` produces zero buzzes. A new `instance` invalidates every outstanding ask, so a tap on a menu drawn for a dead session is refused with a reason you can read.

**Never renamed for status.** `editForumTopic` emits a `forum_topic_edited` service message, which would replace the list preview line — the one line you actually read — with "Topic edited".

**Never deleted, never closed.** Grant the bot `can_manage_topics` and nothing else. Deleting needs `can_delete_messages`; whether an admin bot can post into a *closed* topic is undocumented, and a wrong guess silently swallows a project's last messages.

**If you delete a topic**, there is no service message and no `getForumTopics`. The hub learns from `Bad Request: message thread not found` on a send. That is handled as first-class rebinding — unbind, create, greet, resend, say once that it was recreated. Never retried as a transient.

**Routing is one rule.** Topic + inside the one configured forum chat → that project. Rule 0 only. Every other supergroup numbers reply threads with plain message ids drawn from the same counter, which is how a swipe-to-reply on DM message #20 reached a forum pane. Deleting rules 1 and 2 deletes that failure rather than testing against it.

**A bare sentence in General** gets the digest plus one button per live project — and the hub writes your typed text into a `PromptRecord` beside it. One tap delivers it. You never retype.

**A load-bearing coincidence, write it in the code.** The bot must be an admin to create topics at all. Admin also disables privacy mode, which is the only reason a plain sentence typed in a topic reaches the bot. Narrow the grant and topic creation breaks *and* every message silently stops arriving — and those look like two bugs.

---

## 6. herdr-tg, module by module

### Kept, essentially verbatim

| module | lines | tests | why |
|---|---|---|---|
| `summarize.rs` | 2,265 | 34 | its input is "text you will read"; panes were never its subject |
| `audit.rs` | 342 | 8 | subject renamed to project; `unseen` → `unacked`, same reason |
| `config.rs` | 269 | 5 | drops `submit_key`/`workspace`/`socket`; `forum_chat_id` becomes required |
| `bot::Gate` | 12 | 2 | fail-closed, checked first, silence not refusal |
| `render::escape_html`/`plain_text`/`clip`, `bot::fit` | ~120 | part of 13 | the message-safety layer |
| `fixtures_are_deidentified.rs` | 352 | 3 | repo policy, protocol-independent |

`fit()` gets re-sited. I checked: it has exactly **two** real call sites today, both in `/status`. Every send path gets it, and one test each.

### Changed

| module | lines | tests | change |
|---|---|---|---|
| `routing.rs` | 982 | 22 | `PaneId`→`ProjectId`; the `alive` closure takes `&BTreeSet<ProjectId>`. Four call sites, one test helper. On-disk shape does not move. `PromptRecord` kept, anchor upgraded to `ask_id`. Rules 1 and 2 deleted. |
| `voice.rs` | 1,009 | 29 | ~two-thirds kept: the eight rules and their tests, `is_the_same_text`, the JARGON absence test. Ladder messages deleted. Added: `answered_elsewhere`, `project_went_dark`, `two_bridges_claim`, `throttled`. |
| `notify.rs` | 995 | 21 | anti-spam half kept whole — persisted `Seen`, spawned-per-target debounce, `OnBeat` as a callback. Dedupe key → `(project, ask_id)`. Chrome stripper deleted. Excerpt caps re-sited as a fleet-wide ceiling. |
| `bot.rs` | 1,788 | 20 | plumbing kept: startup order, `say_in_topic`, `topic_for`, `ensure_all_topics`, `push_ask_with`'s ordering, `resolve_tap`, `reply()` logging. Pane semantics deleted. |
| `render.rs` | 627 | 13 | `herd_telegram` kept, roster swapped in. `herd_table`/`event_line`/`roster_line` deleted. |
| `no_live_write_call_site.rs` | 2,706 | 30 | **re-aimed, never deleted.** New subject: nothing outside `summarize.rs` builds an HTTP client; nothing outside `hub.rs` opens or accepts a socket. Its six invariants have caught five distinct bypasses, one of which shipped a live `send_text` past fmt, clippy and 232 tests. |

### Deleted outright

| module | lines | tests |
|---|---|---|
| `permission.rs` | 1,754 | 36 |
| `deliver.rs` | 2,033 | 28 |
| `mirror.rs` | 296 | 10 |
| `cmd/` (status, read, doctor, watch, mod) | 664 | 8 |
| `herdr-client` src (8 files) | 2,792 | 33 |
| `herdr-client` protocol tests (wire, golden, schema_drift, events, failure_paths) | 2,556 | 38 |
| herdr proof scripts | ~400 | — |

**~10,100 lines and 153 tests deleted.** Salvaged from herdr-client: the newline invariant, read-exactly-one-line, the per-frame `Take` restore, the single-place exit-code map, `opaque_id!` as `ProjectId`, and the in-process socket stand-in.

### New

`hub.rs` (listener, admission, roster), `queue.rs` (token bucket, `retry_after`, coalescing), `lock.rs` (flock, pidfile, the 409 counter), `heartbeat.rs`, `crates/hub-proto`. Plus `kickoff-channel/server.ts` in claude-kickoff.

---

## 7. llm-gateway

**Stays exactly where it is. The hub calls it; bridges never do. Nothing centralises.**

Telegram *forces* one consumer per token — that hard constraint is what makes centralising the front door right. **Nothing forces centralised inference.** The gateway is loopback-bound, per-key authenticated, already serving 24 project keys. Proxying would collapse eleven projects into one ledger row, make `sticky` shared mutable state across every project, and put a network call with a timeout inside the one process in the fleet that must never stall.

**Correct the diagnosis.** All 151 herdr-tg ledger rows read `fallback_used: false`. Nothing fell through. 44 of the 46 hosted calls went hosted because `routing.default` **is** `[glm-5.3-flash]`. Two more were cross-class sticky bleed. `fallback_used` is computed against the sticky-*reordered* chain, so it reports false for **all three** silent-hosted paths. It is not usable as a leak detector.

**Where the gates live.** Two gates, disjoint failure sets:

- **The caller owns transport leaks**, because it owns the socket — an ambient `HTTP_PROXY`, a 307 re-sending the POST body, a moved `localhost`. All three have carried a real excerpt past a passing address gate. `summarize.rs` keeps every one: real URL parse, `loopback_pin`, `.no_proxy()`, `redirect::Policy::none()`, `remote_addr()` read back *before* the status is looked at, the `PROBE` line spent before any operator text, the responder allowlist, the one-way `off` latch announced in chat.
- **The gateway owns routing leaks** and must refuse **pre-dispatch** — which the caller structurally cannot do, because it is asking a black box where it will send the next request and the box only answers afterwards.

**The summarizer stays strictly off the delivery path.** Ask goes out first with the agent's own words; the summary is added by editing that message with the keyboard put back on. A wedged gateway costs a summary and never an ask. That fix already exists here; keep it.

**One correction to the record.** `summarize.rs` already defaults `task_class` to `autocomplete` (line 490-493) — the only chain with no hosted member. The guard is right. Its test name (`no_task_class_is_sent_unless_the_operator_asks_for_one`) asserts the opposite of what it says. Rename it.

**Ask llm-gateway for, on its own tracker, not blocking here:** `allowed_providers` deny-by-default and `allow_fallthrough: false` in `KeyConfig`, enforced in `resolveChain` as a 422 in the shape the vision gate already established. A monthly cap on every project key — today only opencode and sandbox have one, so a wedged bridge looping against a hosted chain has unlimited spend. And fix `allowed_task_classes: []` meaning **ALL** — a fail-open default in the one system where everything else fails closed.

Do **not** flip `routing.default` globally. opencode runs on it with a $20 cap.

---

## 8. The trust boundary, as rules

**Two places, one agent, no race.** The operator works at his laptop in herdr's TUI and from his
phone at the same time — that is the product, not an edge case. Today it is the catastrophe: review
blocker #4 was exactly this shape, a relative key move computed from a selection that moved
underneath it, and the bridge reporting "Reject" after confirming "Allow once". The hub does not type.
A reply arrives as a message in the agent's own turn, so the TUI and the phone stop being two writers
fighting over one keyboard. The two-writer race is not mitigated here; it has no mechanism.

**Inbound Telegram content SELECTS. It never NAMES.**

You may pick from a list the machine already knows. You may never supply a path, a repo, a project id, a command, a model, or an allowlist entry. `callback_data` is 64 bytes and carries an opaque id, never a decision.

**An inbound message MAY:**
- be relayed verbatim, once, as an opaque payload, to exactly one connected project — the one whose topic you typed in
- resolve a tap into an `option_id` the bridge itself minted, answered against the record written down beside the message, refused when nothing is written down
- trigger the hub's closed command set (`/status`, `/projects`, `/help`)
- get a refusal posted in the topic

**An inbound message MAY NOT, ever:**
- enrol a project, mint a token, or edit the chat allowlist — admission is terminal-only
- start, stop, or refresh a session — `--auto` stays argv-only at the terminal, and a refresh costs a worker its in-flight context
- change any hub configuration, including the forum id or the summarizer's remote escape hatch
- reach a project that is not connected — dropped, visibly, in the topic, never queued
- reach a model — `Summarizer::one_line` keeps exactly one call site, agent→operator only, machine-checked
- reach a shell — **structurally impossible now.** A topic routing into a pane whose agent exited, with Enter making the *shell* run your reply, has no mechanism when the far end is a socket
- move a credential — the harness already hard-denies this even with your explicit consent; the hub adds no path around it and stays off Mission Control's write side

**Standing rules that do not change:**
- The chat allowlist runs **first**, before command parsing, before any state is touched. Empty, missing or unparseable answers **nobody**. A rejected chat gets silence, not a refusal. Under a hub this is the *only* scope boundary, because one-bot-per-workspace no longer supplies one.
- The token comes from the environment only. A token-shaped key in a TOML is a hard startup error naming `/revoke`.
- Every agent-authored string reaching Telegram passes `render::escape_html`. One function. Every path.
- Nothing is sent without a record of it first. Every branch that sends nothing writes `refused`, so a dangling `sent` means the process died mid-write and nothing else.
- Persisted state survives a kill at any instant: atomic temp+rename, 0600, corrupt file starts empty rather than refusing to boot.

**On the socket side:** authority flows one way. The hub decides where a bridge's words go. A bridge decides nothing about the hub. A frame carries no addressing at all — the hub knows which connection is which project. A bridge that names a project gets `fatal`.

---

## 9. First slice

**One project, one topic, one ask, one tap, and the answer lands in a live worker's turn.**

Nothing is deleted. `permission.rs`, `deliver.rs`, `mirror.rs` and `herdr-client` all stay on disk. The other ten orgs keep running on their own tokens. Cost if it fails: ~700 lines and a day.

**In scope:**
1. `crates/hub-proto` — frames and NDJSON transport, salvaged.
2. `lock.rs` — flock on the state dir, pidfile naming the holder, **taken before the `Bot` is constructed**. Plus the runtime 409 counter: three consecutive conflicts stops the dispatcher, stamps the heartbeat `conflicted`, alarms.
3. `hub.rs` — listener, `SO_PEERCRED`, token registry, single claim, `ping`/`pong`.
4. `herdr-tg enroll <repo>` — terminal-only, mints the token.
5. `queue.rs` — token bucket, `retry_after`, `fit()` on every path, ack every frame.
6. Topic bind on project id, greet on create, rebind on `message thread not found`.
7. `kickoff-channel` plugin — MCP stdio, `reply` tool, `notifications/claude/channel`, socket client. SDK pinned to the official plugin's version (the `skip:era` arm rejects a "modern" protocol revision).
8. `heartbeat.rs` + the watchdog timer unit.
9. managed-settings written, **one** repo flipped: herdr-tg itself.

**The test — `an_ask_becomes_a_tap_becomes_a_choice`.** Real `UnixListener`, fake bridge, Telegram behind a trait:

1. `hello` with a valid token → `welcome`; exactly one `createForumTopic` with the title **from the registry**, not from anything the bridge sent (the test sets a wire field impersonating another project); exactly one greeting.
2. `ask{ask_id:"a1", options:[y,n]}` → exactly one send, right `(chat, topic)`, agent's words verbatim, escaped, under budget, two opaque `callback_data`, a `PromptRecord` on disk carrying the labels.
3. An `audit` `sent` record precedes the send; `outcome` follows.
4. Tap `y` from an allowed chat → the bridge receives `choice{ask_id:"a1", option_id:"y"}`, label resolved from the record.
5. The **same** tap from a **different** chat → nothing sent, nothing received.

**Three supporting tests, each pinning a past failure:**

- `a_second_hub_never_reaches_the_token` — start a second hub against the same state dir. It exits non-zero **before any `Bot` is constructed**, naming the holding pid. Boring on purpose: today the same situation gives you two live processes, an infinite 409 backoff, a deaf worker, and a 198-line `/proc` walk built to notice.
- `a_burst_never_exceeds_the_chat_budget` — six projects, forty `say` frames each, ten seconds. Sends stay under 18/min, **every frame acked exactly once**, zero unaccounted for, shedding produced a message not a log line.
- `a_dm_reply_to_cannot_reach_a_forum_topic` — the existing test, re-aimed.

**Two manual proofs that are not optional:**

- **The round trip.** Type one sentence in herdr-tg's topic from your phone. Watch the coordinator answer. A registered channel and a rejected one look identical from the process table — only a round trip distinguishes them.
- **Fire the watchdog on purpose.** Kill the hub. Confirm your phone buzzes. A watchdog that has never fired is a watchdog you are guessing about.

---

## 10. The slices after it

1. **Round trip** — above. One project, nothing deleted.
2. **The deletion.** `permission.rs`, `deliver.rs`, `mirror.rs`, `cmd/`, `herdr-client`, the proof scripts. Re-aim `no_live_write_call_site.rs`. Only after slice 1 has run a week.
3. **The fleet.** Migrate the other orgs, one at a time. `ask_resolved`, the General picker, the `/status` digest, permission relay (`permission_request` → `ask`, `choice` → `permission`).
4. **Liveness transfer.** The hub's own deaf-worker alarm, proven against a deliberately broken bridge. Only then shrink kickoff's belt. Only then delete `bridge-reap.sh` and the per-project tokens — and only after the alarms have a proven new home.
5. **The opencode bridge.** Its SSE stream is ~24 typed callbacks and is the best structured source on the box. Second client of the same frames.
6. **`/new`** — start a project from your phone. Reads the adopters registry, offers repos as buttons, runs `kickoff up --detach`. Never `--auto`. Last, because it is the only feature that touches the autonomy grant.

---

## 11. Not in v1

- **Flat mode.** No `/target`, `/panes`, sticky, or the switcher. The forum is mandatory. The DM keeps `/status` and nothing else. Aiming should be a tap you never make.
- **The transcript.** No mirror, no live relay. Telegram carries asks, decisions and completions. At the old 4-second tick that was 15 msg/min per project — over the shared 20/min ceiling at **two** projects. If you want the running transcript it is a Tailscale page.
- **Telegram visibility of panes that are not kickoff projects.** After this, a thing is visible in the FORUM if and only if it is a kickoff project with a bridge. Keeping the protocol alive for one command costs ~5,300 lines.

  **This is not losing herdr, and an earlier draft of this line read as though it were.** Slice 2 deletes `herdr-client` — our client for its protocol — not herdr, which stays the operator's TUI running his panes and agents exactly as now. He steers from the terminal through herdr and from his phone through the hub. That is the intended shape, not a casualty of it.
- **Absorbing kickoff's alarms.** `announce_restart` and `tg_send_tokenless` keep their own tokens and their own curl. They are the only channel that works when the worker — or the hub — is broken.
- **Deleting `bridge-reap.sh` or any per-project token** before slice 4.
- **Private-chat topics.** Strictly the better shape — no supergroup, no admin grant, no General ambiguity. Blocked: pinned teloxide 0.17.0 targets Bot API 9.1, nothing newer on crates.io, so it means raw HTTP alongside teloxide. The surface is ~8 months old and already regressed once in production. Revisit when teloxide catches up.
- **Files, images, voice, attachments.** Text only.
- **Multi-operator.** One allowlist, one human.
- **Topic pinning, per-project cost lines, Mission Control writes.**

---

## 12. The three risks, and the cheapest probe for each

**Risk 1 — the channel gate is vendor-controlled and fails silently.**
`allowedChannelPlugins` can be replaced by a remote statsig ledger. A rejected channel boots and exits at ~0.1 s, leaving the worker reading into the void, looking alive. One policy change could deafen eleven projects at once, and `bridge_present` would agree they were healthy.

*Probe, today, 20 minutes:* install `kickoff-channel` in `kickoff-local`, write the managed-settings file, flip herdr-tg's `CHANNEL_SPEC`, refresh, type one sentence in the topic. If the coordinator answers, the door works. If it does not, you have spent an afternoon and permission.rs is untouched.

*Mitigation in the design:* a project is **live** only after `hello` plus five seconds plus one `pong`. Under that window: no topic, no greeting, one line in General — "*its worker connected and vanished; it is probably not allowed to talk to me.*" That window is an inference from a recorded symptom — verify it live with a deliberately unallowlisted build before anything rests on it.

**Risk 2 — one bot is now a fleet-wide single point of failure, and its death is silence.**
Absence is the hardest state to notice from a pocket, and it is this system's signature failure: the deaf worker, the 5,164-char message dropped, the mirror producing zero relays for 116 seconds.

*Probe, 15 minutes:* build the watchdog first, before the hub is interesting. `systemctl --user stop herdr-tg`, wait, confirm your phone buzzes. Fifteen lines of shell, load-bearing out of all proportion. The real risk is that it gets skipped as not the interesting part.

**Risk 3 — the 20/min ceiling is per-chat, and I have designed around an assumption.**
Topics grant no quota of their own. teloxide's throttle has no thread dimension at all and defaults to 10/min for a supergroup. If it is per-thread, the fairness machinery is over-engineering; if it is per-chat, it is the difference between working at six projects and not.

*Probe, 5 minutes, no code:* fire 40 messages across 4 topics in 60 seconds with `curl`. Watch for a 429 and read `retry_after`. While you are there, check whether `editMessageText` is charged against the same budget — that decides whether coalescing happens before or at the API call.

---

## 13. What survives of five rounds, and what was spent

**Honest accounting. herdr-tg is 13,298 lines and 234 tests today.**

**Survives — about two-thirds, ~8,500 lines and ~150 tests.** And it survives because none of it was ever about panes:

- The **egress proof** — 2,265 lines, 34 tests. The loopback proof by real URL parse, the DNS pin, the peer address read back off the connection, the probe spent before your text, the one-way trip-off announced in chat. Whole, untouched, and the single largest asset in the repo.
- The **allowlist** — twelve lines that now carry the entire scope boundary.
- **Routing** — 982 lines, 22 tests. Already chat-scoped, already storing a bare string. Four call sites change.
- The **tap ledger** — `PromptRecord` plus `resolve_tap`. The mechanism that stopped a button reading "Reject" from confirming "Allow always". Kept, and its evidence upgraded from a heuristic to a fact.
- The **eight voice rules** and the tests that enforce them, including the character-for-character rule that a summary may never stand in for what was said.
- The **audit discipline** — sent-before, `refused`/`failed`/`unseen`, nothing dangles.
- The **anti-spam half** of notify — persisted dedupe, spawned debounce, a callback instead of a Bot.
- The **guard machinery** in `no_live_write_call_site.rs` — 2,706 lines that have caught five real bypasses. Re-aimed, not deleted.
- The **credential discipline**, the **hardened unit**, the **de-identification policy**, `escape_html`, `fit`, the blocked-first phone layout.

**Spent — ~10,100 lines and 153 tests.**

- `permission.rs`, 1,754 lines and 36 tests, is spent. It was excellent work against an ill-posed problem: reconstructing structure from bytes after the structure had been thrown away. Its own header already admits the case it cannot see.
- `deliver.rs`, 2,033 lines and 28 tests, is spent. The four-rung ladder existed because an `ok` meant nothing. Its **principle** — never claim a rung you did not observe — is the spine of `ack{yes|no|unseen}`, in four lines instead of two thousand.
- `mirror.rs`, 296 lines, is spent. Its epitaph is its own review finding: 116 seconds of a real working agent producing zero relays.
- The whole `herdr-client` crate and its protocol suite, ~5,350 lines and 71 tests, is spent as a runtime dependency. Its socket **reasoning** is not — the newline invariant, read-exactly-one-line, the per-frame `Take` restore and `opaque_id!` all move into the hub, and each of them closes a failure that cost a real debugging session.

**What the five rounds actually bought.** They did not build the write path — they proved it could not be built, which is the finding that makes this design possible. Six-to-one against real captures, five independent reviews converging, and a machine-checked guard that caught five distinct bypasses of the rule that the write RPCs must have no live call site. Without that evidence, this redesign would be a preference. With it, it is a conclusion.

The read path is not spent at all. It is the product.