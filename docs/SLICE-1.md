# Slice 1 — herdr-client crate (revised build spec)

> Target repo: `$HOME/Projects/herdr-tg` · branch `main` · supersedes the slice-1 row of `PLAN.md:143`.
> Every fact below was re-verified live against **herdr 0.8.2 / protocol 20** on thev-lap on 2026-08-28,
> read-only, and every crate version against crates.io the same day. Where this document and
> `HERDR_API.md` (protocol 16) disagree, this document wins.

---

## What changed from PLAN.md, and why

The replan delta. Read this section to decide; the rest is execution.

### The proof cell is unexecutable as written — this is the headline

| # | PLAN.md says | Actually true | Evidence (run tonight) |
|---|---|---|---|
| 1 | proof = `herdr-tg status` "diffed against `herdr status` output" | `herdr status` prints **client/server version metadata only** — zero herd content. There is nothing to diff. | `herdr status` → `client:{version,channel,protocol}` `server:{status,version,protocol,compatible,socket}` `update:{restart_needed}`. No workspace/pane/agent key exists in the text or `--json` form. |
| 2 | — | The correct reference is **`herdr api snapshot`** — a thin CLI wrapper over the same `session.snapshot` RPC, making the proof a genuine two-independent-decoders equivalence test. | `diff <(jq -S .result.snapshot raw.json) <(jq -S .result.snapshot cli.json)` → **empty**. 9442 B socket vs 9456 B CLI; the 14-byte delta is the envelope `id` string. |
| 3 | — | The replacement proof, as previously drafted, **cannot tell a Rust client from a two-line shell script**. A `#!/bin/sh` + `herdr api snapshot` cheat passes a naive status diff. | Closed by running the client `env -i HOME=$HOME PATH=<empty dir>`: verified a child resolving `herdr` from PATH exits **127**, while a real Rust binary at an absolute path reaches the socket and exits 0. |
| 4 | slice-1 scope includes "handshake" | **There is no handshake/hello/connect/initialize method in protocol 20.** A one-shot RPC works with zero preamble. | grep for `hello\|handshake\|connect\|initialize` over the 255,484-byte schema → 0 hits. Re-worded as: `ping` + assert `protocol >= 20`. |
| 5 | — | `ping` **is** a real capability handshake and is not in `HERDR_API.md` at all. | `{"id":"p","method":"ping","params":{}}` → `{"type":"pong","version":"0.8.2","protocol":20,"capabilities":{"live_handoff":true,"detached_server_daemon":true}}` |

### The event model — the single highest-value correction

| # | The trap | Verified |
|---|---|---|
| 6 | **The stream multiplexes two incompatible envelope encodings on one connection.** Lifecycle frames are snake_case with a redundant `data.type`; the product's one load-bearing event arrives **dot-form with no `type` in `data` at all**. A `#[serde(tag="type")]` model over `data` — what `HERDR_API.md`'s "the event field is snake_case" sentence leads you to — parses lifecycle frames and **silently errors on every ask**. | One 12 s connection, both families: 30 × `{"data":{"pane":{…},"type":"pane_updated"},"event":"pane_updated"}` beside 1 × `{"data":{"agent":"opencode","agent_status":"working","pane_id":"wD:p1","workspace_id":"wD"},"event":"pane.agent_status_changed"}`. Schema agrees: `PaneAgentStatusChangedEvent.properties` has no `type`. |
| 7 | **There is no global agent-status subscription.** The product's only trigger is reachable *only* per-pane, and `events.subscribe` freezes the set at connect (no `events.update`), so a new pane forces a stream teardown. This is materially more scope than PLAN.md's slice-1 row implies. | `{"type":"pane.agent_status_changed"}` → `invalid_request: missing field 'pane_id'`. `{"type":"pane_agent_status_changed"}` → `unknown variant`. Exactly **3 of 27** Subscription variants require `pane_id`. |
| 8 | **`pane.updated` replays a stale backlog on every connect**, at ~100 ms/frame, each frame carrying a historical `agent_status`. It is the only globally-subscribable status-bearing event, and a bridge that reads status from it fires a phantom-push burst on every reconnect. | 30 frames in 2.93 s: wA:p1 replayed revisions **6→18** all reading `agent_status:"blocked"`, then wD:p1 revisions 12→26. Nothing for the following 9 s. |
| 9 | **A filtered subscription pinned to a pane's *current* status replays it immediately** — for any status, not just `blocked`. This is the deterministic, herd-state-independent, read-only proof of the event decoder, and it hands slice 3 the "laptop asleep → recover missed asks" row with no diffing. | `filter=idle` on w9:p1 (idle) → fired at **t=0.00**. `filter=working` on wD:p1 (working) → fired at **t=0.00**. Unfiltered on wD:p1 → ack only. `filter=blocked` on wD:p1 (working) → ack only. |

### Facts that reshape the client's types

| # | Correction | Evidence |
|---|---|---|
| 10 | `agent.send` **no longer exists**; `pane.send_input {pane_id, text?, keys?}` is new and does text+keys in one RPC (only `pane_id` required). | `agent.send` → `unknown variant`. `PaneSendInputParams.required = ["pane_id"]`. |
| 11 | Result payloads are **nested under a per-method key**, not flat. A flat model breaks on every call. | `session.snapshot`→`{type,snapshot}` · `pane.read`→`{type,read}` · `pane.list`→`{type,panes}` · `agent.list`→`{type,agents}` · writes→bare `{type:"ok"}`. |
| 12 | **`#[serde(other)]` on `AgentStatus` silently rewrites the wire value** and would corrupt the client's own `--json` the day herdr adds a status. A manual impl carrying `Unrecognized(String)` is required. | Compiled both: `"reticulating"` → `#[serde(other)]` re-serializes as **`"unrecognized"`** (round-trip false); the manual impl re-serializes as `"reticulating"` (round-trip true). |
| 13 | `params` and `id` are **both mandatory**; `id` must be a JSON string; the reply `id` is blanked to `""` on parse/routing errors but echoed on semantic ones — so **never correlate on it**. | `{"id":"b","method":"ping"}`→`missing field \`params\`` · `{"method":"ping","params":{}}`→`missing field \`id\`` · `{"id":7,…}`→`invalid type: integer` · `pane.read zz:p9`→`{"id":"probe","error":{"code":"pane_not_found"}}` vs garbage→`{"id":""}`. |
| 14 | **Omitting the trailing newline makes the server hang forever** — no error, no close. Only a client-side timeout catches it, so the invariant must be enforced at the type level. | Sent an unterminated `ping`: `TimeoutError after 5.01s`, connection still open. |
| 15 | RPC is strictly one-shot. The **connection is the correlation** — no pool, no id map, no background reader. | Second write on an answered connection → `BrokenPipeError [Errno 32]`. |
| 16 | `layouts` is **required** in `SessionSnapshot` — it must be carried (opaquely) or the client emits a snapshot herdr's own schema calls invalid. | `SessionSnapshot.required = [version,protocol,workspaces,tabs,panes,layouts,agents]`. Live: 6 layout objects. |
| 17 | `state_change_seq` has `"default": 0` and is **not** in `AgentInfo.required` — it must be `Option<u64>` or the slice-3 dedupe key silently collapses to 0 for every pane. | `{"default":0,"format":"uint64","minimum":0,"type":"integer"}`; absent from `AgentInfo.required`. |
| 18 | `PaneInfo.revision` is live but is **not** a change detector (it indexes the retained `pane_updated` backlog); `PaneReadResult.revision` is a hard-zero stub. Do not confuse them. | Live: pane revisions 5/18/5/9/26/8 in `pane.list`, while `pane.read` returned `revision=0` on every call. The replay showed revision climbing 6→18 as backlog index. |
| 19 | herdr **omits** unset optional fields rather than emitting `null` — so every optional field needs `skip_serializing_if`, or the client emits `"label":null` into the diff *and* into a Telegram message body. | `[.. |objects|to_entries[]|select(.value==null)]` over the whole live snapshot → **`[]`**. Absent live on all 6 panes: `label,title,tokens,state_labels,display_agent`. |
| 20 | `herdr pane read` **defaults to `--source recent`** — the harvest-scrolling source that moves the operator's screen. The Rust client must have no `Default` that can yield it. | `--source <SOURCE>  Terminal snapshot source (default: recent)`. |

### Two things every prior draft got wrong

| # | Common belief | Verified truth |
|---|---|---|
| 21 | "the socket's `pane.read` text carries one more trailing newline than the CLI; trim it" | **False — that is a `jq -r` artifact.** `jq -j '.result.read.text'` is **byte-identical** to `herdr pane read --source visible --format text`, 3/3 runs (6163 B each); `jq -r` adds exactly one byte. Gate 4 uses `cmp`, no trim. |
| 22 | "assert the visible read's line count equals `viewport_rows`" | **False — it is `viewport_rows − 1`.** A full `visible` read returns **62** newline-terminated lines on a 63-row viewport, on **5/5** panes. The gate asserts `1 ≤ lines ≤ viewport_rows` instead of baking in an unexplained off-by-one. |
| 23 | `.result.snapshot.protocol` compared as the string `"20"` | It is a **JSON number**. `jq -r '…protocol\|type'` → `number`. |

### Scope, repo wiring, and one PLAN.md self-contradiction

| # | Item |
|---|---|
| 24 | **`pane.send_text` / `send_keys` / `send_input` ship as typed, mock-tested code with *no live call site and no binary subcommand that reaches them*.** They cannot be proven tonight without typing into the operator's real agent panes — the exact catastrophic failure D3 exists to prevent. Live verification is scripted (`scripts/verify-send-p20.sh`) and gated on a throwaway `herdr --session probe` (verified: `--session <name>` is a real flag). |
| 25 | **The `pane.send_keys` key grammar is UNVERIFIED-ON-P20.** Pane lookup precedes key validation (a bogus pane returns `pane_not_found`, never `invalid_key`), so it *cannot* be probed against a live herd; the schema does not constrain keys at all (`keys: array of string`). Since `agent.send` was **removed** between p16 and p20, a p16 grammar is not a p20 fact — so `Key` ships as a validating newtype, **not** a closed enum. |
| 26 | **`pane.focus` / `agent.focus` are permanently absent from the client's public API.** `done` = "idle after work the operator has not *seen*", and focusing marks a pane seen — a focus call from the bridge would destroy the signal PLAN.md's second push trigger depends on. Type-level absence, so it cannot be called by accident. |
| 27 | **PLAN.md contradicts itself on the bot token**: line 144 says `.env` (git-ignored), line 156 says `herdr-tg.toml` (**not** ignored today — `git check-ignore` matches nothing). Resolved below; must land before slice 2 writes a config loader. |
| 28 | **The gate command's `env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH"` prefix is load-bearing twice.** `~/.cargo/bin` is **not** on this session's PATH (`which -a cargo` → only mise shims), and `mise env` exports `RUSTUP_TOOLCHAIN=stable` from the **global** `~/.config/mise/config.toml`, which silently overrides `rust-toolchain.toml` in every Rust repo on this box. |
| 29 | **Slice 1's real budget is 66 packages / 3.9 s cold build**, not the 242-package / ~15 s teloxide figure — that is slice 2's graph. Measured: cold build **3.86 s**, clippy **1.86 s**, fmt **0.02 s**, test **1.78 s**, `target/` 369 M. |
| 30 | **`tokio::io::Lines::next_line` is cancel-safe; `AsyncBufReadExt::read_line` is not** ("data may have been partially read, and this data is lost"). Slice 3 will `tokio::select!` the event stream against the Telegram long-poll, so this decides the reader. We use `Lines` — cancel-safe **and** zero extra crates (`tokio-util`/`codec` would add 2). |
| 31 | `README.md:44` documents `KICKOFF_CORE_DIR="$HOME/kickoff-core"`; `.kickoff/instance.env:236` sets `$HOME/Projects/claude-kickoff`. A fresh clone following the README gets "kickoff engine not present" from every scanner shim. |
| 32 | **Scanner baseline is green today** (`scan-secrets` and `scan-structure` both ✅). Anything they flag after slice 1 was introduced by slice 1. `scan-secrets` has **no Telegram pattern** — it catches a bot token only via its generic rule, which needs a key named `secret\|token\|password\|api_key\|…` **and** a quoted value. |

---

## The proof

### Command

```bash
cd $HOME/Projects/herdr-tg && \
  env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace && \
  env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" cargo build && \
  ./scripts/proof-slice1.sh
```

### Expected result

`cargo test --workspace` green with **no herdr running and no network** (that is what makes the crate
gateable on thev-box, D6), then seven gate lines and exit 0:

```
gate 0  reference sane         ok    session_snapshot, protocol 20, herdr 0.8.2
gate 1  herd non-empty         ok    6 workspaces / 6 panes
gate 2  sandboxed client       ok    PATH=<empty>, no HERDR_* env → socket fallback works
gate 3  snapshot equivalence   ok    sandwich matched on attempt 1
gate 4  pane.read parity       ok    w9:p1: 6163 B byte-identical, truncated=false, 62/63 rows
gate 5  event decode           ok    w9:p1 → pane.agent_status_changed{idle} (dot-form, untagged data)
gate 6  failure paths          ok    missing socket → exit 3; protocol 19 → exit 4; no panic
SLICE 1 PROOF: PASS — herdr-tg agrees with herdr 0.8.2 / protocol 20 on 6 workspaces / 6 panes
```

Any gate failing prints `gate N <name> FAIL <reason>` to stderr and exits 1. Gate 3's failure additionally
prints the canonicalized diff **and the explicit list of fields normalized out of both sides** — a proof that
silently drops fields can hide the bug it exists to catch.

**Gates 0, 1, 4 and 5 were executed tonight against the live herd; gates 2 and 6 were executed against a
compiled Rust stand-in; gate 3's sandwich mechanics were executed against a scripted client.** The script
skeleton, `normalize.jq` and `mock-herdr.py` are working at
`$SCRATCH/scratchpad/proof/`
— copy them into `$HOME/Projects/herdr-tg/scripts/`, do not re-derive them.

### What each gate proves

**Gate 0 — reference sanity.** `herdr api snapshot` exits 0, `.result.type == "session_snapshot"`,
`.result.snapshot.protocol == 20` (a **number**). Guards a dead server, a renamed CLI subcommand, and a
protocol bump. Honest note: on herdr 21 this gate fires *and* the client's own assertion would fire; gate 0
runs first and its message names both causes.

**Gate 1 — non-vacuity.** `workspaces > 0 && panes > 0`. An all-empty client must not trivially pass an
all-empty comparison.

**Gate 2 — sandboxed client (the anti-cheat).** Runs
`env -i HOME="$HOME" PATH="$(mktemp -d)" ./target/debug/herdr-tg status --json` and requires a
`session_snapshot` envelope. This does two jobs at once: a client that shells out to `herdr` gets rc=127
(verified), **and** it exercises the `HERDR_SOCKET_PATH`-absent → `~/.config/herdr/herdr.sock` fallback that
the production `systemd --user` unit depends on (every `HERDR_*` var is pane-injected only; verified a
stripped child sees none). Verified end to end with a real compiled Rust binary: reaches the socket, exits 0,
`ldd` shows only libc/libgcc_s/vdso/ld.so.

**Gate 3 — sandwiched canonical equivalence, `ATTEMPTS=5`.** Run client → reference → client; pass if the
reference matches **either** bracket. Both sides through `jq -S -f scripts/normalize.jq`. The sandwich is
load-bearing, not defensive: `focused` is duplicated into every pane *and* every agent record on top of the
three top-level `focused_*_id` fields, so a single focus switch dirties ~22 canonicalized lines, and a naive
single-pair diff flaps for reasons unrelated to client correctness — which trains the operator to ignore red.
The comparison is a **full-fidelity round trip**: `status --json` re-serializes the *typed* `Snapshot` back
into the `{"id","result":{"type","snapshot"}}` envelope, so any field herdr emits that we do not model drops
out of our side and turns the diff red. That is the drift alarm, sited in one place we control rather than in
twenty implicit `deny_unknown_fields` that would all fire at once under `Restart=always`.

`scripts/normalize.jq` drops from **both** sides and no more: `revision`, `state_change_seq`, `scroll`,
`screen_detection_skipped`, `terminal_title`, `terminal_title_stripped`, `layouts`, and all nulls. It
**keeps** `agent_status` (normalizing it out would make the proof vacuous — it is the product's entire
payload), `focused`/`focused_*_id`, ids, labels, numbers, counts, `cwd`, `foreground_cwd`, `agent`,
`display_agent`, `agent_session`, `version`, `protocol`, and — since the review below — `tokens`,
`title`, `state_labels`, `interactive_ready`, `launch_pending`. It sorts no arrays (`jq -S` sorts keys
only) — tabs must render in array order, never by `number`.

> **AMENDED 2026-08-28, review minor.** Those last five used to be on the drop list. A reviewer showed
> that made them invisible to BOTH proof layers at once: drop-listed here AND absent from every fixture
> AND absent from the live herd, so a wrong type would have been deleted from both sides before the
> compare and nothing in the repo would have noticed. They now sit in the KEEP set, where a future herdr
> that starts emitting one turns gate 3 RED instead of silently dropping it — the same safe failure mode
> `display_agent` and `name` already had. It costs nothing today: the live census is 0 occurrences of
> each, and the sandwich still matches on attempt 1. The offline half is
> `crates/herdr-client/tests/golden.rs::unobserved_optional_fields_decode_from_bytes`, which hand-builds
> the seven never-observed fields from the checked-in schema and asserts the decode. `terminal_title` /
> `terminal_title_stripped` stayed dropped: unlike the five, they are observed on every pane and really
> are volatile.

> Drift measured with this exact file: **6 consecutive live snapshots 1 s apart → 0 diff lines, 5/5
> intervals.** `terminal_title_stripped` is in the drop list because opencode retitles panes as the agent's
> task changes; the coverage that loses is recovered offline by `golden::snapshot_roundtrip_loses_nothing`,
> where the content is frozen.

**Gate 4 — `pane.read` parity.** Picks the first pane whose `pane_id != $HERDR_PANE_ID` (never reads the pane
the session runs in). `herdr-tg read <pane>` vs `herdr pane read --source visible --format text <pane>`,
compared with **`cmp` — byte-for-byte, no trim** (see delta #21). Also asserts, from the client's own `--json`
echo, that it sent `source == "visible"` (a silent `recent` default would scroll the operator's screen on
every background read) and `truncated == false`, because the text comparison alone is blind to `truncated`
and a client that clipped a long ask would otherwise pass. Line count asserted in `1..=viewport_rows`, not
`== viewport_rows` (delta #22). Retried 3× — the pane is live and its content can shift between the two calls.

**Gate 5 — event decode (deterministic, read-only).** Reads the snapshot, takes the first agent pane whose
status is not `unknown`, and runs
`herdr-tg watch --once --pane <id> --expect-status <its current status> --timeout-ms 5000`. The filtered
subscription replays that status at subscribe time, so **this gate needs no transition and no live agent
activity** — verified firing at t=0.00 on `idle` and on `working`, and verified silent when unfiltered or
mismatched. **This is the gate that proves the two-envelope decoder** — the one bug that would make the whole
product silently deliver nothing. Retried 3× in case the pane transitions between the read and the subscribe.

**Gate 6 — failure paths.** `HERDR_SOCKET_PATH=/nonexistent/herdr.sock herdr-tg status` must exit **3**, print
`herdr unreachable: …`, and contain no `panicked at` (verified with a real Rust binary: exit 3,
`herdr unreachable: /nonexistent/herdr.sock (No such file or directory (os error 2))`). Then
`scripts/mock-herdr.py --protocol 19` (written and tested tonight — a 25-line one-shot unix server answering
`pong` at protocol 19, one request per connection like the real one) must make `herdr-tg doctor` exit **4**
with a message naming the protocol. This is PLAN.md's "herdr dies / socket gone" row, which the old harness
structurally could not test — its gate 0 aborts the whole run whenever herdr is down, i.e. it tests the
opposite of the documented behaviour.

### Proving the proof can fail

`./scripts/proof-selftest.sh` runs eight fake clients through `proof-slice1.sh --gates=3` and asserts the
verdicts. It drives gate 3 in isolation because the fakes are shell scripts and would all fail gate 2 by
construction — *that is the anti-cheat working*.

| fake | required verdict |
|---|---|
| `fake-honest.sh` (socket RPC → `jq -c .`) | PASS |
| `fake-wireorder.sh` (reference re-emitted with top-level keys reversed) | PASS |
| `fake-nullpad.sh` (emits `"label":null`) | PASS |
| `fake-extracounter.sh` (mutates `revision`) | PASS |
| `fake-flap.sh` (stale `focused_workspace_id` on the first call only) | PASS *via the sandwich* |
| `fake-dropped_ws.sh` (drops a workspace) | FAIL |
| `fake-stale_status.sh` (mutates an `agent_status`) | FAIL |
| `fake-truncated_cwd.sh` (mutates a `cwd`) | FAIL |

Plus, run against the **full** gate set and mandatory: **`fake-cheat.sh`** — a shebang and
`herdr api snapshot`, nothing else — must **FAIL at gate 2**. If it passes, the sandbox is not working and
the whole proof is theatre.

> Fix before shipping: the existing `fake-honest.sh` and `fake-wireorder.sh` at
> `$HOME/.cache/tmp/proof/` are **byte-identical below the shebang** (verified with `diff`), so the
> wire-key-order case has never actually been exercised. Rewrite `fake-wireorder.sh` to emit real
> declaration-order keys.

### Automation and gate wiring

Everything in `cargo test --workspace` runs offline against a mock socket and checked-in fixtures. Append the
cargo gates to **`$HOME/Projects/herdr-tg/.kickoff/lefthook-kickoff.yml`**, *after* the existing
`secret-scan` entry — that file's own header says verbatim "The /adopt session ADDS the stack gates … to this
file. It is YOURS (adopter-owned): edit freely", the hook runner emits root-`lefthook.yml` commands *before*
`extends:` commands (so the root file would put a 1.9 s clippy ahead of a 0.09 s secret scan), and
`adopt-manifest.py` treats an adopt-created root `lefthook.yml` as pure wiring on `kickoff eject`. Leave the
root file as its bare `extends:`.

```yaml
pre-commit:
  commands:
    secret-scan:
      run: bash .kickoff/bin/scan-secrets --staged
    rust-fmt:
      run: env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" cargo fmt --all --check
    rust-clippy:
      run: env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" cargo clippy --workspace --all-targets -- -D warnings
pre-push:
  commands:
    structure-scan:
      run: bash .kickoff/bin/scan-structure
    rust-test:
      run: env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace
```

Three constraints from reading `.git/hooks/_kickoff-hook-runner` — **lefthook is not installed on this
machine**; a regex-subset shim runs the gates: one-line `run:` values only (a `run: |` block scalar executes
the literal `|`), `glob:` / `skip:` / `priority:` / `{staged_files}` are silently ignored, and file order is
execution order. Do **not** add `--all-features`: our crates declare none today, and the day one declares a
TLS-selecting feature it would link openssl into every clippy run.

**Cost to write down, not work around:** because `{staged_files}` is ignored, these gates run on the **working
tree, not the staged index** — an unstaged, half-finished file anywhere in the repo blocks every commit.
Workaround is `git stash -k`.

---

## Layout

All paths under `$HOME/Projects/herdr-tg/`.

### Create

| Path | Purpose |
|---|---|
| `Cargo.toml` | Workspace root: `resolver = "3"`, members, `[workspace.package]` (edition 2024, rust-version 1.85), `[workspace.dependencies]`, `[profile.release]`. |
| `Cargo.lock` | **Commit it** — this ships a binary. Verified no `.gitignore` rule matches it today. |
| `rust-toolchain.toml` | `channel = "stable"`, `components = ["rustfmt","clippy"]`, `profile = "minimal"`. Channel, **not** a version pin — a hard pin triggers a ~120 MB toolchain download on a fresh clone. Carries a comment that the pin is **inert under the mise shim** and only the gates' `env -u` restores it. |
| `crates/herdr-client/Cargo.toml` | Lib manifest. No tokio `rt`/`macros` — a library must not pick the runtime. No teloxide, no clap. |
| `crates/herdr-client/src/lib.rs` | Re-exports, protocol constants, and crate-level docs carrying the two invisible traps (two envelope families; the `pane.updated` replay) so a later session inherits them instead of re-deriving them. |
| `crates/herdr-client/src/ids.rs` | `PaneId` / `WorkspaceId` / `TabId` transparent newtypes. |
| `crates/herdr-client/src/keys.rs` | `Key` validating newtype + `Key::parse`, under an `UNVERIFIED-ON-P20` module banner. |
| `crates/herdr-client/src/error.rs` | `HerdrError`, `ErrorCode` (open string + data-carrying catch-all), the four routing predicates, `exit_code()`. |
| `crates/herdr-client/src/transport.rs` | Dial, one-shot `round_trip`, the newline invariant, the 1 MiB request guard, the 32 MiB response guard, timeouts. |
| `crates/herdr-client/src/client.rs` | `HerdrClient`, the `Request` trait, every typed method, `handshake()`. |
| `crates/herdr-client/src/proto/mod.rs` | Module wiring + re-exports. |
| `crates/herdr-client/src/proto/model.rs` | `SessionSnapshot`, `PaneInfo`, `AgentInfo`, `WorkspaceInfo`, `TabInfo`, `AgentSessionInfo`, `PaneScrollInfo`, `PaneRead`, `AgentStatus`, `PaneAgentState`, `ReadSource`, `ReadFormat`. |
| `crates/herdr-client/src/proto/request.rs` | The nine request types + `EmptyParams` (serializes as `{}`, never omitted). |
| `crates/herdr-client/src/proto/response.rs` | The `Reply` envelope, `ErrorBody`, and the per-method result **wrappers** (`{type,snapshot}`, `{type,read}`, …). |
| `crates/herdr-client/src/proto/event.rs` | The two-step decoder, `Event` / `AgentStatusChanged` / `RosterEvent`, `Subscription`. |
| `crates/herdr-client/src/stream.rs` | `EventStream` over `tokio::io::Lines`; `impl futures_core::Stream`; the no-self-heal contract. |
| `crates/herdr-client/src/handshake.rs` | `Pong`, `ServerCapabilities`, `Handshake`, `Compatibility`. |
| `crates/herdr-client/tests/support/mod.rs` | `MockHerdr`: a real `UnixListener` in a `tempfile::TempDir`; answers one request per connection **and closes**, mirroring the real server; records raw request bytes and connection count. |
| `crates/herdr-client/tests/fixtures/*` | Frames captured from the live socket (see build order step 3). |
| `crates/herdr-client/tests/{wire,events,golden,failure_paths,schema_drift}.rs` | The offline suite. |
| `crates/herdr-tg/Cargo.toml` | Bin manifest. teloxide stays **commented out** until slice 2. |
| `crates/herdr-tg/src/main.rs` | clap parse, tracing-subscriber init, dispatch, exit-code mapping. |
| `crates/herdr-tg/src/cmd/{status,read,doctor,watch}.rs` | The four subcommands. |
| `crates/herdr-tg/src/render.rs` | The human herd table. |
| `scripts/proof-slice1.sh` | The seven gates, with a `GATES=0,1,…` selector. |
| `scripts/normalize.jq` | The canonicalizer, drop list documented inline. |
| `scripts/mock-herdr.py` | The protocol-skew one-shot server for gate 6. |
| `scripts/proof-selftest.sh` + `scripts/fakes/fake-*.sh` | The mutation set that proves the proof can fail. |
| `scripts/capture-fixtures.sh` | Re-dump the schema + the NDJSON frames from the live herd. |
| `scripts/verify-send-p20.sh` | **Deferred.** Refuses to run unless `HERDR_TG_PROBE_SESSION` is set *and* the resolved socket differs from `~/.config/herdr/herdr.sock`. |

### Edit (do not create)

| Path | Change |
|---|---|
| `.kickoff/lefthook-kickoff.yml` | Append the three cargo gates after `secret-scan`. |
| `.gitignore` | Append the block below. |
| `PLAN.md` | Four corrections (build order step 11). |
| `README.md:44` | `KICKOFF_CORE_DIR` → `$HOME/Projects/claude-kickoff`. |

```gitignore
# Rust (cont.) — a member crate run standalone gets its own target/
target/
*.orig
*.rej
perf.data
perf.data.old
*.profraw

# Config carrying the bot token (PLAN.md "Stack") — track the example, never the real one
herdr-tg.toml
config/herdr-tg.toml
!herdr-tg.example.toml

# State: the atomic-write temp does NOT match the existing *.state.json rule
*.state.json.tmp
/state/

# Audit trail — records every keystroke sent into a real terminal; local-only by design
*.audit.log
/logs/
```

Verified with `git check-ignore -v`: **none** of these paths is ignored today. Three are security, not
tidiness — the config PLAN.md says holds the bot token, the `.tmp` shadow of the already-ignored state file,
and the append-only log of every keystroke typed into a live agent terminal.

---

## Dependencies

Every version re-queried against crates.io on 2026-08-28 and confirmed as `max_stable_version`. The whole set
below resolves to **66 packages** and builds cold in **3.86 s** on this aarch64 box (measured, with these
exact manifests).

```toml
# $HOME/Projects/herdr-tg/Cargo.toml
[workspace]
resolver = "3"
members  = ["crates/herdr-client", "crates/herdr-tg"]

[workspace.package]
edition      = "2024"
rust-version = "1.85"          # edition 2024 and clap 4.6.6 both require 1.85; toolchain here is 1.98.0
license      = "MIT"
repository   = "https://github.com/vinceferro/herdr-tg"

[workspace.dependencies]
tokio        = { version = "1.53.1", default-features = false }
serde        = { version = "1.0.229", features = ["derive"] }
serde_json   = "1.0.151"
thiserror    = "2.0.20"
futures-core = { version = "0.3.34", default-features = false, features = ["std"] }
tracing      = "0.1.44"
anyhow       = "1.0.104"
clap         = { version = "4.6.6", features = ["derive"] }
tracing-subscriber = { version = "0.3.23", default-features = false, features = ["fmt", "env-filter", "ansi"] }
tempfile     = "3.27.0"

[profile.release]
lto           = "thin"
codegen-units = 1
strip         = "debuginfo"    # keeps symbols: PLAN.md's failure table wants greppable backtraces
# NO panic = "abort": one panicking tokio task would kill the whole bridge, which interacts badly
# with the systemd Restart=always + StartLimitIntervalSec=0 posture, and breaks #[should_panic]
# under --release. Revisit at slice 4 with the unit in hand, as a deliberate call.
```

```toml
# crates/herdr-client/Cargo.toml
[package]
name = "herdr-client"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
tokio        = { workspace = true, features = ["net", "io-util", "time"] }  # NO rt, NO macros
serde        = { workspace = true }
serde_json   = { workspace = true }
thiserror    = { workspace = true }
futures-core = { workspace = true }
tracing      = { workspace = true }

[dev-dependencies]
tokio    = { workspace = true, features = ["rt-multi-thread", "macros", "net", "io-util", "time", "sync"] }
tempfile = { workspace = true }
```

```toml
# crates/herdr-tg/Cargo.toml
[package]
name = "herdr-tg"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
herdr-client = { path = "../herdr-client" }
tokio      = { workspace = true, features = ["rt-multi-thread", "macros", "signal", "time"] }
serde      = { workspace = true }
serde_json = { workspace = true }
anyhow     = { workspace = true }
clap       = { workspace = true }
tracing    = { workspace = true }
tracing-subscriber = { workspace = true }

# SLICE 2 — Telegram. `default-features = false` is LOAD-BEARING, not stylistic: teloxide 0.17.0's
# default is ['native-tls','ctrlc_handler','teloxide-core/default'] and teloxide-core 0.13.0's default
# is ['native-tls'] -> reqwest/native-tls -> openssl-sys, i.e. a build-time C dependency and a runtime
# link against system libssl, which silently contradicts D1's single-binary thesis.
# Verified 2026-08-28 with this exact feature string: 222 packages, reqwest 0.12.28, rustls 0.23.43,
# ring 0.17.14 (not aws-lc-rs, which wants cmake/clang-bindgen on aarch64), webpki-roots 1.0.9,
# hyper 1.11.0 — and ZERO openssl / openssl-sys / native-tls / aws-lc-* / rustls-native-certs crates.
# h2 and encoding_rs are absent, so Bot API long-polling runs over HTTP/1.1 (fine for D7).
# Consequence for slice 4's hardened unit: webpki-roots is compiled in, so no /etc/ssl/certs access is
# needed; the flip side is that CA-root updates need a rebuild (escape hatch: rustls -> rustls-native-roots).
# Supply-chain facts for the decision log: 0.17.0 dates from 2025-07-11 (13 months stale), MSRV 1.82,
# and it pulls proc-macro-error2 v2.0.1 which cargo already flags future-incompatible.
# teloxide = { version = "0.17.0", default-features = false, features = ["rustls", "ctrlc_handler", "macros"] }
```

Deliberate omissions, each with the reason:

- **No `tokio-util` / `LinesCodec`.** `tokio::io::Lines::next_line` is documented cancel-safe in tokio
  1.53.1's own source (`AsyncBufReadExt::read_line` is explicitly **not**), so `BufReader::lines()` gives
  slice 3 the `tokio::select!` composability for free. `tokio-util` would add 2 crates for nothing here.
- **No `tokio-stream`.** `futures_core::Stream` is the trait the client implements; the binary can poll it
  directly for slice 1's one-shot `watch`.
- **No `toml`.** Slice 2. Pinned when it arrives: `toml = "1.1.4"` resolves to `1.1.4+spec-1.1.0`, MSRV 1.85.

---

## Public API

```rust
// ══════════════════════ crates/herdr-client/src/lib.rs ══════════════════════
pub mod client; pub mod error; pub mod handshake; pub mod ids; pub mod keys;
pub mod proto;  pub mod stream; mod transport;

pub use client::{HerdrClient, Request, WriteAccepted};
pub use error::{ErrorCode, HerdrError};
pub use handshake::{Compatibility, Handshake, Pong, ServerCapabilities};
pub use ids::{PaneId, TabId, WorkspaceId};
pub use keys::{Key, KeyParseError};
pub use proto::event::{AgentStatusChanged, Event, RosterEvent, Subscription};
pub use proto::model::*;
pub use stream::EventStream;

/// The protocol this client was built and tested against (herdr 0.8.2, verified 2026-08-28).
pub const KNOWN_PROTOCOL: u32 = 20;
/// Below this we refuse to run. Unknown ADDITIONS are survivable; REMOVALS are not —
/// `agent.send` vanished between protocol 16 and 20, and a missing method surfaces as
/// `invalid_request: unknown variant`, which the client can detect but cannot repair.
pub const MIN_SUPPORTED_PROTOCOL: u32 = 20;
/// Max JSON body, newline EXCLUDED. Belt-and-braces: the server is loud (ECONNRESET), not silent.
pub const MAX_REQUEST_BODY_BYTES: usize = 1_048_576;
pub(crate) const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_SOCKET_RELPATH: &str = ".config/herdr/herdr.sock";
```

```rust
// ══════════════════════ src/ids.rs ══════════════════════
// The compile-time half of D3: the catastrophic failure of a remote-control surface is words
// landing in the wrong terminal, ids are plain strings on the wire, and `wC` / `wC:t1` / `wC:p1`
// are trivially transposable. `send_text(&PaneId, …)` cannot be called with a WorkspaceId.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct PaneId(String);
// identical: WorkspaceId, TabId

impl PaneId {
    pub fn new(s: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}
impl fmt::Display for PaneId { /* … */ }
impl From<&str> for PaneId { /* … */ }
// NO `workspace_hint()`. The "<workspace>:<pane>" shape holds in all 6 live samples but the schema
// types every id as an opaque string. Route from `PaneInfo::workspace_id`; never parse an id.
```

```rust
// ══════════════════════ src/keys.rs ══════════════════════
//! ⚠ UNVERIFIED-ON-P20. `pane.send_keys`'s key grammar is HERDR_API.md evidence enumerated against
//! herdr 0.7.0–0.7.4 / protocol 16. It CANNOT be re-probed against a live herd: pane lookup precedes
//! key validation (a bogus pane returns `pane_not_found`, never `invalid_key`), so validating it means
//! typing into a real pane. The schema does not constrain keys at all — `PaneSendKeysParams.keys` is
//! `{"items":{"type":"string"},"type":"array"}`. `agent.send` was REMOVED between p16 and p20, so a
//! p16 fact is not a p20 fact. Settle with scripts/verify-send-p20.sh in `herdr --session probe`, and
//! treat the per-agent SUBMIT key as a per-harness table (only claude and opencode are live here).
//! This is a validating NEWTYPE, not a closed enum, precisely so the p16 grammar is not encoded as
//! type-level truth.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)] pub struct Key(String);

impl Key {
    /// Rejects empty, whitespace-only, and anything containing '\n' or '\r' — a newline inside a
    /// "key" would become a real Enter at the PTY. Validates at CONFIG-LOAD, not at send time.
    pub fn parse(s: &str) -> Result<Key, KeyParseError>;
    pub fn enter() -> Key;                 // "Enter"
    pub fn as_str(&self) -> &str;
}
```

```rust
// ══════════════════════ src/error.rs ══════════════════════
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HerdrError {
    #[error("herdr unreachable: {path} ({source})")]
    Connect { path: PathBuf, #[source] source: io::Error },
    #[error("herdr I/O error during {method}: {source}")]
    Io { method: &'static str, #[source] source: io::Error },
    #[error("herdr timed out after {elapsed:?} during {method}")]
    Timeout { method: &'static str, elapsed: Duration },
    #[error("herdr closed the connection during {method} before replying")]
    ClosedEarly { method: &'static str },
    #[error("{method} request body is {len} B, over the server's {max} B cap")]
    RequestTooLarge { method: &'static str, len: usize, max: usize },
    #[error("could not decode herdr reply to {method}: {source}\n  line: {line}")]
    Decode { method: &'static str, #[source] source: serde_json::Error, line: String },
    #[error("herdr returned {code}: {message}")]
    Protocol { method: &'static str, code: ErrorCode, message: String },
    #[error("{method} returned result type {got:?}, expected {expected:?}")]
    UnexpectedResult { method: &'static str, expected: &'static str, got: String },
    #[error("herdr {server_version} speaks protocol {server}; this client requires >= {min} (built for {client})")]
    ProtocolTooOld { server: u32, min: u32, client: u32, server_version: String },
}

/// `ErrorBody.code` is an OPEN string in the schema — verified: `{"code":{"type":"string"},
/// "message":{"type":"string"}}` with no enum, and grepping the whole 255 KB dump for
/// `pane_not_found` / `invalid_key` returns nothing. A closed enum would fail to parse a future
/// code and mask its message. Needs a MANUAL Deserialize — derive cannot express a data-carrying
/// catch-all.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidRequest, PaneNotFound, TabNotFound, WorkspaceNotFound,
    AgentNotFound, AgentBlocked, AgentNotReady, AgentPromptFailed, AgentPromptStalled,
    AgentSendKeysFailed, InvalidKey, InvalidTarget, UiBusy, UnsupportedEventWaitMatch,
    Other(String),
}
impl ErrorCode { pub fn as_str(&self) -> &str; pub fn from_wire(s: &str) -> Self; }
impl<'de> Deserialize<'de> for ErrorCode { /* String -> from_wire */ }

// Each predicate maps 1:1 to a named row of PLAN.md's failure table, so the binary branches on a
// type rather than string-matching.
impl HerdrError {
    /// "herdr dies / socket gone" → /status says "herdr unreachable"; the loop backs off.
    pub fn is_unreachable(&self) -> bool;   // Connect | Io | Timeout | ClosedEarly
    /// "sticky target pane closed" → offer the picker, NEVER silently reroute.
    pub fn is_not_found(&self) -> bool;     // Pane/Tab/WorkspaceNotFound
    /// `invalid_request` whose message begins "invalid request: unknown variant" — this herdr lacks
    /// the method. Verified: the message enumerates every method it DOES have, so this doubles as a
    /// capability probe.
    pub fn is_unsupported_method(&self) -> bool;
    /// Exit non-zero with a distinct log signature; do not retry. Currently ProtocolTooOld.
    pub fn is_fatal(&self) -> bool;
    /// 3 = unreachable · 4 = protocol skew · 5 = herdr protocol error · 1 = otherwise.
    /// Proof gate 6 asserts 3 and 4 exactly.
    pub fn exit_code(&self) -> i32;
}
```

```rust
// ══════════════════════ src/proto/model.rs ══════════════════════
// THE SERIALIZATION RULE, and it is not stylistic. herdr OMITS unset optionals rather than emitting
// null (verified: zero nulls anywhere in the live snapshot; label/title/tokens/state_labels/
// display_agent absent on all 6 panes). So EVERY non-required field is Option<T> with
// `#[serde(default, skip_serializing_if = "Option::is_none")]` — maps and vecs INCLUDED. A bare
// `#[serde(default)] BTreeMap` would re-serialize as `{}` where herdr emitted nothing and fail
// proof gate 3 for a purely cosmetic reason; and `"label":null` would appear verbatim in a Telegram
// message body.

/// Read everywhere. MANUAL (de)serialize, deliberately: `#[serde(other)]` compiles but DISCARDS the
/// wire string — verified, `"reticulating"` re-serializes as `"unrecognized"`, which would silently
/// corrupt the client's own --json and fail gate 3 the day herdr adds a status. Agent-detection
/// manifests are REMOTELY versioned (claude on 2026.08.21.1, opencode on 2026.06.10.1, 20 manifests
/// on this host) and gain values without a herdr release, so the catch-all is not optional.
/// `Unrecognized` is deliberately distinct from herdr's own `Unknown`: one means "herdr does not know
/// what this agent is doing", the other means "this herdr is newer than this client". Neither pushes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentStatus { Idle, Working, Blocked, Done, Unknown, Unrecognized(String) }
impl AgentStatus {
    pub fn as_str(&self) -> &str;
    pub fn from_wire(s: &str) -> Self;      // unknown value -> Unrecognized(s), warn! once
}
impl Serialize for AgentStatus { /* serialize_str(self.as_str()) */ }
impl<'de> Deserialize<'de> for AgentStatus { /* String -> from_wire */ }

/// Write side only (`pane.report_agent`). Verified distinct: NO `done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneAgentState { Idle, Working, Blocked, Unknown }

/// SessionSnapshot.required = [version, protocol, workspaces, tabs, panes, layouts, agents].
/// NOTE `layouts` is REQUIRED — carried opaquely so the round trip is lossless, and dropped from
/// the proof diff by normalize.jq. The client is therefore explicitly NOT proven to parse pane
/// geometry; if slice 3's switcher ever renders it, that becomes a real hole.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub version: String,                       // "0.8.2"
    pub protocol: u32,                         // 20 — a NUMBER on the wire, not a string
    pub workspaces: Vec<WorkspaceInfo>,
    pub tabs: Vec<TabInfo>,                    // render in ARRAY order; never sort by `number`
    pub panes: Vec<PaneInfo>,
    pub layouts: Vec<serde_json::Value>,
    pub agents: Vec<AgentInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub focused_workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub focused_tab_id: Option<TabId>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub focused_pane_id: Option<PaneId>,
}
impl SessionSnapshot {
    pub fn pane(&self, id: &PaneId) -> Option<&PaneInfo>;
    pub fn agent(&self, id: &PaneId) -> Option<&AgentInfo>;
    /// Panes with a detected agent whose status is neither Unknown nor Unrecognized — the set
    /// gate 5 picks from and slice 3 fans subscriptions out over.
    pub fn agent_panes(&self) -> impl Iterator<Item = &AgentInfo>;
}

/// 19 schema properties, 7 required (verified). NO `deny_unknown_fields` — see risks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: PaneId, pub terminal_id: String, pub workspace_id: WorkspaceId,
    pub tab_id: TabId, pub focused: bool, pub agent_status: AgentStatus,
    /// NOT an output-change detector and NOT a state-change counter: it indexes the retained
    /// `pane_updated` backlog (verified climbing 6→18 during a replay while the pane was static).
    /// Normalized out of the proof. Do not build change detection on it.
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub display_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub title: Option<String>,
    /// Live on 6/6 panes ("OC | Omarchy tooling shipping"). Agent-authored only WHILE an agent owns
    /// the pane — a shell prompt otherwise. Volatile (opencode retitles every 20–40 s), so it is
    /// dropped from the proof diff and its decoding is proven by the offline fixture test instead.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub terminal_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub terminal_title_stripped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub foreground_cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_session: Option<AgentSessionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub scroll: Option<PaneScrollInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub state_labels: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub tokens: Option<BTreeMap<String, String>>,
}

/// 22 properties, the same 7 required, plus 5 agent-only fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    pub terminal_id: String, pub agent_status: AgentStatus, pub workspace_id: WorkspaceId,
    pub tab_id: TabId, pub pane_id: PaneId, pub focused: bool, pub revision: u64,
    /// Monotonic per-pane state-change counter — the dedupe key slices 3/4 need. Verified
    /// `"default": 0` and NOT in `required`, so it can legitimately be absent: Option, never a bare
    /// u64, or the key silently collapses to 0 for every pane. It is NOT carried on the status
    /// EVENT, so keying a dedupe on it costs an extra `agent.get` with a race. Slice 1 surfaces it
    /// and takes no position; slice 3 must verify its granularity first (see risks).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub state_change_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub interactive_ready: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub launch_pending: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub screen_detection_skipped: Option<bool>,
    // …plus the same optional set as PaneInfo (agent, display_agent, label, title,
    //   terminal_title, terminal_title_stripped, cwd, foreground_cwd, agent_session,
    //   scroll, state_labels, tokens)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceInfo {           // required: all 8
    pub workspace_id: WorkspaceId, pub number: u32, pub label: String, pub focused: bool,
    pub pane_count: u32, pub tab_count: u32, pub active_tab_id: TabId, pub agent_status: AgentStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabInfo {                 // required: all 7
    pub tab_id: TabId, pub workspace_id: WorkspaceId, pub number: u32, pub label: String,
    pub focused: bool, pub pane_count: u32, pub agent_status: AgentStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSessionInfo { pub source: String, pub agent: String, pub kind: String, pub value: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneScrollInfo { pub offset_from_bottom: u64, pub max_offset_from_bottom: u64, pub viewport_rows: u64 }

/// PaneReadResult.required = 8 fields (the doc lists 3).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneRead {
    pub pane_id: PaneId, pub workspace_id: WorkspaceId, pub tab_id: TabId,
    pub source: ReadSource, pub format: ReadFormat, pub text: String,
    /// ALWAYS 0 — a hard-coded stub here, while PaneInfo::revision is live. Named `_stub` in the
    /// accessor docs so a later reader cannot get the asymmetry backwards.
    pub revision: u64,
    /// True when the RETURNED text is shorter than what the source held. Asking for MORE lines than
    /// exist is satisfied silently with truncated:false (verified: lines=200 on a 63-row viewport →
    /// the full text, truncated:false). So this means "I clipped", not "I clamped to the viewport".
    pub truncated: bool,
}
impl PaneRead {
    pub fn line_count(&self) -> usize;
    /// `lines` counts NEWLINES, and a full `visible` read returns viewport_rows − 1 of them
    /// (verified 62 on a 63-row viewport, 5/5 panes). Matters when sizing a Telegram excerpt.
    pub fn trimmed_tail(&self, max_lines: usize) -> &str;
}

/// NO `Default` impl anywhere in this crate — `herdr pane read` defaults to `Recent`, the
/// harvest-scrolling source, and any Rust Default that yields it would move the operator's screen
/// on every background poll. This enum is `pub(crate)`; see the client's read methods.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadSource { Visible, Recent, RecentUnwrapped, Detection }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadFormat { Text, Ansi }
```

```rust
// ══════════════════════ src/proto/response.rs ══════════════════════
#[derive(Deserialize)]
pub(crate) struct Reply {
    /// NEVER COMPARED. Verified: semantic errors echo the request id, parse/routing errors blank it
    /// to "". Because RPC is one-shot the CONNECTION is the correlation; an
    /// `assert_eq!(reply.id, sent.id)` would misclassify every invalid_request as a framing bug and
    /// hide the real message.
    #[serde(default)] pub id: String,
    pub result: Option<serde_json::Value>,
    pub error:  Option<ErrorBody>,
}
#[derive(Deserialize)] pub(crate) struct ErrorBody { pub code: ErrorCode, pub message: String }

// Result payloads are NESTED under a per-method key (verified live on all six methods), so every
// Response type models its own wrapper. A flat model breaks on every call.
#[derive(Deserialize)] pub(crate) struct SnapshotResult { pub snapshot: SessionSnapshot }
#[derive(Deserialize)] pub(crate) struct ReadResult     { pub read: PaneRead }
#[derive(Deserialize)] pub(crate) struct AgentListResult{ pub agents: Vec<AgentInfo> }
#[derive(Deserialize)] pub(crate) struct PaneListResult { pub panes:  Vec<PaneInfo> }
#[derive(Deserialize)] pub(crate) struct OkResult       {}
```

```rust
// ══════════════════════ src/client.rs ══════════════════════
/// Stateless: a path and two timeouts. RPC is strictly one-shot (verified: a second write on an
/// answered connection → BrokenPipe), so there is no pool, no id-correlation map, and no background
/// reader task.
#[derive(Clone, Debug)]
pub struct HerdrClient { /* Arc<Path>, connect_timeout, request_timeout */ }

/// Ties method + params + the result TAG + the result TYPE together so a mismatch is
/// unrepresentable. RESULT_TAGs verified against the schema's 58 ResponseResult tags:
/// ping→"pong", session.snapshot→"session_snapshot", pane.read→"pane_read",
/// events.subscribe→"subscription_started", agent.list→"agent_list", pane.list→"pane_list",
/// and the three writes→"ok".
pub trait Request: Serialize + Send + Sync {
    const METHOD: &'static str;
    const RESULT_TAG: &'static str;
    type Response: DeserializeOwned + Send;
}

impl HerdrClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self;               // 2 s connect, 10 s request
    /// $HERDR_SOCKET_PATH, else $HOME/.config/herdr/herdr.sock. The env var is PANE-INJECTED ONLY
    /// (verified: a stripped child sees no HERDR_* at all), so a `systemd --user` unit WILL take the
    /// fallback. The socket is srw------- thev:thev, so filesystem permissions ARE the auth layer.
    /// Proof gate 2 exercises exactly this path.
    pub fn from_env() -> Result<Self, HerdrError>;
    pub fn with_timeouts(self, connect: Duration, request: Duration) -> Self;
    pub fn socket_path(&self) -> &Path;
    pub async fn call<R: Request>(&self, req: &R) -> Result<R::Response, HerdrError>;

    pub async fn ping(&self) -> Result<Pong, HerdrError>;
    /// Errors `ProtocolTooOld` below MIN_SUPPORTED_PROTOCOL; warns ONCE above KNOWN_PROTOCOL and
    /// proceeds. MUST be re-run on every event-stream reconnect, not only at boot:
    /// `capabilities.live_handoff` is true on this server, so herdr can swap its own binary
    /// underneath a running bridge without the socket path changing.
    pub async fn handshake(&self) -> Result<Handshake, HerdrError>;
    pub async fn snapshot(&self) -> Result<SessionSnapshot, HerdrError>;
    pub async fn agents(&self) -> Result<Vec<AgentInfo>, HerdrError>;
    /// D2 (one bot per workspace): scope the roster server-side in one RPC. An unknown id returns a
    /// distinct `workspace_not_found`, so a bot whose workspace closed says so rather than reporting
    /// an empty herd.
    pub async fn panes(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<PaneInfo>, HerdrError>;

    // ── reads. `recent` / `recent_unwrapped` / `detection` are UNREACHABLE from this crate in v1. ──
    /// The safe background read. Sends {source:"visible"} with `lines` OMITTED, so it cannot trip
    /// the lines>viewport_rows scroll harvest even in principle.
    pub async fn read_visible(&self, pane: &PaneId) -> Result<PaneRead, HerdrError>;
    /// Also safe: `visible` is clamped however large `lines` is. NonZeroU32 because lines=0 returns
    /// an empty string with truncated:true (verified).
    pub async fn read_visible_tail(&self, pane: &PaneId, lines: NonZeroU32) -> Result<PaneRead, HerdrError>;

    // ── writes. NO live call site in slice 1; no binary subcommand reaches these. ──
    /// `WriteAccepted`, never `bool`, and never a method named `deliver`: the wire ack is a bare
    /// {"type":"ok"} that means herdr took the bytes. It does NOT mean the agent received, rendered,
    /// parsed or acted on them. Slice 3's Telegram confirmation must say "accepted", not "delivered".
    #[must_use] pub async fn send_text(&self, pane: &PaneId, text: &str) -> Result<WriteAccepted, HerdrError>;
    #[must_use] pub async fn send_keys(&self, pane: &PaneId, keys: &[Key]) -> Result<WriteAccepted, HerdrError>;
    /// Protocol 20's atomic text+keys in ONE RPC (verified: `PaneSendInputParams.required = ["pane_id"]`).
    /// Slice 3's intended product path — it collapses the send_text→send_keys pair and removes the
    /// ordering question. UNVERIFIED: whether it frames text in bracketed paste. HERDR_API.md's 0.7.4
    /// finding that send_text writes RAW bytes (so a \n inside text is a real Enter, not pasted
    /// content) was never retested on 0.8.2, and multi-line Telegram replies are this product's
    /// DEFAULT case. scripts/verify-send-p20.sh settles it before slice 3 sends anything.
    #[must_use] pub async fn send_input(&self, pane: &PaneId, text: Option<&str>, keys: &[Key])
        -> Result<WriteAccepted, HerdrError>;

    /// Returns only AFTER the {"result":{"type":"subscription_started"}} ack is consumed, so
    /// "subscribed" is a distinct awaitable moment and the ack never leaks as an event.
    pub async fn subscribe(&self, subs: &[Subscription]) -> Result<EventStream, HerdrError>;
    /// There is NO heartbeat on the event stream (>9 s of silence observed on a healthy one after
    /// the backlog drained), so liveness must be probed out-of-band on a fresh connection. Verified
    /// that fresh RPCs work fine while a stream is held open — the stream does not serialize herdr.
    pub async fn is_alive(&self) -> bool;
}

#[derive(Clone, Debug)] #[must_use]
pub struct WriteAccepted { pub pane_id: PaneId, pub bytes: usize, pub at: SystemTime }

// DELIBERATE, PERMANENT ABSENCES — enforced by the type system, not by a comment:
//   pane.focus / agent.focus — focusing marks a pane SEEN, and `done` is defined as "idle after
//     work the operator has not seen". A focus call from the bridge would destroy the very signal
//     PLAN.md's second push trigger depends on. Not exposed, so it cannot be called by accident.
//   read with a caller-chosen ReadSource — `recent` is not reachable from this crate.
//   agent.prompt — slice 3. It is bracketed-paste-aware and appends its own Enter, but it REFUSES an
//     already-blocked agent with `agent_blocked` BEFORE sending anything, and has a distinct
//     `agent_prompt_stalled` outcome. That is a real fork PLAN.md does not contain: an IDLE agent
//     gets agent.prompt; a BLOCKED agent — the case this product exists to serve — must go through
//     send_input/send_keys.
```

```rust
// ══════════════════════ src/proto/event.rs ══════════════════════
/// Both envelope schemas require exactly {event, data} — same envelope, two incompatible `data`
/// shapes, discriminated ONLY by the outer `event` string. Decode is therefore two-step and tagged
/// on the OUTER field. A single #[serde(tag="type")] over `data` parses the roster family and
/// silently errors on every ask.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Event {
    /// THE ONLY EVENT SLICE 3 MAY PUSH ON. Dot-form outer `event`; `data` carries NO `type`.
    /// Not replayed at subscribe unless a status filter matches — these are the real edges.
    AgentStatus(AgentStatusChanged),
    ScrollChanged(serde_json::Value),
    OutputMatched(serde_json::Value),
    /// CACHE-INVALIDATION POKE ONLY — see RosterEvent.
    Roster(RosterEvent),
    /// A kind this client was not built for (a protocol-21 event, or one of the 19 lifecycle kinds
    /// we do not model). Bucketed and logged once, never fatal. This is the property that keeps the
    /// bridge alive through a routine `herdr update`.
    Unrecognized { event: String, data: serde_json::Value },
}

/// Verified required = [pane_id, workspace_id, agent_status]. `title`, `display_agent` and
/// `state_labels` are optional AND were absent from every live frame captured — an ask summary is
/// NOT free here; slice 3's ask extraction must come from read_visible().
#[derive(Clone, Debug, Deserialize)]
pub struct AgentStatusChanged {
    pub pane_id: PaneId, pub workspace_id: WorkspaceId, pub agent_status: AgentStatus,
    #[serde(default)] pub agent: Option<String>,
    #[serde(default)] pub display_agent: Option<String>,
    #[serde(default)] pub title: Option<String>,
    #[serde(default)] pub state_labels: Option<BTreeMap<String, String>>,
}

/// ROSTER CHANGES ONLY — and the deserializer READS THE EMBEDDED PaneInfo AND THROWS IT AWAY,
/// keeping only ids. `pane.updated` replays a rolling, ageing backlog on EVERY connect (verified:
/// 30 frames in 2.93 s at ~100 ms cadence, wA:p1 replaying revisions 6→18), and it is the only
/// globally-subscribable status-bearing event. Making the status structurally unreadable from this
/// family is what makes the phantom-blocked burst on every reconnect impossible to express. Truth
/// comes from snapshot(). Because the backlog AGES, dedupe-by-frame-identity cannot work either.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RosterEvent {
    PaneCreated { pane_id: PaneId, workspace_id: WorkspaceId },
    PaneUpdated { pane_id: PaneId, workspace_id: WorkspaceId },
    PaneClosed  { pane_id: PaneId, workspace_id: WorkspaceId },
    PaneExited  { pane_id: PaneId, workspace_id: WorkspaceId },
    /// PLAN.md needs a new failure row for this: a moved pane gets a NEW pane_id and the old one
    /// stops resolving even though the agent is alive and the pane is not closed. `previous_pane_id`
    /// lets sticky state migrate silently instead of falling back to the picker.
    PaneMoved { previous_pane_id: PaneId, pane_id: PaneId, workspace_id: WorkspaceId },
    PaneAgentDetected { pane_id: PaneId, workspace_id: WorkspaceId,
                        agent: Option<String>, released: Option<bool> },
    WorkspaceClosed { workspace_id: WorkspaceId },
}

pub(crate) const KNOWN_ROSTER: &[&str] = &[
    "pane_created","pane_updated","pane_closed","pane_exited",
    "pane_agent_detected","pane_moved","workspace_closed",
];

/// Two-step by construction. The KNOWN_ROSTER / known-dot-form gate is what makes the Unrecognized
/// arm safe: an unmodelled KIND is bucketed, but a MALFORMED frame of a kind we DO claim to handle
/// still errors loudly.
pub fn decode_event(line: &str) -> Result<Event, HerdrError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Subscription {
    /// pane_id is REQUIRED — verified live and in the schema (exactly 3 of 27 variants take it).
    /// There is NO global agent-status subscription, so slice 3 must fan out one per agent pane.
    /// With `agent_status: Some(s)` the server REPLAYS the pane's current status at subscribe time
    /// if it already equals s (verified firing at t=0.00 for idle and for working; unfiltered and
    /// non-matching both fire nothing). That is proof gate 5's mechanism and slice 3's recovery path.
    #[serde(rename = "pane.agent_status_changed")]
    PaneAgentStatusChanged { pane_id: PaneId,
        #[serde(skip_serializing_if = "Option::is_none")] agent_status: Option<AgentStatus> },
    #[serde(rename = "pane.scroll_changed")]  PaneScrollChanged { pane_id: PaneId },
    #[serde(rename = "pane.created")]         PaneCreated,
    #[serde(rename = "pane.closed")]          PaneClosed,
    #[serde(rename = "pane.exited")]          PaneExited,
    #[serde(rename = "pane.moved")]           PaneMoved,
    #[serde(rename = "pane.agent_detected")]  PaneAgentDetected,
    #[serde(rename = "workspace.closed")]     WorkspaceClosed,
    /// ⚠ FIREHOSE — ~10 frames/s observed with no user interaction. Never in a default set, never a
    /// push trigger. Present only so slice 3 cannot "discover" it by re-deriving the wire name.
    #[serde(rename = "pane.focused")]         PaneFocused,
}
impl Subscription {
    /// Slice 1 helper (also what `watch --once` uses): a FILTERED subscription pinned to a status.
    pub fn agent_status(pane: &PaneId, status: AgentStatus) -> Self;
    /// UNFILTERED — fires only on real transitions, never replays.
    pub fn agent_status_any(pane: &PaneId) -> Self;
}
```

```rust
// ══════════════════════ src/stream.rs ══════════════════════
/// Reader is `tokio::io::Lines` over `BufReader::new(stream.take(MAX_RESPONSE_BYTES))`.
/// `Lines::next_line` is documented cancel-safe in tokio 1.53.1 (partial state lives in the struct);
/// `AsyncBufReadExt::read_line` explicitly is NOT ("this data is lost") — and slice 3 will
/// tokio::select! this stream against teloxide's long-poll, where a lost partial line is a lost ask.
pub struct EventStream { /* UnixStream + Lines + the subs it opened with */ }

impl EventStream {
    /// The set this stream was opened with. `events.subscribe` FREEZES the set at connect and there
    /// is no `events.update`, so a newly created pane requires tearing this down and re-opening —
    /// the binary re-issues these verbatim plus the new pane.
    pub fn subscriptions(&self) -> &[Subscription];
}

impl futures_core::Stream for EventStream {
    /// `None` == the server closed the stream. THE CLIENT NEVER RECONNECTS ITSELF: PLAN.md's failure
    /// table demands "a single recovery notice when the stream re-establishes (not one per retry)",
    /// which is unimplementable if the client hides the drop — and because subscribe replays a
    /// rolling roster backlog, a silent internal reconnect would re-deliver history as phantom edges.
    /// Disconnect is a first-class observable the binary owns.
    type Item = Result<Event, HerdrError>;
}
```

```rust
// ══════════════════════ src/transport.rs (crate-private) ══════════════════════
/// THE ONLY WRITER. Appends the newline itself — there is no public path that can send an
/// unterminated line, because omitting it makes the server hang FOREVER with no error and no close
/// (verified: 5.01 s, zero bytes, connection still open). Only the request timeout would catch it,
/// so this is a type-level invariant, not a doc comment. Also: rejects over-cap bodies client-side,
/// reads through .take(MAX_RESPONSE_BYTES) so a pathological reply cannot OOM the bridge, and maps a
/// 0-byte read to ClosedEarly rather than an empty-string parse error.
pub(crate) async fn round_trip(
    socket_path: &Path, method: &'static str, body: &[u8],
    connect_timeout: Duration, request_timeout: Duration,
) -> Result<String, HerdrError>;
```

```rust
// ══════════════════════ crates/herdr-tg/src/main.rs ══════════════════════
#[derive(clap::Parser)]
struct Cli {
    /// Overrides $HERDR_SOCKET_PATH and the ~/.config/herdr/herdr.sock fallback. Gate 6 needs it.
    #[arg(long, global = true)] socket: Option<PathBuf>,
    #[command(subcommand)] cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Human herd table by default. `--json` is the PROOF surface: the FULL RPC envelope,
    /// re-serialized from the client's own typed structs — not a Value passthrough, or gate 3 stops
    /// being a decoder test. The envelope (including the `result` wrapper) is mandatory because
    /// normalize.jq begins at `.result.snapshot`; a bare snapshot makes the harness report
    /// "produced no parseable JSON", which reads as a crash rather than a shape mismatch.
    Status { #[arg(long)] json: bool, #[arg(long)] workspace: Option<String> },
    /// pane.read, visible only. Text to stdout; `--json` emits the full {type,read} envelope so the
    /// proof can assert `source == "visible"` and `truncated == false`.
    Read { pane: String, #[arg(long)] lines: Option<NonZeroU32>, #[arg(long)] json: bool },
    /// Server version, protocol, capabilities, compatibility verdict, resolved socket path.
    /// The one command the operator can run from a phone-driven session to answer "is the bridge's
    /// view of herdr still valid", and the only thing that exercises the handshake outside startup.
    Doctor { #[arg(long)] json: bool },
    /// `--once` opens ONE filtered subscription pinned to --expect-status, prints the decoded event,
    /// and exits — this is proof gate 5. Without --once it prints decoded events until the stream
    /// closes (the manual smoke test; slice 3's reconnect loop replaces it).
    Watch { #[arg(long)] pane: String, #[arg(long)] once: bool,
            #[arg(long)] expect_status: Option<String>, #[arg(long, default_value = "5000")] timeout_ms: u64 },
}
// Exit codes: 0 ok · 1 other · 2 usage · 3 herdr unreachable · 4 protocol mismatch · 5 herdr protocol error.
// Gate 6 asserts 3 and 4 exactly.
```

`status` (human), rendered from tonight's live herd — `*` marks focus, panes in **array order**:

```
herd: 6 workspaces, 6 panes   (herdr 0.8.2, protocol 20)
  w9:p1  acme-monorepo       idle     opencode  OC | Session one
  wA:p1  desktop-lab         blocked  opencode  OC | Session two
  wB:p1  agent-kickoff       blocked  opencode  OC | Session three
  wC:p1  api-gateway         blocked  opencode  OC | Session four
  wD:p1  notes-linkmap       working  opencode  OC | Session five
* wE:p1  bridge-tg           working  claude    Session six
```

---

## Tests

Every test below runs **offline** — no herdr socket, no network, no `HERDR_*` env — against
`tests/support::MockHerdr` (a real `UnixListener` in a `tempfile::TempDir` that answers one request per
connection **and closes**, mirroring the real server) and NDJSON fixtures captured from the live herd. That is
structural, not stylistic: D6 puts pushes on thev-box, which has no herdr socket, so a client testable only
against a live herd could not be gated at all.

### `tests/events.rs` — the highest-value file

| Test | What it proves |
|---|---|
| `two_envelope_families_decode_from_one_stream` | **THE test.** Feeds the captured dot-form `{"data":{"agent":"opencode","agent_status":"working","pane_id":"wD:p1","workspace_id":"wD"},"event":"pane.agent_status_changed"}` (no `data.type`) immediately followed by a captured snake_case `pane_updated` frame (`data.type` present); both must decode to the right variant. A serde enum tagged on `data.type` parses the second and silently drops the first — the product's only trigger. |
| `roster_event_discards_pane_info` | Feeds a `pane_updated` frame whose embedded `PaneInfo` says `agent_status:"blocked"`; asserts the decoded `RosterEvent` exposes no status at all. The phantom-push burst on every reconnect must be **structurally unreachable**, and this is easy to "helpfully" restore in a later refactor. |
| `unknown_event_kind_is_bucketed_not_fatal` | An invented `{"event":"pane_teleported","data":{}}` → `Event::Unrecognized`, `Ok` not `Err`. Also a real unmodelled kind (`workspace_focused`). The forward-compat contract that replaces `deny_unknown_fields` and keeps the bridge alive through a `herdr update`. |
| `malformed_known_kind_still_errors` | A `pane.agent_status_changed` frame missing `agent_status` → `Err`, not `Unrecognized`. Proves the catch-all is gated on the KIND and cannot swallow real corruption. |
| `subscription_serializes_dot_form_with_pane_id` | `Subscription::agent_status(&pane, Blocked)` → `{"type":"pane.agent_status_changed","pane_id":"…","agent_status":"blocked"}`; the unfiltered form **omits** `agent_status` (not null). Live, the snake_case form is `unknown variant` and the pane_id-less form is `missing field pane_id`. |
| `ack_is_consumed_then_events_flow` | Mock emits the ack + 3 frames + EOF; `subscribe()` returns only after the ack and the stream yields exactly 3 items — `subscription_started` must never leak as an event. |
| `end_of_stream_is_none_and_no_reconnect` | EOF → `None`, and the mock's connection counter shows no further dial. The no-self-heal contract slice 3's single-recovery-notice rule depends on. |
| `subscriptions_are_retained_for_reissue` | `stream.subscriptions()` equals what was passed, so the reconnect loop re-issues verbatim. |

### `tests/wire.rs`

| Test | What it proves |
|---|---|
| `request_is_always_newline_terminated` | Mock inspects the raw bytes; last byte must be `0x0A`. Omitting it hangs the real server forever with no error and no close — the worst failure mode for a phone-only operator, so this is an assertion, not a doc comment. |
| `params_is_emitted_even_when_empty` | The `ping` line contains `"params":{}`. Verified live: omitting it → `invalid_request: missing field params`. So `skip_serializing_if` must NEVER apply to `params`. |
| `id_is_a_string_and_is_never_correlated` | The id serializes as a string, and a reply with `"id":""` is accepted normally. Guards against anyone adding `assert_eq!(reply.id, sent.id)` later. |
| `connection_is_not_reused` | Mock answers one request and closes; two sequential calls both succeed (two dials). A client that pooled fails here exactly as it would live. |
| `oversize_request_is_rejected_client_side` | A body of `MAX_REQUEST_BODY_BYTES + 1` → `RequestTooLarge` **without dialing** (mock records zero connections); exactly at the cap it dials. |
| `result_wrapper_is_unwrapped_per_method` | `pane_read`→`.read`, `session_snapshot`→`.snapshot`, `agent_list`→`.agents`, `pane_list`→`.panes`; a wrong `type` yields `UnexpectedResult`, not a confusing serde error. |
| `read_visible_sends_source_visible_and_omits_lines` | Mock asserts the `pane.read` params carry `"source":"visible"` and **no** `lines` key; `read_visible_tail(n)` carries both. Encodes the type-level guarantee that `recent` is unreachable. |
| `write_ack_is_the_bare_ok_tag` | All three send methods map `{"type":"ok"}` to `WriteAccepted`. The type carries no delivery claim and the API offers no `bool` to mistake for one. |

### `tests/failure_paths.rs`

| Test | What it proves |
|---|---|
| `missing_socket_is_a_typed_error_not_a_panic` | `/nonexistent/herdr.sock` → `Connect`, `is_unreachable()`, `exit_code() == 3`, inside the connect timeout. PLAN.md's "herdr dies / socket gone" row — the offline twin of gate 6, and the case the old harness structurally could not test. |
| `directory_socket_maps_to_the_same_error` | A directory path (ECONNREFUSED, verified live) and a missing path (ENOENT) must both yield `Connect` / exit 3 — one operator message, not two. |
| `older_protocol_is_fatal_newer_is_a_warning` | Mock pong at 19 → `ProtocolTooOld`, `is_fatal()`, exit 4. At 21 → `Ok` with `Compatibility::ServerNewer{by:1}`. At 20 → `Exact`. Encodes "unknown additions are survivable, removals are not" as a test. Also: a pong with `capabilities` absent still parses. |
| `server_that_never_replies_hits_the_request_timeout` | Mock accepts and stays silent; with `request_timeout=200ms` → `Timeout` naming the method. The bridge cannot wedge on a wedged herdr. |
| `server_that_closes_early_yields_closed_early` | A 0-byte read → `ClosedEarly`, not an empty-string parse error. |
| `blank_id_and_echoed_id_errors_both_map_to_protocol` | The captured `{"id":"","error":{"code":"invalid_request",…}}` and `{"id":"probe","error":{"code":"pane_not_found",…}}` → `Protocol` with the right `ErrorCode`; `is_not_found()` true for the second, false for the first. |
| `unknown_error_code_becomes_other_and_keeps_the_message` | `{"code":"future_code_2027"}` → `ErrorCode::Other(...)` with the message intact. `ErrorBody.code` is an open string in the schema — a closed enum would mask a future error. |

### `tests/golden.rs`

| Test | What it proves |
|---|---|
| `snapshot_roundtrip_loses_nothing` | Deserialize the real 9,442-byte `fixtures/snapshot.json`, re-serialize into the envelope, compare as `serde_json::Value`. **Zero field loss.** This is gate 3 made offline and deterministic, so a modelling omission fails at `cargo test` on thev-box instead of only at the live proof on the lap. Expect one or two iterations here — that is the test doing its job. It is also where `terminal_title` / `terminal_title_stripped` decoding is proven, since normalize.jq drops them from the live diff. |
| `absent_optional_fields_do_not_serialize_as_null` | A pane with no `label`/`title`/`tokens`/`state_labels`/`display_agent` round-trips **without** those keys, and `state_labels: None` must not become `{}`. |
| `agent_status_unrecognized_round_trips_verbatim` | `"reticulating"` → `AgentStatus::Unrecognized("reticulating")` → re-serializes as `"reticulating"`, and is `!=` `AgentStatus::Unknown`. **Verified that `#[serde(other)]` fails this** — it emits `"unrecognized"`. |
| `pane_read_revision_is_zero_while_pane_info_revision_is_not` | Pins the asymmetry from the fixtures so a later reader cannot get it backwards. |

### `tests/schema_drift.rs`

Reads the checked-in 255,484-byte `fixtures/herdr-schema-p20.json`. This replaces the loudness
`deny_unknown_fields` would have given, sited in one place we control — a red `cargo test` naming the exact
field, on the operator's terms, instead of a missed ask at 2 a.m.

| Test | Assertion |
|---|---|
| `fixture_is_the_protocol_we_target` | `protocol == KNOWN_PROTOCOL`, `schema_version == 1`. |
| `every_method_we_call_still_exists` | Our nine names against `request.oneOf[].properties.method.const` (91 declared). **Comment required:** the schema is not a complete method list — the wire accepts 92 and `pane.graphics.stream` is missing from the dump — so presence is meaningful, absence needs a live check. |
| `every_result_tag_we_assert_still_exists` | `pong`, `session_snapshot`, `pane_read`, `subscription_started`, `agent_list`, `pane_list`, `ok` against the 58 `ResponseResult` tags. |
| `agent_status_variants_match_ours` | `[idle, working, blocked, done, unknown]` exactly; a new variant fails by name rather than silently becoming `Unrecognized` in production. Also `PaneAgentState == [idle, working, blocked, unknown]` (no `done`). |
| `required_fields_we_treat_as_mandatory_are_still_required` | `PaneInfo` 7 · `AgentInfo` 7 · `WorkspaceInfo` 8 · `TabInfo` 7 · `SessionSnapshot` 7 (incl. `layouts`) · `PaneReadResult` 8 — **and specifically that `state_change_seq` is still NOT required**, so the `Option` stays justified. |
| `only_three_subscription_variants_require_pane_id` | `pane.output_matched`, `pane.agent_status_changed`, `pane.scroll_changed`. If a global agent-status subscription ever appears, this test fails and tells slice 3 it can drop the fan-out. |
| `subscription_event_kinds_are_still_the_dot_form_three` | `SubscriptionEventKind == [pane.output_matched, pane.agent_status_changed, pane.scroll_changed]`, and `EventKind` is 26 snake_case names — the schema's own record of the two-family split. |

> The 27 `$defs` names that appear in more than one sub-schema compare as identical **only** after
> rewriting `#/schemas/<sub>/$defs/` → `#/$defs/`. Do that normalization inside the drift test or it
> red-flags on every run.

### Live-herd only (not `cargo test`)

`scripts/proof-slice1.sh` gates 0–6, and `scripts/proof-selftest.sh`. Both run on thev-lap by hand at the
slice's done-boundary. Mark this split in PLAN.md so a green `cargo test` is never read as a green proof.

---

## Build order

1. **Scaffold and prove the gates before any product code.** Create `Cargo.toml`, `rust-toolchain.toml`, both
   member manifests and stub `lib.rs`/`main.rs`. Run
   `env -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" cargo build` — **expect 66 packages, ~3.9 s**
   (measured with these exact manifests). Then `fmt`, `clippy`, `test`. Commit.
2. **Wire the gates and the ignores before any code can slip past them.** Append the three cargo gates to
   `.kickoff/lefthook-kickoff.yml` after `secret-scan`, with a comment recording *both* reasons the
   `env -u RUSTUP_TOOLCHAIN PATH=…` prefix exists. Add the `.gitignore` block. **Verify RED then GREEN**:
   deliberately mis-format a file, `git commit`, confirm the hook prints the rustfmt diff and blocks; fix,
   confirm it advances to the next gate and lands. Do not trust the YAML by reading it — the runner is a
   regex-subset parser, not lefthook.
3. **Capture the fixtures while the herd is up — before writing the decoder.** Write and run
   `scripts/capture-fixtures.sh`:
   - `herdr api schema --json > tests/fixtures/herdr-schema-p20.json` (expect 255,484 B, protocol 20)
   - `session.snapshot` → `snapshot.json` (~9.4 KB); `ping` → `pong.json`; `pane.read` visible on a pane that
     is **not** `$HERDR_PANE_ID` → `pane_read.json`
   - `errors.ndjson`: `pane.read` on `zz:p9` (echoed id) and `ping` with `params` omitted (blank id)
   - `events-mixed.ndjson`: **one** `events.subscribe` carrying `{"type":"pane.updated"}` plus
     `{"type":"pane.agent_status_changed","pane_id":"<a non-focused agent pane>","agent_status":"<its current
     status>"}`. The filtered form replays immediately and the roster backlog drains in ~3 s, so a 10 s
     capture reliably yields **both families on one connection**. Append the invented `pane_teleported` line
     by hand.

   **Safety:** `source:"visible"` only, ever. Never `pane.send_*`, never `agent.send`, never touch `wE:p1`.
   Do this first, so the decoder is written against real bytes rather than against the doc — every
   correction in this spec came from real bytes disagreeing with `HERDR_API.md`.
4. **`ids.rs` + `error.rs` + `transport.rs` + `tests/support/mod.rs`, bottom-up, tests first.** Build
   `MockHerdr` before anything that needs it. Land all of `tests/wire.rs` and `tests/failure_paths.rs`. Step
   proof: `cargo test -p herdr-client` green with nothing touching `$HOME/.config/herdr/herdr.sock`.
5. **`proto/model.rs` + `proto/response.rs`.** Model every property the live schema declares (counts and
   required lists are in the API section). Apply the serialization rule mechanically. Land `tests/golden.rs`
   and `tests/schema_drift.rs`.
6. **`client.rs` + `handshake.rs`.** The `Request` trait, `ping`/`handshake`/`snapshot`/`agents`/`panes`/
   `read_visible`/`read_visible_tail`. Every `Response` models its own nested wrapper. No `focus`, no
   `agent.prompt`, no public `ReadSource`.
7. **`proto/event.rs` + `stream.rs`.** Write `decode_event` **before** the `Event` enum, so the decoder's
   shape drives the types rather than the reverse. Assert `roster_event_discards_pane_info` explicitly. Put
   both invisible traps into the crate-level docs and onto `subscribe` itself.
8. **`keys.rs` + the three write methods**, mock-tested only, under the `UNVERIFIED-ON-P20` banner. Do **not**
   call them against the live herd from any code path, and expose no subcommand that reaches them. Write
   `scripts/verify-send-p20.sh` now, while the reasoning is fresh, so slice 3 inherits a runnable procedure
   rather than a paragraph.
9. **The binary.** `main.rs` (clap, tracing-subscriber, exit-code mapping) + `cmd/{status,read,doctor,watch}.rs`
   + `render.rs`. `status --json` and `read --json` emit the **full** envelope re-serialized from the typed
   structs. Verify by hand against the live herd before automating.
10. **The proof harness.** Copy `proof-slice1.sh`, `normalize.jq` and `mock-herdr.py` from
    `$SCRATCH/scratchpad/proof/`
    — gates 0/1 already run green against the live herd there and the drop list is already drift-verified at
    0/5 intervals. Add `scripts/fakes/` (rewriting `fake-wireorder.sh`, which is byte-identical to
    `fake-honest.sh` in the older scratch set) and `scripts/proof-selftest.sh`.
11. **Run the self-test first, then the proof.** `proof-selftest.sh` must show 5 PASS / 3 FAIL, and
    `fake-cheat.sh` must FAIL at gate 2 — if it passes, the sandbox is broken and the whole proof is theatre.
    Then the full command. If gate 3 fails, read the printed drop list before touching the client: the cause
    is far more likely a modelling omission (a field herdr emits that we drop) than a decode bug, and the diff
    names it.
12. **PLAN.md edits — four, all correcting things now known to be wrong.**
    - **Line 143, proof cell** → `` `./scripts/proof-slice1.sh` exits 0 — seven gates: reference sanity
      (protocol 20), non-vacuity, sandboxed client under a stripped PATH, sandwich-diffed
      `herdr-tg status --json` vs `herdr api snapshot` through `scripts/normalize.jq`, `pane.read` text
      parity, event decode via a filtered-status replay, and the negative paths (missing socket → exit 3,
      protocol 19 → exit 4). NOT `herdr status`, which prints versions, not the herd. `` Add: *the status
      diff proves `session.snapshot`; gate 4 proves `pane.read`; gate 5 proves `events.subscribe`;
      `pane.send_text` / `send_keys` / `send_input` are built but have **no live proof** in slice 1.*
    - **Line 143, the word "handshake"** → *"`ping` handshake: assert `protocol >= 20`, record version and
      capabilities, fail closed below the minimum"* — there is no handshake method in protocol 20.
    - **Lines 144 vs 156, the token contradiction** → resolve to **TOML for structure, env for the secret**:
      `herdr-tg.toml` carries workspace name, socket path, chat allowlist, quiet hours and pane scope and is
      git-ignored; the token itself is read from `HERDR_TG_TOKEN`, which keeps the credential out of the file
      that gets copied around and makes the systemd unit's `EnvironmentFile=` the natural delivery path.
      Whichever way it lands, **name the key `token` or `bot_token`** — `scan-secrets` has no Telegram pattern
      and catches it only via `(secret|token|password|api[_-]?key|…)["']?\s*[:=]\s*["'][^"'\s]{12,}`; named
      `bot`, `id` or `key` it is invisible and a leak reaches origin.
    - **New failure-table row** → *"Sticky target pane was MOVED | `pane_moved` event | the pane_id changes
      and the old one stops resolving even though the agent is alive and the pane is not closed; the event
      carries `previous_pane_id` plus the full new `PaneInfo`, so migrate sticky state silently rather than
      falling back to the picker."*
    Also fix `README.md:44`.
13. **Commit and push on explicit go (D6).** Then re-run the proof cold from a clean shell in front of the
    operator.
14. **Write the durable, non-obvious facts to project memory**, so the next session inherits rather than
    re-derives: the two envelope families; no global agent-status subscription; the filtered-replay trick;
    the `pane.updated` backlog; `ok` carries no delivery semantics; the missing-newline hang; one-shot RPC;
    `PaneReadResult.revision` is a stub while `PaneInfo.revision` indexes the replay backlog; `#[serde(other)]`
    destroys the wire string; `jq -r` adds a newline (the "trailing newline" delta was never real);
    a full visible read is `viewport_rows − 1` lines; mise's global `RUSTUP_TOOLCHAIN` makes
    `rust-toolchain.toml` inert; `herdr status` is not the herd.
15. **The probe-session checklist — before slice 3, never against the operator's herd.** `herdr --session probe`
    (verified a real flag), open a plain shell pane, and settle five things this spec is careful not to lean
    on: (1) the `pane.send_keys` key grammar on protocol 20, including the per-harness submit key; (2) the
    success tag of the three send methods (`ok` is the only void tag among 58, but the schema does not map
    methods to results); (3) whether `pane.send_input` frames its text in bracketed paste — multi-line
    Telegram replies are this product's default case; (4) whether a **filtered** subscription fires on later
    transitions *into* the filtered status or is catch-up-at-subscribe only; (5) whether `state_change_seq`
    churns while a pane sits blocked.

---

## Risks and what is deliberately out of scope

### Risks

- **The send path has no live proof in slice 1 — the plan's largest honesty gap.** Three specific unknowns:
  the key grammar is p16 evidence and is unprobeable without typing into a real pane; the `{"type":"ok"}`
  success tag is inferred from it being the only void tag among 58; and whether `send_input` frames text in
  bracketed paste is unknown, which matters because `HERDR_API.md`'s 0.7.4 finding is that `send_text` writes
  **raw** bytes — so a `\n` inside text is a real Enter, and a multi-line Telegram reply could execute
  line-by-line in the operator's terminal. Mitigated structurally (no live call site, no subcommand) and by
  step 15; PLAN.md must say a green proof does not imply working send methods.
- **`pane.updated` is the only globally-subscribable status-bearing event and it replays history.** Mitigated
  structurally — `RosterEvent` cannot express a status — but the replay is also a **rolling, ageing** window
  delivered at ~100 ms/frame, so a reconnect costs seconds before the stream goes live, dedupe-by-identity
  cannot work across a real outage, and slice 3's re-snapshot reconcile must tolerate arriving **before** the
  replay finishes.
- **The subscription fan-out is materially more scope than PLAN.md states, and it lands on slice 3.** No global
  agent-status subscription, no `events.update`, so a new pane forces a stream teardown and re-subscribe — and
  the roster set must itself be kept current via `pane.created`/`agent_detected`/`closed`/`exited`/`moved`.
  One connection carries many subscriptions, which keeps it tractable. Re-estimate slice 3 with this in it.
- **The dedupe key is unsound in the direction that matters.** `state_change_seq` is not in the event payload
  (an extra RPC per event, with a race), is `default: 0` and not required, and its granularity relative to
  delivered events is unestablished. If it churns while a pane sits blocked, a resubscribe replay after a
  reconnect sees a different seq for the same unanswered ask and **pushes again** — the anti-spam contract
  broken exactly when the operator can least tolerate it. Slice 1 exposes the field and takes no position.
- **A filtered subscription may be catch-up-only.** Verified firing at t=0.00 for an already-matching pane, on
  two panes and two statuses. **Not** verified: whether it fires again when a pane *later* enters that status —
  the herd stayed static through every window anyone has held open. Gate 5 needs only the replay, so slice 1
  is safe; but if it is replay-only, a bridge built on it alone would deliver each ask once at startup and
  then go silent forever. Slice 3 must pair filtered with unfiltered until step 15 settles it.
- **`done` is unproven end to end.** PLAN.md's second push trigger has never been observed on this host: no
  detection manifest emits it, `pane.report_agent` cannot report it, and the `seen` bit herdr derives it from
  is **not readable** from any API the bridge uses. So the bridge cannot observe whether a pane is seen,
  cannot predict whether an idle pane will surface as `done`, and cannot verify the never-call-focus rule
  held. Mitigated by the permanent absence of any focus method; slice 3 needs one real observation.
- **`live_handoff` is advertised `true` and its effect on a held event stream is untested.** herdr can replace
  its own binary underneath a running bridge without the socket path changing. Nobody has checked what that
  does to an open `events.subscribe` connection, whether the socket inode is replaced, or whether the backlog
  survives. This is PLAN.md's "bridge restarted" and "laptop asleep" rows. Mitigated by re-running the
  handshake on **every** reconnect and by making disconnect observable.
- **Gate 3 couples the proof to our modelling choices.** A field herdr adds that we do not model turns the diff
  red. That is the intended drift alarm, but it means a routine `herdr update` can redden the proof for a
  benign reason. Mitigated: `schema_drift.rs` fires at `cargo test` first, the failure prints the field, and
  the normalizer already excludes every volatile family so only genuinely new *content* can trip it.
- **Rejecting `deny_unknown_fields` trades loudness for uptime, against PLAN.md's stated value.** `PaneInfo`
  gained 7 fields and `AgentInfo` 5 between p16 and p20; a p16-era client with that attribute would have
  hard-failed on **every** snapshot the moment the operator ran `herdr update` — under `Restart=always` +
  `StartLimitIntervalSec=0`, an infinite crash loop on a machine whose operator has only a phone. That is
  precisely the D1 failure class. Residual risk: a field herdr *renames* becomes a silent `None`, caught only
  if it is in a `required` list the drift test asserts.
- **Gate 0 pins protocol 20 in two places** (the reference and the client), so herdr 21 fails the proof for two
  independent reasons with one reported. Gate 0's message names both and points at `refresh-schema.sh`.
  Related: the whole proof depends on `herdr api snapshot`, an `api` subcommand whose stability across herdr
  versions is unestablished.
- **Snapshot atomicity is unprobed** — the one thing that could make gate 3 flake unfixably. If the *reference*
  can tear, retries multiply the flake rather than fix it. Nothing observed suggests it does (6 consecutive
  normalized samples were identical), but it is unproven. Tell: if gate 3 ever needs attempt 3+, investigate
  this before raising `ATTEMPTS`.
- **Gates run on the working tree, not the staged index.** An unstaged half-finished file anywhere blocks every
  commit via `clippy --workspace --all-targets`. No config fix exists — the runner is a regex-subset parser.
  Workaround: `git stash -k`.
- **There is no CI, and no supply-chain gate.** No `.github/` exists; every gate is a local hook that
  `LEFTHOOK=0 git commit` bypasses by design, on one laptop, and `origin` is reachable so pushes land ungated.
  Separately, neither `cargo audit` nor `cargo deny` is installed or proposed, for a binary that will hold a
  Telegram bot token and type keystrokes into live agent terminals. The whole offline suite is designed to run
  with no herdr socket precisely so CI is fixable later without rework — but it is not fixed by slice 1.
- **The lint surface floats.** `channel = "stable"` + `clippy -D warnings` means the gate's definition of
  "warning" changes on every rustup update, and rustfmt's `style_edition` tracks edition 2024. For a project
  whose thesis is "no runtime to drift", this is the one vector left unpinned, and the likeliest cause of a
  future mysterious hook failure. Accepted because a hard version pin costs a ~120 MB download on a fresh
  clone; the fix if it bites is a `[workspace.lints]` table plus a pinned channel, not more `#[allow]`s.
- **Two scratch artifacts are traps for a builder who copies instead of reading.**
  `$HOME/.cache/tmp/proof/normalize.jq` keeps `terminal_title_stripped` (live-volatile) and
  `.../scratchpad/final-lefthook.yml` uses the hardcoded `$HOME/.cargo/bin/cargo` form that does **not**
  unset `RUSTUP_TOOLCHAIN` and breaks on thev-box. Use the corrected versions named in this spec.

### Out of scope

- **Telegram, entirely** — no teloxide dependency (the line ships commented out with its verified feature
  string and rationale), no bot token, no chat allowlist, no TOML config, no `/status` command. Slice 2. This
  is why slice 1's graph is 66 packages / 3.9 s rather than 222 / ~15 s, and why teloxide's 13-month staleness
  is not yet on the critical path.
- **Any live write into the operator's herd.** The three send methods are built and mock-tested; the binary
  exposes no subcommand that reaches them. Their live verification is `scripts/verify-send-p20.sh`, which
  refuses to run unless a throwaway probe-session socket is targeted.
- **The subscription fan-out *policy*, the reconnect loop, the backoff, and the single-recovery-notice rule.**
  Slice 1 ships the primitives (the `Subscription` type with `pane_id` unrepresentable-if-missing, an
  `EventStream` that carries its subs and terminates observably, the roster variants). The loop is slice 3's,
  and lives in the binary because it is policy.
- **The notification-discipline state machine** — debounce, `(pane_id, state_change_seq)` dedupe, retraction,
  quiet hours, digest batching, the pushes-per-hour cap. Slices 3–4.
- **Sticky routing**, the reply-to correlation, the switcher keyboard, the target picker, and the atomic-JSON
  state file. Slice 3. Slice 1 adds only the `PaneMoved` data a later slice needs, plus its PLAN.md row.
- **Ask extraction.** Slice 1 exposes `read_visible` / `read_visible_tail` and nothing more. Two off-by-one
  traps for whoever sizes the excerpt: `lines` counts **newlines**, and a full visible read returns
  `viewport_rows − 1` of them; and `truncated` is true for any `N` below the full height regardless of whether
  content was actually lost.
- **The append-only keystroke audit log** and the **systemd unit** with its hardening. Slice 4. Their
  `.gitignore` lines land in slice 1 because the files must never be trackable before they can exist.
- **`agent.prompt` and the split reply path.** An idle agent gets `agent.prompt`; a blocked agent cannot
  (`agent_blocked` before any input is sent) and must go through `send_input`/`send_keys`. That fork lands in
  the exact case the product exists to serve and is not in PLAN.md today — it is slice 3's, flagged here.
- **`pane.focus` / `agent.focus` — permanently, not deferred.** See delta #26.
- **`recent` / `recent_unwrapped` / `detection` read sources.** `ReadSource` is `pub(crate)`; there is no
  public path. If a later slice genuinely needs scrollback harvesting it must be an explicitly-named,
  user-initiated call site, never reachable from a timer.
- **Modelled layout types.** `layouts` is carried as `Vec<serde_json::Value>` so the round trip is lossless
  and normalized out of the proof. The client is explicitly **not** proven to parse `PaneLayoutSnapshot`.
- **Codegen from `herdr api schema --json`.** It under-declares (91 methods vs 92 on the wire;
  `pane.graphics.stream` missing) and over-declares (`EventMatch` lists 19 variants while `events.wait` rejects
  all but one). Hand-write the ~11 types; keep the schema as a drift-test **fixture**, never a source.
- **~82 of the 92 wire methods**, `events.wait` (supports only agent-status matches and reports timeout as an
  error), `strip_ansi` (no observable effect on `pane.read` output), `pane.report_agent`, and anything that
  mutates the herd's shape.
- **Live-herd tests inside `cargo test`** — structurally excluded, so the crate is gateable on thev-box.
- **Greening the lap's kickoff gates beyond the three cargo gates** (D6: real but separate work), and the four
  cosmetic `grep: warning: stray \ before -` lines `scan-structure` emits (pre-existing engine noise — worth a
  one-line note upstream to claude-kickoff so it is not later misread as a herdr-tg regression).

---

## Open questions for the operator

Three. Everything else in this document is a decision already made.

1. **Your hands, before slice 3: the probe session.** Step 15 needs `herdr --session probe` — a second,
   throwaway herdr session started while you are at the machine — to settle the `pane.send_keys` key grammar,
   the per-harness submit key, whether `send_input` frames text in bracketed paste, and whether a filtered
   subscription fires on later transitions. None of it can be done against your live herd without typing into
   real agent panes. It does not touch your existing session, but it needs your go-ahead and ten minutes of
   your presence. **Slice 1 does not need it; slice 3 cannot start without it.**

2. **A values call: should the bridge refuse to start against an older herdr?** This spec sets
   `MIN_SUPPORTED_PROTOCOL = 20` — a herdr you downgraded, or one not yet upgraded, makes the bridge exit 4
   rather than run degraded. The argument for it: a missing method surfaces as `unknown variant`, which the
   client can detect but not repair, and running degraded means **silently missing asks** — worse than
   refusing to start. The argument against: a bridge that refuses to start is a bridge that is not there when
   you only have a phone. Setting it to 16 with a loud degraded-mode warning is defensible. This is about how
   you want the thing to behave at its worst moment, so it is yours.

3. **Confirm the token resolution before slice 2 writes a config loader.** PLAN.md says both `.env` (line 144)
   and `herdr-tg.toml` (line 156), and only the `.env` path is git-ignored today. The recommendation is
   structure in a git-ignored `herdr-tg.toml`, the secret itself in `HERDR_TG_TOKEN` delivered by the systemd
   unit's `EnvironmentFile=`. It is your credential; say yes or name the other one, and the key gets called
   `token` either way so the existing secret scanner covers it.

No money is spent, nothing irreversible is touched, and no external service is contacted by slice 1.
