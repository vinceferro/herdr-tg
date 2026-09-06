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

`plugins/kickoff-channel/` is the MCP tool server — **both** engines start it. It holds no token, no
allowlist and no model. Three files beside it are the attach surface, and **all three adapters share
all three**: `attach.ts` is the only file in the repo that reads a configuration variable, `where.ts`
answers what the machine says once a directory has been named, and `hub-link.ts` is the wire. One
copy of each, because the one time this project had two copies of the link, the copy drifted by
thirteen already-fixed defects — and a test now fails if any adapter starts writing its own again.

`adapters/kickoff-hub-attach/` is **the** adapter — one command that is all three jobs an opencode
worker used to hand-start. It holds the claim and opens a door (the relay, `relay.ts`); it watches an
opencode server and relays the prompts the agent did not choose (`opencode.ts`, an in-process
producer at its own door); and with `--run` it starts the engine as its child so a wall has one
entrypoint (`run.ts`). The tool server (what the agent chose to say) and the watcher both attach to
the door over a local socket speaking hub-proto unchanged, so **the hub never learns there were
several**. `--check` proves an environment can reach the hub without creating a topic. There is one
thing to run under `adapters/`, on purpose.

**One namespace, `KICKOFF_HUB_`, and one document.** `docs/ATTACHING.md` is the contract an adapter
attaches by — eight variables of which a normal adopter sets one, the address a dispatcher mints, the
credential rule, the handshake, and the twelve wire rules; §13 is `kickoff-hub-attach` itself. It is
written to be implementable by a stranger, and `docs/examples/attach-from-the-document.ts` is a
stranger's adapter that imports nothing of ours and is run against the real door by attach's own suite.

Docs, in the order they are worth reading: `docs/ATTACHING.md` (how anything attaches),
`docs/CAPABILITIES.md` (what the hub offers, requires and refuses),
`docs/INTERFACES.md` (the four seams and the closed list of what this project does), `docs/HUB-AND-KICKOFF.md` (how it wires to kickoff, and two questions
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

Twelve tests are `#[ignore]`d — ten need bun (one of those runs the plugin from before
conversations existed against the new hub, one dispatches three rooms with the real adapter), one
is a proxy-driven child, one runs the pre-change bridge against the new hub. `scripts/install-channel-plugin.sh` runs them, and refuses to install
a bridge that disagrees with the hub:

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
- **Three projects are enrolled**, two of them throwaways: `herdr-tg`, `~/scratch/hub-dogfood`, and
  `~/scratch/oc-dogfood`. `docs/MULTIPLEXER-READINESS.md` is the audit of what still breaks between
  two projects and fourteen.
- **A lane is its own addressable thing, and it is proven live.** On 3 September two git worktrees
  of one throwaway ran at the same moment and each got its own forum topic beside the project's,
  both delivering. Before that, the second was refused. The bridge takes the lane from git's own
  worktree name, which git guarantees unique — never the folder basename, which is not.
- **The opencode adapter, the relay and the entrypoint are one command: `adapters/kickoff-hub-attach/`.**
  `--opencode <url>` watches a server and maps `question.v2.asked` and `permission.v2.asked` onto
  `ask`, a tap back onto opencode's own reply endpoints — the old event bridge, now an in-process
  producer at attach's own door. The door itself is the old fan-in (`relay.ts` + `ledger.ts`), moved
  in unchanged. `--run` starts the engine as attach's child so a wall has one entrypoint; `--check`
  proves reachability and makes no topic. `adapters/fanin/` and `adapters/opencode-bridge/` are
  **gone** — a stranger opening `adapters/` finds one thing to run. Every check the two suites held
  survives, moved to attach; the systemd template is `deploy/kickoff-hub-attach@.service`. It ran for real on 5 September: enabled for the opencode throwaway it held the claim,
  started the engine as its child, attached the watcher, and created no topic.
- **Typed steering reaches an opencode worker** (5 September). The watcher carries a `message` to
  `POST /session/{id}/prompt_async` verbatim, in the session the server lists for the project
  directory; a reply typed under a question — the hub sets `in_reply_to_ask` from the message he
  swiped to reply to, for a question this conversation's live session asked — goes to the session
  that asked and leaves the question open for the tap. Every `message` is answered
  `ack{status, reason?}`, and **the hub now reads it**: a `refused` becomes one line in the topic he
  typed in, under the line it refuses. It read the status of no ack before. On an engine that cannot
  read a channel message the tool server refuses typed words on the wire, and attach's door folds
  several producers' answers into the one the hub hears. Every request the watcher makes of opencode
  has a deadline; words opencode took and the agent then could not act on are said so in the topic.
- **Nothing a bridge says before its pong is destroyed unanswered** (5 September). The hub holds
  65 frames before the pong — the 64 `hub-link.ts` may have queued plus the one it puts back at the
  head of its queue on close, which is what it carries into a reconnect; it
  was 256 KiB, and a bridge with an afternoon's backlog was refused on every redial — and on every
  way a connection is refused before it is live, each frame the hub read is acked `no` first. The
  bridge's half: a frame flushed whole on a connection that ends is handed back as unconfirmed
  (`onUnanswered`), never kept in silence and never re-sent. The bridge from before this change is
  run against the new hub by `a_bridge_from_before_this_change_still_works_against_the_new_hub`.
- **A conversation is a row, and its secret lives outside every repo** (6 September;
  `docs/CONVERSATIONS.md` built through step 6). `herdr-tg open <repo>` mints a project's
  conversation writing nothing into the repo; `grant <repo> --rooms N` mints rooms — siblings of
  the seed, in its repo, each its own secret and topic, sixteen vacant at once, taken by a
  dispatcher renaming a slot in `<state>/grants/<seed>/` and setting `KICKOFF_HUB_CONVERSATION`;
  `adopt-secrets --apply` copies the three enrolled projects' secrets across once, never writing
  `projects.json`; `remove-repo-secret <repo>` is step 7's verb, per project, at his hand, and
  has NOT been run on any real repo. **Before either runs on the real box, install this build
  where the shell finds `herdr-tg` (`~/.cargo/bin` shadows `~/.local/bin` on PATH; both got this build on 6 September) and
  restart the hub on it**: an older `enroll` rewrites the repo's copy alone and leaves the
  channel's stale, and every new session then presents the stale one. The bridge answers "which
  conversation am I" by a four-term ladder (`attach.ts`) — told, bound (a link looked for on the
  legacy walk, so a folder with no git or a project below a repo's top is found), legacy, refuse
  — and the relay's door is keyed on the conversation. A rotation rewrites the secret where it
  already lives and puts no token back into an opened project's tree. `conversations.rs` owns the
  home: 0700 re-asserted, ids shape-refused at every door, containment asserted before every
  write.
- **The screen-scraper is deleted, not disabled.** `permission.rs`, `deliver.rs`, `mirror.rs`,
  `voice.rs`, `notify.rs`, `audit.rs` and `routing.rs` are gone, along with the `HERDR_TG_PANES`
  flag that briefly gated them. `there_is_no_way_from_telegram_to_a_keyboard.rs` pins the deletion.
- **The watchdog is live** and shares no code or process with the hub. It arms the first time
  something stamps `~/.local/state/herdr-tg/hub.heartbeat`.
- **A project has an off switch, at the terminal only.** `herdr-tg disable <repo>` writes the flag
  for the project's seed AND every room of it (a room's id in place of the folder switches that
  one room);
  the hub watches the registry file and, within a second, drops a LIVE connection's claim and ends
  it — `refused{not_enabled}` first, then `no` for every frame it had queued, the one waiting for
  its turn included, then the close; only a message already mid-send is finished — and it does so
  even when the loud bridge's backlog has the read loop parked on a full queue. `/projects` on the
  phone says "switched off". `enable` is the way back, and re-enrolling keeps the switch where it
  was. No Telegram command can do this; the command set is pinned as a closed list of two.
- **`herdr-tg projects --json` is built.** `connected` (the project's own voice) and
  `connected_lanes` (its addresses live now) come from `hub.connected.json`, which the hub rewrites
  on every claim and release under its own pid — and unlinks when a rewrite fails — and the command
  believes it only when the lock's holder is alive, is a herdr-tg, and wrote it — otherwise `null`.
  A registry that cannot be read is refused, never printed as `[]`. See `presence.rs`.
- **His typed line carries a reaction for the stage it reached**: 👀 when the hub handed it on,
  👍 when the bridge acked it, 👎 (plus the line saying why) when the bridge refused it. The Claude
  tool server acks `accepted` the moment the words are in the agent's turn, so the thumb lands on
  that engine too. The eyes land in a task of their own; `relay` never waits on Telegram. Measured
  5 September (`docs/RATE-PROBE.md` §3): reactions are not charged against the send ceiling, have a
  twenty-a-minute ceiling of their own — the hub keeps a ledger for it and stops asking past
  eighteen — and `✅`/`❌` do not exist as a bot's free reactions. A reaction Telegram refuses for
  anything but the ceiling is said once at warn in the journal.
- **A line that does not open with a slash is never a command.** The parser splits the first word
  at `@` before it looks for a slash, so an `@mention`, an email or an ssh remote read as another
  bot's command and were dropped in silence; `what_he_typed` guards on the slash first.
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
