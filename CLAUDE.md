<!-- kickoff:begin backup-pre-redact-30-g54cc520 -->
@.kickoff/KICKOFF.md
<!-- kickoff:end -->

# herdr-tg

One Telegram bot, one forum, one topic per project. A project's agent connects to a Unix socket and
says what it is doing and what it is asking; the operator reads it on his phone and taps an answer,
which arrives back in the agent's own turn.

**This binary cannot type into a terminal.** Not "does not by default" — cannot. The path that read
rendered panes and sent keystrokes was deleted, and the guard that used to permit one audited call
site now forbids naming a write RPC anywhere.

## Layout

Three crates in one Cargo workspace.

- `crates/hub-proto` — the wire contract between the hub and an adapter. NDJSON over `AF_UNIX`, nine
  frames up, six down. Knows nothing about herdr, kickoff, claude or panes, and must not learn.
- `crates/herdr-tg` — the bot. A `clap` binary: `enroll`, `projects`, `serve`, plus four read-only
  herdr subcommands (`status`, `read`, `doctor`, `watch`). **It binds nothing** — no listening port,
  and the Unix socket is not one.
- `crates/herdr-client` — the typed client for herdr protocol 20. Used ONLY by the read-only
  subcommands now; the bot does not talk to herdr at all.

`plugins/kickoff-channel/` is the Claude Code adapter: an MCP channel plugin that dials the socket.
It holds no token, no allowlist and no model.

Docs, in the order they are worth reading: `docs/INTERFACES.md` (the four seams and the closed list
of what this project does), `docs/HUB-AND-KICKOFF.md` (how it wires to kickoff, and two questions
still open), `docs/HUB-DESIGN.md` (the original redesign — historical, and it predates the
deletion), `docs/SLICE-3-REVIEW.md` and `docs/SLICE-3-FIXES.md` (why the scraper died).

## Build and test — all three parts are required

```
env -u RUSTUP_TOOLCHAIN TMPDIR=<a real absolute dir> PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace
```

`PATH` because mise shims hide cargo. `env -u RUSTUP_TOOLCHAIN` because mise exports it globally and it
overrides `rust-toolchain.toml`. `TMPDIR` because an agent session inherits it as the literal string
`%h/.cache/tmp`, which fails seven `herdr-client` transport tests for a reason you did not cause.

The same applies to `git commit`: the pre-commit hook runs six gates in your environment, so prefix
the commit too, or it is refused with seven red tests you did not break.

Two tests are `#[ignore]`d because they need bun. `scripts/install-channel-plugin.sh` runs them, and
refuses to install a bridge that disagrees with the hub:

```
cargo test -p herdr-tg the_real_plugin -- --ignored
```

## The domains, and who owns them

| Domain | Owner | Lives in |
| --- | --- | --- |
| the wire contract | `wire-protocol` | `crates/hub-proto/` |
| the socket, identity, claims, the tap ledger | `write-safety` | `hub.rs`, `registry.rs`, `no_live_write_call_site.rs` |
| what leaves this machine | `egress` | `summarize.rs`, the de-identification guard |
| the operator's channel | `operator-channel` | `bot.rs`, `surface.rs`, `queue.rs` |

`herdr-client` is in maintenance: it serves four read-only commands and nothing else.

## The state of the repo

- **The hub is the product.** Slice 1 is functionally complete: identity, presence, delivery,
  resolution, retirement, alarm. `an_ask_becomes_a_tap_becomes_a_choice` passes, and so does
  `the_real_plugin_and_the_real_hub_agree_on_the_wire` — the real bun bridge against the real hub
  over a real socket, both directions, with only Telegram faked.
- **The round trip with a live Telegram has run**, on 2 September 2026: a question left an agent's
  turn, reached the operator's phone, and his tap came back as a message in that same turn. It needs
  a session started with `claude --channels plugin:kickoff-channel@herdr-tg-local`; a `claude -p` run
  cannot do it, because print mode ends the turn and the bridge dies before the tap arrives.
- **Two projects are enrolled.** `herdr-tg` and a throwaway, `~/scratch/hub-dogfood`, which proved a
  project other than this one can hold a topic of its own. `docs/MULTIPLEXER-READINESS.md` is the
  audit of what still breaks between two projects and fourteen.
- **The screen-scraper is deleted, not disabled.** `permission.rs`, `deliver.rs`, `mirror.rs`,
  `voice.rs`, `notify.rs`, `audit.rs` and `routing.rs` are gone, along with the `HERDR_TG_PANES`
  flag that briefly gated them. `there_is_no_way_from_telegram_to_a_keyboard.rs` pins the deletion.
- **The watchdog is live** and shares no code or process with the hub. It arms the first time
  something stamps `~/.local/state/herdr-tg/hub.heartbeat`.
- `fix/r5-parser` is **dead**: it improved the screen parser, and there is no screen parser.
- The tracker shim `.kickoff/bin/mc` is **dead** in this repo. Report status in chat.

## The quality bar

The conventions this repo actually holds to. Every specialist charter's CANON block points here.

- **Comments explain WHY**, in plain words, usually naming the failure the code prevents. They do not
  narrate what the line does. Read three neighbouring files before writing one.
- **Test names are full sentences** describing a property:
  `a_question_answered_once_can_never_be_answered_twice`, not `test_tap_2`.
- **Operator-facing strings carry no jargon** — no project ids, no enum names, no "None", no "parse".
- **Fail closed.** When the code cannot prove what it is about to do is right, it refuses and says so.
- **RED before GREEN.** A regression test that never failed proves nothing. Watch it fail for the
  right reason first.
- **A fix nobody attacked is a draft.** Three review rounds on the hub found 19, 7 and 9 distinct
  defects; rounds two and three each found defects introduced by the previous round's fixes. Gates
  are necessary, not sufficient.
- **Write and read back after every edit.** A patch script that aborts halfway writes nothing, and
  the commit message still claims it. That has happened here.
