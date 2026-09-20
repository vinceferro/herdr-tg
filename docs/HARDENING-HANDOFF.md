<!-- HANDOFF, 8 September 2026. Ten commits, c6c8266 (exclusive) to 059fe5a. Every claim below was
     checked against the code or against a command's output on this box; where a commit message and
     the code disagree it is said so and the CODE is what is written down. -->

# The hardening brief — handoff

Ten commits, `c6c8266`..`059fe5a`. The wire gained four additive fields and words, all optional and
all absent from the bytes when unset; nothing an adapter written before them sends or reads changed.
The rest is the hub, the one adapter, the bridge, the installer and the docs.

Read §5 first if you are the Kickoff agent. Read §6 before you tell anybody this is proven.

---

## 1. What changed, per brief item

### Item 2 — a question answered at the terminal, and a tap on the phone, at the same moment (`13ea2d4`)

**The defect.** The order was wrong. When an adapter sent `ask_resolved{answered}`, the hub went to
Telegram first to take the keyboard off, and only the edit's *success* ended the question. A tap in
that window — a whole Telegram round trip on a menu he is looking at — resolved and delivered a
choice into a turn that had already moved on. If the edit failed, nothing was written down at all,
so the menu stayed live on his phone with the hub believing the question open.

**What holds now.** The ledger is written first. `AskLedger::close_all` marks every message of an ask
closed under one lock before any edit and hands back only the keyboards somebody still has to take
off (`crates/kickoff-channel/src/hub.rs:1018`). A tap after that is refused, in his words, by the single
predicate both the unlocked look and the lock-held re-check use — `AskRecord::refusal_if_closed`
(`hub.rs:614`): answered on either side reads as *already answered*, withdrawn or timed out reads as
*nobody is asking any more*. The record survives a failed edit and carries the note its keyboard must
be signed off with, so the next `ask_resolved` or the next session's arrival finishes the job with
the right words instead of writing "the session that asked this restarted" over an answered question.

Two more, in code this slice did not write: a refused tap is a toast alone and no longer also a
posted line (which cost one of the eighteen sends a minute on every retry), and an edit Telegram
answers *message is not modified* is read as a success.

**Storage, not wire.** `asks.json` gains an optional `closed` field, `#[serde(default)]`
(`hub.rs:591`). An older binary ignores it; a ledger written before it reads back with its questions
open.

### Item 3 — the operator's words go to the session a launcher named, or to nothing (`b5931ce`, shape converged in `b1d6c5a`)

**The defect, found live.** The watcher picked the session for a typed line by recency — the most
recently updated root session for the project directory. On 6 September, in a live kickoff steering
room, the only root session the server listed for that directory was an **unrestricted coordinator**,
while the room's own agent was a different session the launcher had pinned and nothing read. The next
line typed into that topic would have gone to the session that may edit files, run a shell and
commit — and both ends would have read it as success. The room was stopped; no such line was typed.

**What holds now.** `--opencode-binding-file` names a file the launcher owns. It is read at **every**
delivery and never cached, so a launcher that moves the worker is obeyed on the next line. One
validation function serves all four paths — a typed line, a question, a permission prompt and a tap —
which was the first draft's mistake: typed words were validated against the server while the tap
guard compared a raw id, so a question from a session the binding refuses was drawn on his phone and
his tap posted into it. Where a binding was given there is **no fallback**: the guess is unreachable,
and the refusal reaches him in words rather than as silence.

`--opencode-binding-generation <n>` is the floor that survives a restart. What a process remembers
about how far the launcher had got is exactly what a restart destroys, and a stale file outlives it,
so the number travels in the unit's environment file, which systemd re-reads on every start
(`deploy/kickoff-hub-attach@.service`, proved by `the_worker_unit_starts_the_command_it_says_it_starts.rs`).

The binding's exact shape and all ten checks are in §5 — that is the part another org must consume.

### Item 4 — green now means all three, and an agent nobody can name is not this room's (`0b8af38`, corrected in `059fe5a`)

**The defect, health half.** Answering Telegram was the whole of the heartbeat. A hub whose socket
had never bound answered `get_me` every forty-five seconds and stamped a green file while every agent
on the box talked to nobody.

**What holds now.** `heartbeat.rs` holds three facts apart — the phone line answering, a connection
coming out the far end of the accept loop, and the dispatcher still being handed his taps — and
`stamp_if` touches the file only when all three were true inside `FRESH_FOR` (90 s)
(`crates/kickoff-channel/src/heartbeat.rs:82`, `:303`, `:522`). Withholding is the only signal a script that
reads a modification time can hear, so the diagnosis goes in a **second** file, `hub.health`, rewritten
every tick (`heartbeat.rs:542`) — writing "degraded" into the stamp would refresh the very timestamp
the alarm watches. The watchdog reads that note **only** to word an alarm it has already decided to
raise. The third leg exists because the first two are not enough: a second copy of the bot holding the
update slot answers the update call with a conflict while both other legs stay true, and every tap
dies in silence. A single refusal is a blip; a run of them sustained across the window is a fault.

The boot hole found afterwards: a hub that could not reach Telegram at boot propagated the error
before anything built the health machinery, so nothing ever stamped or wrote a note and the watchdog —
which arms itself the first time something does — never armed. The health and the heartbeat are now
constructed **before** the first fallible call, and the note is written on the way in and on the way
out (`crates/kickoff-channel/src/bot.rs:732`, `:756`, `:789`). The stamp was not weakened: an unreachable
phone line still withholds it. `deploy/herdr-tg.service` gained the comment saying that `Restart=always`
is what keeps that note young enough for the alarm to quote.

`herdr-tg doctor` reported an armed watchdog on any box with a hub on it, because it inferred the
answer from the hub's two files and read none of the four the watchdog writes. It reads them now and
reports what it **observed** apart from what it **infers** (`crates/kickoff-channel/src/cmd/doctor.rs:40`, `:203`, `:311`).

**The defect, fence half.** The reverse-direction agent check was fail-open on the case that matters.
It withheld a question only when the running agent was known **and** different, while the lookup
answered "not known" for four separate reasons — the event named no message, the lookup failed, it
timed out, or the message named no agent. A question whose authority could not be established was
shown under this room's name.

**What holds now.** The two kinds of unknown are told apart
(`adapters/kickoff-hub-attach/opencode.ts:496`). *Could not find out yet* is transient and goes into
the mechanism this file already had — kept, bounded in count and in time, offered again when the
answer arrives. *Can never be known* is withheld at once. In both cases the worker is **turned down**
rather than left waiting on a keyboard nobody will draw (`sayWhatBecameOfIt`, `opencode.ts:627`), and
the operator is told once. `059fe5a` is the correction that made the earlier commit message true: a
permission request names no message to look up, so on a wall whose binding names an agent it lands
every time on the one path that withheld it and released nothing — its tool call waited for ever. A
permission is now rejected through the same endpoint a tap answers one with (`turnDown`,
`opencode.ts:562`), and the operator's line is keyed on the **outcome** as well as the reason, so a
second withheld permission in one spell is not swallowed.

### Item 5 — an install that reaches the box (`9d88eb3`)

**The defect, measured before a line was written.** The marketplace cache is keyed by the plugin's
version and the installer only ever ran the *install* verb, which is a no-op for a version already
present. On this box the installed copy was six days old, 15 262 bytes, with **no `hub-link.ts` in it
at all**, while the source beside it was 74 665 bytes. Both said `0.1.0`. That copy stamps no lease
and promises no confirmation — so on the operator's own sessions, silently, for a week, every tap read
as *Sent* for ever and the run-replaced fence never engaged.

**What holds now.** `the_channel_plugins_version_moves_with_its_content`
(`crates/kickoff-channel/tests/the_channel_plugin_install_is_reproducible.rs:142`) walks back to the oldest
commit still carrying the working tree's version and compares that commit's bytes with the tree's, so
a change without a bump turns the whole workspace red and a bump not yet committed is not punished. A
hand-bumped number was chosen over a content hash because the version is also what the tool lists,
what the box records, and the only thing that can answer "is the copy here **older**". The plugin is
now `0.2.1` in both `.claude-plugin/plugin.json` and `package.json`, and a third test pins that the
bridge announces the version its manifests carry.

`scripts/install-channel-plugin.sh` fails loud: no swallowed exit codes, and after installing it
compares what landed against the source file by file — and says what it compared, because the copy on
the box also holds dependencies this repository does not track. What it put there is written to
`plugin.installed` in the hub's state directory, outside every repository.

### Item 6 — public files that describe what ships (`9d88eb3`)

**The defect.** `README.md` spent eleven days describing a screen-reading router with a sticky pane
and a terminal mirror — a product that was deleted. `TRACKER.md` spent six days telling a reader the
live round trip to a phone had not happened; it had, the evening the tracker was last written.

**What holds now.** Both rewritten, and
`crates/kickoff-channel/tests/the_public_docs_describe_the_product_that_ships.rs` (9 tests) fails if either
drifts back. The rule it keeps is the one that matters: **a number in a public file is checked against
the thing that produces it, never against a second copy of itself.** The command list is read out of
`main.rs`, the rates out of `queue.rs`, and no number above Telegram's measured twenty may stand beside
"a minute" anywhere in the README — written as a word or as a digit.

### The north-star additions

**The transport seam (`ea52545`).** The north star asks for the transport behind an interface —
authenticated `AF_UNIX` today, a mutually authenticated remote stream later, identical semantics.
None of that is buildable while the semantics layer takes a `UnixStream` and reads `SO_PEERCRED`
itself. `crates/kickoff-channel/src/transport.rs` (new, 537 lines) is now the only file in the bot that knows
what a socket and a peer uid are; it hands the hub an `Accepted` — a byte stream and a
`ConnectionIdentity`. Everything moved rather than being rewritten. Two things fall out of naming the
identity: a peer that does not share this machine's filesystem is offered no outbox (bytes cross by
mount, and a mount is a property of *this* transport), and a connection the kernel will not name is
dropped where it is accepted rather than reaching the loop's backoff, where one unnameable peer could
starve every bridge behind it. The seam is asserted in **both** directions by
`tests/the_hub_does_not_know_what_a_socket_is.rs` (15 tests), which fails closed on every way a scan
can come up empty; the duplex harness runs the same test body over the socket and over an in-memory
pipe. Nothing on the wire changed.

**Identity and generation (`0d5bab1`, `b1d6c5a`, `309756f`).** A run of an address now holds a numbered
lease. The hub mints it under the claim lock, stamps it on the `welcome` that grants it and on
everything it sends afterwards; a bridge stamps it back on everything it sends, the redialling `hello`
included. An older run cannot reclaim, cannot deliver, cannot speak through the drain after its claim
is gone, and cannot release a claim that is no longer its; a dead incumbent is **kicked** rather than
silently overwritten. And a bridge that promises `confirms: ["choice"]` answers every tap, so the line
on his phone is edited from `Sent: X` to `Taken: X`, or to why it was not taken. The wire half is §2;
the duties are §5.

### Also in the range — three settled questions (`30a9624`)

`docs/HUB-AND-KICKOFF.md` was a sketch with two open questions. Both are now answered in the file, and
so is a third that had sat unread in the inbox since 3 September:

* **Restart and re-grounding are kickoff's exclusively.** The hub carries the operator's request as an
  input and never owns the killing or the respawning. One line: we own conversation routing, they own
  the birth, death and rebirth of every session on the channel.
* **Q1 — a topic per lane**, answered by the operator on 2 September and built.
* **Q2 — the hub does not ask systemd to start units**, answered by the architecture rather than by
  taste. What starts a process is a **launcher**, which is another adapter: it offers what it may start
  and acts on the choice itself. Lifecycle intentions may travel as typed operations against specs
  declared in advance — never as an image, a command, a mount, a secret, an environment variable or a
  host path from a phone.

The file's header now says the rest of it is history: it predates the conversation model, the
exact-session binding and the transport seam, and where it disagrees with `docs/ATTACHING.md` the
contract wins.

---

## 2. Protocol changes — exhaustive

Everything below is `crates/hub-proto/src/frame.rs`, verified against the file rather than against a
commit message. **Four additions. All additive. All absent from the bytes when unset.** Wire version
is unchanged (`VERSION` = 1); the document's own version moved to v22.

| # | Where | Field / variant | Rust | Wire |
| --- | --- | --- | --- | --- |
| 1 | `Envelope<P>`, both directions, every frame | `generation` | `Option<u64>` (`frame.rs:84`) | `"generation": <number>`, omitted when absent |
| 2 | `BridgeFrame::Hello` | `confirms` | `Option<Vec<String>>` (`frame.rs:464`) | `"confirms": ["choice"]`, omitted when it promises nothing |
| 3 | `AckWhy` | `StaleGeneration` (`frame.rs:224`) | kebab-case enum (`frame.rs:185`) | `"why": "stale-generation"` |
| 4 | `RefusedReason` | `StaleGeneration` (`frame.rs:267`) | snake_case enum (`frame.rs:229`) | `"reason": "stale_generation"` |

Note the spelling asymmetry, which is the enums' pre-existing `rename_all` and not a slip: the
**ack's** why is `stale-generation` with a hyphen, the **refusal's** reason is `stale_generation` with
an underscore. Both are pinned as literal bytes by tests (`frame.rs:1452`, `:1470`).

Also exported from the crate (`crates/hub-proto/src/lib.rs:31`):

* `MAX_GENERATION: u64 = (1 << 53) - 1` (`frame.rs:123`). Every bridge that exists reads frames with
  `JSON.parse`, which has no integers; past this a number comes back as the nearest a double can hold
  and the bridge stamps a generation the hub never minted, with no wrong-looking value anywhere.
* `promises_to_confirm(&Option<Vec<String>>, &str) -> bool` (`frame.rs:148`). The **only** way to read
  `confirms`. A reader that asks whether the promise was *present* rather than whether it *names this
  frame* holds a tap open for a bridge that promised something else.
* `Envelope::with_generation(u64)` (`frame.rs:108`), a builder rather than a second constructor,
  because fifty call sites hold no generation and a `None` passed by hand at fifty sites is a `Some`
  waiting to be pasted into the wrong one.

**Two zero-value rules, both enforced in the codec:**

* **`generation: 0` is read as absent and never written.** `a_zero_is_no_generation` on the way in
  (`frame.rs:126`), `holds_no_generation` on the way out (`frame.rs:137`), and `with_generation`
  filters it too. Reason: `welcome.generation ?? 0` is what a bridge written against the document in
  the language every bridge is written in puts on the wire before it has been welcomed, and a zero
  that reached a fence would lose every comparison it was ever in — fencing that run for ever.
* **`confirms: []` is read as absent and never written.** `an_empty_promise_is_no_promise`
  (`frame.rs:153`), `promises_nothing` (`frame.rs:161`). An adapter that builds the list by filtering
  emits `[]` every time it promises nothing, and two spellings of one meaning is how a reader branches
  on the wrong one.

**What an adapter written before this sees.** Nothing. A peer that stamps no generation and promises
nothing puts **byte for byte** what it always put on the wire — pinned as bytes, not as a round trip,
on the `hello` and on the `welcome` (`frame.rs:1512`, `:1603`), because a round trip stays green when
a `"confirms": null` has appeared and that would make an older hub tolerate a field for no reason on
the one frame whose failure is a project that can never connect. In the other direction, a frame a
newer peer *stamped* still parses on a build that has never heard of generations
(`frame.rs:1491`) — which matters because every frame in both directions now carries the field, so a
build that rejected it would go deaf on the first ping.

**The one shape rule a stranger must know.**

> **A payload field may not share a name with an envelope field.** Concretely: **no payload field in
> either direction may be named `generation`.**

`Envelope<P>` carries its payload with `#[serde(flatten)]`, so a frame is one flat JSON object. A
payload `generation` and the envelope's `generation` are therefore the same key: a peer that set both
would emit a **duplicate key**, which serde refuses to read, and a peer that set only the payload's
would have it **silently swallowed** by the envelope's and read back as nothing. This protocol met the
collision once already, when `ping` and `pong` tried to name their nonce `id`. This is why the
`welcome` that *grants* a lease carries it on its own envelope and has no payload field for it — the
absence is deliberate and commented in place (`frame.rs:604`). The rule is held by
`a_generation_rides_on_every_frame_in_both_directions_and_is_named_exactly_once` (`frame.rs:1780`),
which fails the day one is added.

**Two duties this crate cannot enforce**, written on both new words: the hub may send
`stale_generation` / `stale-generation` **only to a connection that has stamped a generation**. A
bridge old enough not to know the word renders an unknown `why` as "his phone did not take it" and an
unknown `refused` reason as *temporary* (§8 rule 8, a deliberate default in both refusal tables written
against this protocol) — so sending it to one tells an agent that the operator's messaging app refused
a frame his phone never saw, and starts a redial loop with nothing on the phone to say why. The hub
enforces it: `speaks_generations` is set by a connection's read loop the first time one of its frames
carries a generation (`hub.rs:1616`), and every send of either word is gated on it (`hub.rs:2724`,
`:5125`).

**Not on the wire.** `asks.json` gains an optional `closed` (item 2). The binding file of item 3 is a
local file between a launcher and one adapter, and **no frame carries a session id** — pinned by a
test that captures every frame of a run with the flag set.

---

## 3. Changed files

Generated from `git diff --stat c6c8266..HEAD`: **48 files, +21 846 / −855**.

**Hub and bot (`crates/kickoff-channel/`)**

| file | ± | what moved |
| --- | --- | --- |
| `src/hub.rs` | +1648 −296 | the lease (mint, reclaim fence, delivery fence, kick), the confirmed tap, `close_all` and the closed record |
| `src/hub/tests.rs` | +2975 −65 | the hub's own suite for all of the above |
| `src/bot.rs` | +1248 −82 | health before the first fallible call, the tap receipt's edits, reaction ledger paths |
| `src/heartbeat.rs` | +1008 −41 | three legs, `stamp_if`, `hub.health` and its reader |
| `src/cmd/doctor.rs` | +886 −23 | reads the watchdog's own four files; observed apart from inferred |
| `src/transport.rs` | +537 (new) | the socket, the peer uid, `Accepted`, `ConnectionIdentity`, `fence_is_alive` |
| `src/surface.rs` | +86 −5 | the operator-facing sentences for a taken / not-taken / unconfirmed tap |
| `Cargo.toml`, `src/main.rs`, `src/presence.rs` | +10 −2 | tokio features named explicitly; `mod transport`; `fence_is_alive` moved |

**Wire (`crates/hub-proto/`)** — `src/frame.rs` +731 −1, `src/lib.rs` +2 −2. Exactly §2 and its tests.

**Adapter (`adapters/kickoff-hub-attach/`)** — `opencode.ts` +1003 −32 (the binding, the agent fence,
the release, the deadlines), `plan.ts` +508 −3 (`readBindingFile` and the file-safety proofs),
`relay.ts` +248 −40 (strips a producer's lease, stops `stale_generation` at the door), `check.ts` +112,
`main.ts` +56, `README.md` +54. Tests: `test-against-fakes.ts` +1750, `test-check.ts` +306,
`test-harness.ts` +188, `test-what-breaks-it.ts` +354, `test-two-producers.ts` +37.

**Plugin (`plugins/kickoff-channel/`)** — `server.ts` +181 −12 (promises `confirms: ['choice']`, acks a
tap, stops dialling on `stale_generation`), `hub-link.ts` +85 −7 (remembers the lease, stamps it,
deletes any inherited one), `test-against-a-fake-hub.ts` +204, and the two manifests bumped (0.2.0 at the time; 0.2.1 since,
when the identity slice touched the plugin — which is the rule working).

**Deploy and scripts** — `deploy/herdr-tg-watchdog.sh` +212 −8, `deploy/kickoff-hub-attach@.service`
+33 −3, `deploy/herdr-tg.service` +5, `scripts/install-channel-plugin.sh` +486 −26,
`scripts/watchdog-selftest.sh` +397 (76 → 473 lines), `scripts/install-watchdog.sh` +71 −10,
`scripts/install-attach.sh` +3.

**Docs** — `docs/ATTACHING.md` +1081 −27 (v18 → v22; §13.10 new, §8 gains a thirteenth rule),
`docs/CAPABILITIES.md` +224 −12 (v14 → v20), `docs/INTERFACES.md` +52, `docs/HUB-AND-KICKOFF.md` +34 −5
(three questions answered, header now says the rest is history), `docs/examples/attach-from-the-document.ts`
+87 −14, plus `README.md`, `TRACKER.md`, `CLAUDE.md`, `adapters/kickoff-hub-attach/README.md`.

**New test files: five**, all under `crates/kickoff-channel/tests/`.

| file | tests | what it guards |
| --- | --- | --- |
| `the_hub_does_not_know_what_a_socket_is.rs` | 15 | the transport seam, both directions; fails closed on every way a scan can come up empty |
| `the_worker_unit_starts_the_command_it_says_it_starts.rs` | 22 | runs a real `/bin/sh` against the unit's `ExecStart` and asserts the argv attach receives |
| `the_heartbeat_is_earned_not_scheduled.rs` | 5 | holds the hub's `hub.health` sentences and the watchdog script's `case` patterns together, in both directions |
| `the_channel_plugin_install_is_reproducible.rs` | 36 | the version moves with the content; the installer verifies rather than announces |
| `the_public_docs_describe_the_product_that_ships.rs` | 9 | README and TRACKER against `main.rs` and `queue.rs`, never against a second copy of themselves |

---

## 4. Tests run, and what they prove

All run on this box on 8 September with
`env -u RUSTUP_TOOLCHAIN TMPDIR=/tmp/hverify PATH="$HOME/.cargo/bin:$PATH"`.

| gate | result |
| --- | --- |
| `cargo test --workspace` | **695 passed, 0 failed, 12 ignored**, across 21 targets |
| `cargo test -p kickoff-channel the_real_plugin -- --ignored` | **10 passed, 0 failed** |
| `cargo test -p kickoff-channel a_bridge_from_before_this_change -- --ignored` | **1 passed** (18.1 s) |
| `bun run test` in `adapters/kickoff-hub-attach/` | **321 checks, exit 0** — 54 + 36 + 143 + 63 + 25 across the five suites |
| `bun test-against-a-fake-hub.ts` in `plugins/kickoff-channel/` | **161 checks**, "all checks passed" |
| `bash scripts/watchdog-selftest.sh` | **pass=80 fail=0** |

695 matches the count `059fe5a` claims. The 12 `#[ignore]`d are the 10 `the_real_plugin*` tests, the
pre-change bridge test, and one proxy-driven child in `summarize.rs`.

**Compatibility evidence — three independent proofs, all green:**

1. **The pre-change bridge against the new hub.**
   `a_bridge_from_before_this_change_still_works_against_the_new_hub` (`hub/tests.rs:2944`) runs the
   bridge as it was before any of this against the hub as it is now. It stamps no lease and promises
   nothing, and it is welcomed, delivered to and acked exactly as before.
2. **The real bun plugin against the real hub over a real socket.**
   `the_real_plugin_and_the_real_hub_agree_on_the_wire` and nine siblings, both directions, only
   Telegram faked. `scripts/install-channel-plugin.sh` runs these and refuses to install a bridge that
   disagrees with the hub.
3. **A stranger's adapter, run against the real door.**
   `docs/examples/attach-from-the-document.ts` imports nothing of this repository and is written from
   the document alone. It **acks a choice and ignores the lease** — deliberately, because that is what
   most adapters will do. It is run twice by attach's own suites: as a producer at attach's door
   (`an_adapter_written_from_the_document_alone_attaches_and_is_heard`, beside the tool server and the
   watcher, with **one** claim at the hub), and as the "engine" attach starts with `--run` in a
   directory with no git (`a_stranger_written_from_the_document_still_attaches`). If that file ever has
   to change, the interface changed.

The wire-level absence pins are the ones worth naming: `a_hello_that_promises_to_confirm_nothing_is_byte_for_byte_the_hello_this_protocol_has_always_sent`
and `a_welcome_that_names_no_generation_is_byte_for_byte_the_welcome_this_protocol_has_always_sent`
assert **bytes**, not round trips.

---

## 5. What the Kickoff agent must consume

Six things. Each names the file that is the authority.

### 5.1 The binding file — exact shape

Authority: `docs/ATTACHING.md` §13.10. Reader: `adapters/kickoff-hub-attach/plan.ts:297`
(`readBindingFile`). **The names are the launcher's own, taken verbatim** — this repo asked for one
key, `version`, and renamed nothing else.

```json
{"version": 1, "conversation": "c-0123456789ab", "session_id": "ses_00000000000000000000theOne",
 "canonical_project_dir": "/srv/rooms/steering", "agent": "kickoff-room-steering",
 "generation": 7, "verified_at": "2026-09-07T08:00:00Z"}
```

`version` and `session_id` are **required**; the rest are optional. The key set is **closed** — a key
this reader does not know may be a *narrowing* of which session may be spoken to, and obeying the rest
while dropping it would deliver his words on a rule nobody checked. Leaving a key out means ABSENT:
`"conversation": ""` is not a conversation id and refuses the whole binding, so a recipe that
interpolates a room name unconditionally breaks every project that has no room.

**Every check, in order.** Before a byte is believed, about the **file** (`plan.ts:298`–`:353`): the
directory chain is safe (as given *and* as resolved — nobody else's, nobody-writable unless sticky, no
link an outsider can replace); it is not a symlink (`lstat`, then `open` with `O_NOFOLLOW`); it is a
regular file (a FIFO or a device would block this single-threaded process for ever); it is this user's;
mode `& 0o077 == 0`; at most 4096 bytes.

Then about the **contents** (`plan.ts:356`–`:449`), each refusal naming the part that stopped it: a
bare session id on a line is told apart from nonsense and told what to write instead; a note using the
names this reader wanted first (`v`, `session`, `directory`) gets the whole rename in one sentence; a
**duplicated key** is refused, because JSON keeps the last and a person reads the first; `version`
missing is its own sentence; `version` other than `1` is refused whole; an unknown key is refused;
`session_id` must match `ses_` + up to 60 of `[A-Za-z0-9]`; `conversation` must be a `p-`/`c-` id;
`canonical_project_dir` must be absolute; `agent` must be a non-empty string; `generation` a
non-negative safe integer. `verified_at` is known so it is not mistaken for a dropped rule, and read
into nothing.

Then, on **every delivery** (`opencode.ts:199` and `docs/ATTACHING.md` §13.10, steps 1–10):
1 the note is readable; 2 its `conversation` equals the conversation this attach is attached as —
refused, never ignored, when this side cannot say which conversation it is; 3 not below the floor and
not below what this run has already acted on; 4 `canonical_project_dir` against the directory attach
speaks for, before any request goes out (trailing slashes ignored; both sides resolved through symlinks
when the names differ); 5 the server is asked, once; 6 **the binding is read again** after the round
trip, and a line is refused rather than retargeted if it now names a different session; 7 in the
listing, not archived, not a subagent's, not another project's by the session's own word; 8 running the
agent the binding named — a session naming *no* agent is not a match either; 9 a reply under a question
this conversation still holds goes to the session that asked; 10 only then are the words posted.

### 5.2 The lease, and what a stale-generation refusal means

Authority: `docs/ATTACHING.md` §6, *The lease — which run of the address you are*.

The lease arrives on the **`welcome`'s own envelope** as `generation`, and nowhere else. Remember it,
stamp it on every frame you send afterwards including the `hello` you redial with, never mint one and
never invent one, and treat `0` as no lease.

> **`refused{stale_generation}` is PERMANENT for that run.** The way back is a **new run** — a fresh
> process, a new instance, dialling from nothing. **Never a redial.** The address has an incumbent that
> is not going away, so a bridge that retries spins until somebody kills it. **Put it in your permanent
> refusal set before you stamp your first lease** — both refusal tables written against this protocol
> so far treat an unknown reason as *temporary*, on purpose.

Two things that stop you being fenced by accident, both verified in `hub.rs`: on a redial **only a lease
behind the hub's number for the address is refused**; one *ahead* is admitted, and the hub mints past it
(`hub.rs:2674`, `Generations::mint` at `:1761`). And **the backlog you carry into a redial is safe** —
those frames were written before you could read the new `welcome`, so they carry the old number, and the
delivery fence refuses only a frame stamped **ahead** of the lease granted this connection
(`hub.rs:5100`).

Our own implementations of the permanent stop: `plugins/kickoff-channel/server.ts:1094` (stops dialling
and tells the agent to restart the session) and `adapters/kickoff-hub-attach/relay.ts:565` (stops the
whole run, engine and all).

### 5.3 The `confirms` declaration, and the duty to answer every choice

Authority: `docs/ATTACHING.md` §6, *Saying what became of his answer*; §8 rule 13.

Promise on your `hello`: `confirms: ["choice"]` — the hub's frames you will answer with an `ack`, named
by their `t`. **An empty list is no promise**, exactly as saying nothing is. The hub holds nothing open
waiting for an answer it was not promised, and a bridge that promised nothing is never nagged.

Having promised, answer **every** `choice` with `ack{ref, status, reason?}` where `ref` is the choice's
envelope id:

* `accepted` — **the answer is in the agent's turn.** Not "I read the frame", not "I handed it on".
* `refused` — it is not, and will not be later. `reason` is one short sentence in **his** words: no
  status code, no session id, no engine name, no component of yours.

What he sees, each an **edit** of the line he is already looking at (edits cost nothing against the
send ceiling): `accepted` → `Taken: <label>`; `refused` → `Not taken: <label> — <reason>. The agent has
not got your answer.`; promised and silent → after twenty seconds, once, `Sent: <label>. The session has
not confirmed it took your answer.`; promised nothing → `Sent: <label>`, never nagged. Answer **late
rather than not at all** — a minute-old answer still corrects the line. Your `ack` is a frame after
`hello`, so the hub acks it; that ack is bookkeeping, do not answer it.

### 5.4 A relay must strip a producer's lease

Authority: `docs/ATTACHING.md` §9, *What a relay does that a pipe does not*. Ours:
`adapters/kickoff-hub-attach/relay.ts:972`.

Strip `generation` from a producer's envelope where you already rewrite `v` and `id`, and **never
forward a producer's `hello`**. The two costs differ: on an ordinary frame, a producer's invented or
borrowed number is higher than the door's lease, the hub's delivery fence refuses it, and the agent
reads back that a newer run took its place — false, with no wrong-looking value anywhere. On a `hello`
it would raise the **address's floor**, and the project's real run would be locked out of its own
conversation on its next redial.

And **the word stops at the door.** `refused{stale_generation}` is a statement about the lease the door
holds; a producer holds none. Forwarded down, it is the one word a producer acts on for good — it stops
dialling for the life of its session, while the next run of the wall binds that same door seconds
later. **End their sockets instead**: a close is what this wire already means by "the door went away —
wait, and dial again".

### 5.5 The agent rides the delivery

Authority: `docs/ATTACHING.md` §13.10; `adapters/kickoff-hub-attach/opencode.ts:967` and `:984`.

Naming the session was never enough. A room's tree can ship an `opencode.json` whose default agent is
the org's coordinator, and the engine **re-resolves the agent when the prompt runs** — so a session
validated against the binding can still execute under something else.

* **Outbound**: every prompt names the binding's agent explicitly
  (`POST /session/{id}/prompt_async`, top-level `agent`, read off the running 1.18.25 server's own
  OpenAPI at `/doc`). Before the words are posted, the adapter asks whether the server resolves that
  agent at all — measured on 1.18.25: a prompt naming an unknown agent is answered **204 with nothing
  written**, so the words are simply gone and any ack would be a lie.
* **Inbound, symmetric**: a turn observed under an agent the binding does not name is **withheld** and
  the request turned down, not delivered. Where the agent cannot be established yet, the question is
  kept and retried; where it can never be established, it is withheld at once. In both cases the worker
  is released.

For a launcher this is one line: **if you write `agent` in the binding, the wall enforces it in both
directions; if you write `{version, session_id}` only, you get no turn-level check at all.** Both are
workable arrangements and they are not the same one.

### 5.6 The plugin version rule

Authority: `CLAUDE.md`, *Build and test*; the gate is
`the_channel_plugins_version_moves_with_its_content`.

> **If you touch `plugins/kickoff-channel/`, bump its version in the same commit** — in
> `.claude-plugin/plugin.json` **and** `package.json`.

The installed copy is cached by version. A change that does not move the number never reaches a box
that already has the plugin, silently. That is how a bridge from 1 September was still running on
7 September while the source had grown five times over. The whole workspace goes red until the number
moves. Current version: **0.2.1**. If you carry this bridge on another box, reinstall it — an older
copy stamps no lease and promises no confirmation, so every tap there reads *Sent* for ever and the
run-replaced fence never engages.

### 5.7 The hermetic fleet trial — run it before your Runner exists

Authority: `docs/ATTACHING.md` §13.12; the door is `bash scripts/fleet-trial.sh`.

Four conversations of one repo, live at once, against a **real** hub over a real Unix socket and
four **real** `kickoff-hub-attach` processes — with Telegram counted instead of called and one fake
engine instead of four opencodes. It needs no bot token, no opencode, and no share of a forum's
minute. Point a dispatcher at what it proves: each wall speaks only to the session its binding
names, a question from a session no binding names reaches nobody, a tap answers only the session
that asked, a line he types is routed from the topic he typed it in, and a run of an address that
has been replaced is refused `stale_generation` while its siblings keep their claims, their process
ids and their topics — while a wall that only lost its socket is let back in with the lease it
still holds, however many siblings restarted around it.

**One of its four rooms is deliberately paired off by one** — its note names its own session under
the conversation next door, which is what a dispatcher writes when it mints four rooms and pairs
them wrong. Both directions of §13.10 are proved on it: the question it cannot show draws no
keyboard and earns one plain line in its own topic, and a line typed at it is refused with the
reason under it. If your Runner writes bindings, this is the case to write a test against.

It also **measures the spend and prints it**: twelve of the seventeen an agent may have in a
trailing minute to open four conversations and speak in each, four more for the receipts under his
taps and the one line saying his refused words were not taken, one left. A refusal costs what a
delivery costs. Progress is deliberately out of it — one progress line per room is over the ceiling,
and the trial goes red if one appears. And then **the minute is gone**: a wall restarting inside the
same minute as four conversations opening is held, silent, for the rest of it.

This repo is integration support, not a fleet controller. The trial starts four fixed processes
once and stops: no scheduler, no spec resolution, no supervision, no health inference. What it
cannot prove is written at the top of the test itself — the real Bot API's refusals, real timing, a
real engine's event shapes, that an agent **acted**, the last hop to a phone, and the pid fence in
the hard case.

---

## 6. What is NOT proven

Stated plainly, because everything above reads better than the evidence.

1. **The live regression proved destination and body, not execution.** The 6 September measurement in
   the kickoff steering room was of the *listing*: which session the recency rule **would** have
   picked. No line was typed into the wrong session. Likewise the agent-rides-the-delivery fix came
   from kickoff's own green canary — green, because destination and body were right and the executing
   agent was not what anyone had checked.
2. **No execution proof has run in a real room.** Every check in §5.5 is proved against a fake opencode
   in `test-against-fakes.ts`, plus endpoint shapes measured against a real 1.18.25 server on
   7 September. Nothing has yet observed a real room's turn running under the bound agent because the
   prompt named it.
3. **A sourceless permission is withheld on an agent-naming wall.** Measured from the engine's own
   schema: `PermissionV2Asked` carries its message id inside an **optional** `source`, present only
   when the request comes from a tool call. A permission that names no tool carries nothing to look up,
   so on a wall whose binding names an agent it is withheld **every time**. That is the intended
   direction and it is a real, recurring cost — not an edge. Since `059fe5a` the worker is at least
   released rather than blocked for ever, and the operator is told once per outcome. The next org to
   adopt a binding that names an agent is the one that will meet this.
4. **The hub's half of the tap ordering is only that no second tap resolves.** The hub marks the record
   and delivers the choice in **two steps**, so a `choice` for an ask you have already resolved can
   still arrive. `docs/ATTACHING.md` asks the adapter to refuse it and says so plainly. The absolute
   ordering guarantee is deliberately **not published**; making that sentence true is a later slice.
5. **The pid-namespace trap is open.** `SO_PEERCRED` reports pid **0** for a peer outside the reading
   process's pid namespace. A connection the kernel will not name is now refused at accept
   (`transport.rs:303`), which is the fail-closed half; but the hub still asks `/proc` whether an
   incumbent is alive (`fence_is_alive(old.pid)`, `hub.rs:2693`), so a hub inside a namespace its
   bridges are outside of would still evict the fleet. Until that is replaced, the hub and its bridges
   **must share a pid namespace**. The module's own note says the repair "lands in a later slice"; the
   kick half of it has since landed, the `/proc` half has not, so read that comment as half-stale.
6. **CLOSED since this document was written.** It read: the run-number file is written inside the
   claim lock, so a full disk or a slow filesystem stalls delivery for every address. It was named as
   an open risk in `b1d6c5a` and not closed by `309756f`. Both that write and the presence write have
   since moved off the lock — the number and the snapshot are still decided under it, because two
   claims microseconds apart must not be handed the same lease and a snapshot must be consistent, but
   the writing happens on a blocking thread outside it, awaited, its error said. Presence needed more
   than the run-number half: the writer never touches the claims lock at all, and a sequence minted
   under it makes a later snapshot always win, so a slow write cannot put a list the hub has moved
   past back on disk. A failed write still removes the file rather than leaving a stale one carrying
   this hub's pid, which a reader would believe.
7. **No leg of the heartbeat proves a tap was acted on.** A handler that took one and hung looks green
   until the wedge backs up far enough to stop the stream being driven. `heartbeat.rs` says so rather
   than implying it. The alarm still restarts nothing; that belongs to whoever dispatches.
8. **A producer that attaches after the door's `hello` is covered by a promise it did not make.**
   `confirms` is the **connection's**, made once for the whole door, while producers attach and detach
   behind it. A door that promises `["choice"]` earns the "has not confirmed" line for a tap a
   late-attaching producer did take; a door that promises nothing gets pre-v22 silence for every
   producer, including ones that would have answered. Written down in §9 as a **cost**, not solved.
9. **A door only answers for a tap when it knows.** Where the producer holding a tap dies with the
   frame in its hand, the door says nothing at all, and the hub's twenty-second "not confirmed" line
   stands. That line is therefore not evidence the answer was lost.
10. **Two mutations survived the item 5/6 gate.** `9d88eb3` reports 49 mutations with 47 killed and
    says the two survivors are "named in the gate's report". **That report is not in the tree** — no
    file under `scripts/`, `crates/` or `docs/` names them, so a reader here cannot look them up. Treat
    the install and public-docs gates as 47/49 with two unidentified holes.
11. **The `--check` verb cannot ask the server whether a session is open**, by design: the ordinary
    order of starting a wall is *check, start, write the binding*. A binding not written yet is an `ok`
    line, not a `NOT`. So `--check` passing says nothing about whether the bound session exists.
12. **A watchdog installed before this change will not arm on a box that never stamped.** Arming on the
    stamp alone kept exactly the boxes this change is about silent for their whole lives. Such a box
    needs the copy from `scripts/install-watchdog.sh`.

### Where a commit message and the code disagree

* **`059fe5a` says so itself**, and it is right: `0b8af38`'s claim that a withheld question **and** a
  withheld permission both leave the worker turned down was true of one branch and false of the branch
  a permission actually takes. The code now matches the claim.
* **`9d88eb3` says a new conversation "costs two sends before its agent speaks"; the shipped README
  says three turns** — the topic, the greeting, then the message — and derives ~6 conversations a
  minute as 18 ÷ 3 (`README.md:53`). The README is the file under test. Two sends would give nine a
  minute, so the commit message's arithmetic does not reach its own number; read the README.
* **`frame.rs:118` describes the mint as `max(latest + 1, now_ms)`.** The code is
  `max(latest, arriving_run's_own_lease).saturating_add(1).max(now_millis()).min(MAX_GENERATION)`
  (`hub.rs:1761`) — it also mints past a number the *arriving* run claims, deliberately, so an admitted
  run is never fenced by its own backlog. The doc comment is a simplification, not a contradiction.
* **`ea52545` says the pid repair "lands in a later slice"** and `transport.rs:40` still says so; the
  kick half landed in `b1d6c5a`, the `/proc` half did not. See item 5 above.
