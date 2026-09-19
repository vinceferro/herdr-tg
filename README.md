# Kickoff Channel

One Telegram bot, one forum, one topic per conversation. An agent connects to a Unix socket on the
same machine and says what it is doing and what it is asking; the operator reads it on his phone and
taps an answer, and that answer arrives back inside the agent's own turn.

It is a **control surface, not a console.** This binary **cannot type into a terminal** — not "does
not by default", cannot. The path that drove a terminal was deleted rather than switched off, and
two guards hold the deletion open: `crates/herdr-tg/tests/there_is_no_way_from_telegram_to_a_keyboard.rs`
fails the build if the modules or the switch come back, and
`crates/herdr-client/tests/no_live_write_call_site.rs` fails it if a write RPC is so much as named
in any Rust file in the workspace.

## Telegram is a sparse control surface

The phone is where **decisions, alerts, summaries and lifecycle outcomes** go: a question with
buttons, a line the operator must read, the sentence that says a piece of work ended and how. It is
not a telemetry stream, and nothing here will make it one.

Logs, metrics, traces and high-frequency progress belong somewhere else — a file, a journal, a
collector. Two reasons, and the second is the one that bites:

* **The budget is small and shared.** Every conversation in one forum spends from one ceiling of a
  few sends a minute (below). A progress line posted once a second does not cost its own project
  anything; it costs every other project in the forum their questions.
* **A person cannot read a stream.** The surface is his attention, and it does not scale with the
  fleet. Twenty agents that each say one useful thing an hour is a usable channel; twenty agents
  that narrate are noise he stops opening, which is the same as an outage.

What the machine writes down instead of sending lives in the hub's own state directory, outside
every repository. Two of those files are its account of its own health: `hub.heartbeat`, whose
modification time is the alarm, and `hub.health` beside it. **How another program should read them
is not decided** — it is `docs/CAPABILITIES.md` OPEN 6, and the source is settled while the reader
is not. The other two are local records rather than feeds: `hub.connected.json` says which
conversations are live, and `hub.audit.log` carries absolute paths this machine minted, so it is
something to read here and not something to ship anywhere.

## The scale envelope — measured, not estimated

Every **measurement** here was taken against the real Bot API on a real forum and is written down
with its run in `docs/RATE-PROBE.md`; the rows below say which are measurements, which are shipped
constants, and which rest on an assumption we made deliberately. **The ceiling is not raised to make
the arithmetic look better; it is what Telegram enforces.**

**The phone side.**

| what | the number | where it comes from |
| --- | --- | --- |
| the chat's own ceiling | 20 sends a minute, per CHAT | measured: 40 sends across four topics, 20 accepted, the 21st refused |
| topics | buy nothing — they are threads in one chat, and share its budget | same run |
| what the hub spends | at most **18 sends a minute**, and no more than one a second | `queue.rs` (`PER_MINUTE`, `MIN_GAP`), deliberately under the ceiling |
| what the agents get | **seventeen** of those; the eighteenth is reserved for the hub | `queue.rs` (`RESERVED_FOR_THE_OPERATOR`) — the line saying the chat is full must never be the line the chat is too full to send |
| a new conversation | takes **three** turns before its agent's own words land — the topic, the greeting, then the message — so about **six new conversations a minute** across the whole forum (18 ÷ 3) | `hub.rs`; and creating a topic is **assumed** to be charged, never measured — see the unmeasured list below |
| editing a message | free; it is not charged against the ceiling at all | measured: 30 edits after 5 sends, none refused |
| reactions | free against sends, with a ceiling of their own: 20 a minute, of which the hub asks for at most 18 | measured; `queue.rs` (`REACTIONS_PER_MINUTE`) |

Read the fair share off the third and fourth rows: **seventeen sends a minute for every agent in the
forum together.** With C conversations all wanting to speak inside the same minute, each gets about
17/C of it. Nothing is dropped in silence — a send that cannot get a token waits — but it waits
against a shelf life, and past that it is shed and the agent is told: a minute and a half for prose,
twenty seconds for a question, because a question the operator sees late is a question he answers
into a turn that has moved on.

**The wire side, which is where the room is.** The socket has no per-minute limit. What bounds one
connection is a frame ceiling (64 KiB a frame) and what the hub will hold before a new bridge has
answered its first ping — 65 frames, which is what a conforming bridge may carry into a reconnect.
There is a per-minute figure in the `welcome` a bridge receives: it is **advisory, nothing enforces
it, and no adapter should pace against it.** Pace against the phone, which is the scarce end.

**One live connection per address**, always. A second is refused with a reason it can act on; a dead
one is evicted, a live one is never displaced.

One more bound belongs to neither list, because it is a decision of ours and not anything Telegram
charges for: **sixteen rooms may stand vacant per project at once** (`conversations.rs`, `BOOK`),
refilled only at a terminal.

**What is NOT measured** is listed in full at the end of `docs/RATE-PROBE.md`, and nothing above
leans on it silently — where the table rests on an assumption, the row says so. The list: whether
two different bots in one group have separate budgets, whether
`answerCallbackQuery` or topic creation are charged, whether the reaction ceiling is per chat or per
bot, whether photo and document uploads are charged like text, and anything about the paid tier.

## What is true about it, and stays true

* **It cannot type into a terminal.** Deleted, not gated (above).
* **Identity is a project and a conversation — never a path, a pid, or an engine's session.** A
  bridge presents a secret; the hub resolves it to a project and takes the title from its own
  registry. Nothing on the wire names a directory, a session or an engine, and there is no field for
  one. Which session of an engine a conversation means is the adapter's to enforce and whoever
  launched it to decide (`docs/CAPABILITIES.md` REFUSES 6, OPEN 5).
* **A question answered once can never be answered twice.** A tap resolves against a written record,
  under one lock; the second tap, and a tap on a question the agent has already closed from its own
  side, are refused with a sentence rather than answered again.
* **Inbound content selects, it never names.** What arrives from Telegram can only choose among
  things the machine already knows. No message, tap or command can enrol a project, edit an
  allowlist, let a person speak, or touch a credential — and a test fails the build if the bot so
  much as names the setter that would.
* **Admission is terminal-only.** A message that could enrol could grant itself access.
* **The hub is same-host today.** One transport: `AF_UNIX` under the user's own runtime directory,
  admitted on two facts — the peer's uid, which the kernel reports, and the secret in `hello`. That
  is a property of the transport and not of the frames, which is why it sits behind a seam
  (`transport.rs`) that a remote transport could be added at. Nothing of that is built, and building
  it would change no frame; until it is, the honest answer to "can a bridge on another box reach
  this hub" is **no** (`docs/CAPABILITIES.md` OPEN 4).
* **The alarm outlives the thing it watches.** A watchdog sharing no code, no process and no runtime
  with the hub, which only tells the operator — it restarts nothing. Nothing in the serving path can
  start a process: `hub.rs` and `surface.rs` name no way to run one, and `bot.rs` uses the word only
  for a line the operator typed. So no string that arrived from Telegram or from the wire can reach
  a command line (`docs/CAPABILITIES.md` OPEN 1).

## What it does not do

Each of these is a decision, written down so it is not proposed again: spawn, supervise or kill
anything from a message or a frame; type into a terminal; choose a model, an engine or a repository;
know what a lane, a room, a proof, a re-ground or an engine's session **means** — all of those reach
the phone through the conversation primitives and nothing more; enrol a project or mint a secret
from a message. `docs/CAPABILITIES.md` REFUSES is the list another org may rely on, and its wording
is the authoritative one.

Carrying logs, metrics, traces or progress to the phone is the same kind of decision and is
described at the top of this file, but it is **this repository's rule and is not yet written into
that list** — so it is not something another org can hold us to today.

## The parts

Three crates in one Cargo workspace, plus the adapters that attach to them.

* `crates/hub-proto` — the wire contract. NDJSON, nine frames up, six down. It knows nothing about
  herdr, kickoff, Claude or any engine, and must not learn.
* `crates/herdr-tg` — the bot and the hub. **The hub binds nothing**: no listening port, and a
  Unix socket is not one. The one binary here that listens is `kickoff-door`, a separate program
  that binds loopback only and serves the ring and the answers drop over HTTP to the PWA's bridge;
  nothing reachable from a message, a tap or a frame names a port outside it.
* `crates/herdr-client` — a typed client for herdr protocol 20, in maintenance. It serves the four
  read-only subcommands and nothing else; the bot does not talk to herdr at all.
* `plugins/kickoff-channel/` — the MCP tool server both engines start. It holds no token, no
  allowlist and no model.
* `adapters/kickoff-hub-attach/` — the one thing under `adapters/` there is to run: it holds the
  claim, opens a door several producers can share, can watch an opencode server, and with `--run`
  starts the engine as its own child. `--check` proves an environment can reach the hub without
  creating a topic.

`docs/ATTACHING.md` is written so a stranger can implement an adapter from it, and
`docs/examples/attach-from-the-document.ts` is a stranger's adapter that imports nothing of ours and
is run against the real door by the suite.

## The commands

Sixteen, and every one of them is run at a keyboard. Nothing in Telegram can reach any of them.

The phone has two commands of its own and the set is closed: `/projects`, which says which projects
are enrolled, which are connected and which are switched off, and `/help`. Neither can change
anything, and `no_message_can_switch_a_project_off_or_on` in `bot.rs` fails the build if a third
appears, whatever it is called.

| command | what it does |
| --- | --- |
| `serve` | run the hub, reaching him the way `--to` names: `telegram` long-polls the Bot API and answers the allowlisted chats, `app` holds no credential and dials nothing. Required, with no default |
| `open` | open a project as a conversation, writing nothing into its repo |
| `grant` | grant a project rooms — complete conversations of its own, each with its own secret and topic, vacant until a dispatcher takes one |
| `enroll` | the older door, and the rotation: enrol a project, or rotate the secret of one already enrolled |
| `adopt-secrets` | copy every enrolled project's secret to where the channel keeps one. Dry run by default |
| `remove-repo-secret` | remove a project's secret from its repo, once the channel provably holds the same bytes |
| `projects` | every enrolled project, and whether it has a topic yet (`--json` for the machine-readable form) |
| `disable` | switch a project off — it and every room of it. Connected bridges are dropped; topics and history stay |
| `enable` | switch it back on |
| `allow` | let a person speak in one project's conversations |
| `disallow` | stop a person speaking in them. From now on; what was already relayed stays relayed |
| `status` | herdr's view of the herd. Read-only |
| `read` | print what one of herdr's screens is showing, as text, byte for byte. Read-only |
| `doctor` | is this bridge's view of herdr still valid, and is the control plane being watched |
| `watch` | decode herdr's event stream. Read-only |
| `door-token` | mint the token the PWA's gateway takes on its write door. Refuses to overwrite; rotation names the old token's first characters |

## Build and test

All three parts are required, and the prefix is not optional:

```
env -u RUSTUP_TOOLCHAIN TMPDIR=<a real absolute dir> PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace
```

`PATH` because mise shims hide cargo; `env -u RUSTUP_TOOLCHAIN` because mise exports it globally and
it overrides `rust-toolchain.toml`; `TMPDIR` because an agent session inherits it as a literal
`%h/…` string, which fails several transport tests for a reason you did not cause. The same prefix
applies to `git commit`, which runs the gates in your environment.

Some tests are `#[ignore]`d because they need bun or a live child process. The bridge suite is the
one that proves the plugin and the hub still agree on the wire:

```
cargo test -p kickoff-channel the_real_plugin -- --ignored
```

`scripts/install-channel-plugin.sh` is the only supported way to put the bridge on a box. It runs
those proofs and refuses to install a bridge that disagrees with the hub; it refuses an uncommitted
plugin tree unless told otherwise; and it **verifies afterwards** — every file this repo tracks is
compared with the copy on the box, byte for byte, and an install that reported success but produced
different bytes is a failed install. What it put there is written down outside every repository, at
`plugin.installed` in the hub's own state directory.

The plugin's version is bumped by hand in `plugins/kickoff-channel/.claude-plugin/plugin.json`,
`package.json` and `server.ts` together, because the version is the cache key: a box that already
has that number fetches nothing. `the_channel_plugins_version_moves_with_its_content` fails the
suite if the plugin's files change under a version that does not — including a number that has
already shipped and is being re-used.

**A session already running keeps the bridge it started with** until it is restarted, so an install
does not change the session that performed it.

## Read these, in this order

| file | what it answers |
| --- | --- |
| `docs/ATTACHING.md` | how anything attaches — one namespace, the address, the credential, the handshake, the wire rules |
| `docs/CAPABILITIES.md` | what the hub offers, requires and refuses, and what is still open |
| `docs/INTERFACES.md` | the four seams, and the closed list of what this project does |
| `docs/CONVERSATIONS.md` | how a project, a room and a lane each get a conversation |
| `docs/RATE-PROBE.md` | the measurements the envelope above rests on, and what is still unmeasured |
| `CLAUDE.md` | the state of the repo, kept current, and the quality bar |
| `docs/HUB-DESIGN.md`, `docs/SLICE-3-REVIEW.md` | historical: the redesign, and why the screen-reading product died |

## Status

The hub is the product, and it has run for real: on 2 September 2026 a question left an agent's
turn, reached the operator's phone, and his tap came back as a message in that same turn. It needs
an interactive session — a print-mode run ends the turn before the tap can arrive. `CLAUDE.md` is
where the current state is kept, dated claim by dated claim, and `TRACKER.md` says which documents
are maintained and which are not.

Built for personal use by [@vinceferro](https://github.com/vinceferro) on a single Linux box.

## License

MIT — see [LICENSE](./LICENSE).
