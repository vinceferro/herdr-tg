<!-- kickoff:begin backup-pre-redact-30-g54cc520 -->
@.kickoff/KICKOFF.md
<!-- kickoff:end -->

# herdr-tg

A Telegram front door for a herd of coding agents. It watches panes through the herdr daemon,
pushes what an agent says — and what it is asking — to the operator's phone, and can send a reply
back into the pane the operator meant.

**The write half is the dangerous half, and it is currently under review.** See "The state of the
repo" below before touching anything in it.

## Layout

Two crates in one Cargo workspace.

- `crates/herdr-client` — the typed client for herdr protocol 20. NDJSON over a unix socket, one-shot
  RPC plus one event stream. Fixture-pinned against a real herd.
- `crates/herdr-tg` — the bridge itself. A `clap` binary: four read-only subcommands plus `serve`,
  which is the Telegram bot. **It binds nothing** — no listening port, deliberately.

Docs worth reading before you touch the code: `docs/SLICE-3-REVIEW.md` (the adversarial review that
stopped the service), `docs/SLICE-3-FIXES.md` (what closing it cost and what is still open), and
`docs/HUB-DESIGN.md` (a proposed redesign — a proposal, not a decision).

## Build and test — all three parts are required

```
env -u RUSTUP_TOOLCHAIN TMPDIR=<a real absolute dir> PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace
```

`PATH` because mise shims hide cargo. `env -u RUSTUP_TOOLCHAIN` because mise exports it globally and it
overrides `rust-toolchain.toml`. `TMPDIR` because an agent session inherits it as the literal string
`%h/.cache/tmp`, which fails seven `herdr-client` transport tests for a reason you did not cause.

The same applies to `git commit`: the pre-commit hook runs five gates in your environment, so prefix
the commit too, or it is refused with seven red tests you did not break.

The five gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`, and
`.kickoff/bin/scan-secrets --staged`.

## The domains, and who owns them

| Domain | Owner | Lives in |
| --- | --- | --- |
| the herdr wire protocol | `wire-protocol` | `crates/herdr-client/` |
| the audited keystroke path and its guard | `write-safety` | `deliver.rs`, `audit.rs`, `no_live_write_call_site.rs` |
| what leaves this machine | `egress` | `summarize.rs`, the de-identification guard |
| the operator's channel | `operator-channel` | `bot.rs`, `routing.rs`, `render.rs`, `voice.rs`, `notify.rs` |

**Deliberately unowned: screen interpretation** (`permission.rs`, `mirror.rs`). Five adversarial
rounds established that reading a rendered pane to decide what it means is ill-posed against herdr's
protocol, which carries agent status and raw bytes but nothing structured about what an agent is
asking. If the operator does reopen it, the standing lesson is
`.kickoff/memory/build-a-two-sided-corpus-before-tuning-a-classifier.md`: build the corpus from both
sides, and from real captures, before touching a rule — four rounds each traded a false negative for
a false positive because one real screen was pitted against seventeen imagined ones.
`docs/HUB-DESIGN.md` proposes deleting both files. Do not staff it; do not iterate on it
without asking the operator.

## The state of the repo

- **The service is STOPPED and stays stopped** until the operator restarts it. Nothing here assumes
  otherwise. Do not start it to test something.
- `main` carries all seven original blockers closed and verified — 352 tests, up from 232.
- `fix/r5-parser` is **unmerged on purpose**: it improves the parser and a sceptic still broke it.
  Merging it is a decision that depends on whether the write path survives at all.
- The tracker shim `.kickoff/bin/mc` is **dead** in this repo. The pinned core dropped
  mission-control from the public line, and the shim's "engine not present" message misdiagnoses it.
  Report status in chat; do not chase it as a broken install.

## The quality bar

The conventions this repo actually holds to. Every specialist charter's CANON block points here.

- **Comments explain WHY**, in plain words, usually naming the failure the code prevents. They do not
  narrate what the line does. Read three neighbouring files before writing one.
- **Test names are full sentences** describing a property: `a_two_option_dialog_is_never_recognised`,
  not `test_parse_2`.
- **Operator-facing strings carry no jargon** — no pane ids, no enum names, no "parse", no "None".
- **Fail closed.** When the code cannot prove what it is about to do is right, it refuses and says so.
  A refused reply is a small annoyance; a wrong keystroke in someone's terminal is not.
- **RED before GREEN.** A regression test that never failed proves nothing. Check it out against the
  parent commit and watch it fail for the right reason first.
- **A fix nobody attacked is a draft.** Every one of five review rounds was green on all five gates
  before a sceptic broke it. Gates are necessary, not sufficient.
