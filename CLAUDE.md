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

- `crates/hub-proto` — the wire contract between the hub and an adapter. NDJSON, nine frames up, six
  down. Knows nothing about herdr, kickoff, claude or panes, and must not learn — nor about what
  carries it: that is `transport.rs`, the one file in the bot that knows what a socket and a peer
  uid are, and `tests/the_hub_does_not_know_what_a_socket_is.rs` fails if the hub learns either.
- `crates/herdr-tg` — the bot. A `clap` binary: `enroll`, `projects`, `serve`, plus four read-only
  herdr subcommands (`status`, `read`, `doctor`, `watch`). **The hub binds nothing** — no listening
  port, and the Unix socket is not one. The one program in this workspace that listens is
  `kickoff-door` (`src/bin/kickoff-door.rs` + `src/gateway.rs`), a separate binary that binds
  **127.0.0.1 only** and serves the ring and the answers drop over HTTP to the kickoff PWA's
  bridge; its write door takes a bearer token minted at the terminal by `herdr-tg door-token`, and
  nothing reachable from a message, a tap or a frame names a port outside it.
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
several**. **Which** session of that server it speaks to is not a guess: `--opencode-binding-file`
names the file a launcher writes — one JSON object, stamped `"version": 1`, carrying the
conversation it was written for, the session, the project it is for, the agent it should be running
and which writing of it this is, in the launcher's own key names — and attach
speaks to that session and to no other in both directions, refusing his line out loud (a binding
written for a sibling room's conversation among them) rather than falling back to the most recently
active one, with the file proved before a byte of it is believed
(where it sits, the link it might be, its owner, its mode). `--opencode-binding-generation` is the
floor that survives a restart, because what the process remembers about how far the launcher had got
is exactly what a restart destroys and a stale launcher's file outlives — the unit carries it in the
same env file as the port, which systemd re-reads on every start. The binding decides the answer the
hub hears: the watcher is the door's **carrier**, and its refusal outlives another producer's
acceptance, so a line it refused is never answered with a thumb. And a question the machine could
not decide on — the file not written yet, the server stalled — is kept and offered again rather than
dropped, because the thing waiting on it is an agent. `docs/ATTACHING.md` §13.10 is the whole of it.
`--check` proves an environment can reach the hub without creating a topic. There is one thing to
run under `adapters/`, on purpose.

**One namespace, `KICKOFF_HUB_`, and one document.** `docs/ATTACHING.md` is the contract an adapter
attaches by — eight variables of which a normal adopter sets one, the address a dispatcher mints, the
credential rule, the handshake, and the thirteen wire rules; §13 is `kickoff-hub-attach` itself. It is
written to be implementable by a stranger, and `docs/examples/attach-from-the-document.ts` is a
stranger's adapter that imports nothing of ours and is run against the real door by attach's own suite.

Docs, in the order they are worth reading: `docs/RUNNING-THE-HUB.md` (the short one — what to
start, what is deliberately stopped, and what an agent needs to reach the operator),
`docs/PWA-DOOR.md` (the door the app speaks to), `docs/ATTACHING.md` (how anything attaches),
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

**One test in this suite is flaky, and it weakens every green run — follow-up, not yet fixed.**
`summarize::tests::an_ambient_proxy_must_not_be_able_to_reroute_the_gist` (`summarize.rs`) fails
about one run in three with `run the child: Os { code: 2, kind: NotFound }`: it spawns
`current_exe()` and the spawn comes back ENOENT. Re-running it passes, so it has been re-run, and
that is the whole problem. **A green gate is the evidence every contract this repo publishes rests
on** — `docs/PWA-DOOR.md` opens by telling another organisation that every sentence in it was read
in code that passes this suite — and one green run of a suite with a one-in-three flake is one run
of a coin. It also trains the reflex the gate exists to prevent: a red test you have learned to
re-run is a red test you have learned not to read. Nobody has found the cause; the spawn is of this
binary's own path, which is the part that makes it worth an hour rather than a `#[ignore]`. Until
somebody does: hitting it is not a red gate, and it is not licence to re-run anything else.

Fourteen tests are `#[ignore]`d — eleven need bun (one runs the plugin from before conversations
existed against the new hub, one dispatches three rooms with the real adapter, one is the fleet
trial below), one is a proxy-driven child, one runs the pre-change bridge against the new hub, and
one is the hermetic PWA-door trial (`scripts/pwa-door-trial.sh`). The count is held to the tree by
nothing, so it has been wrong before: when you add one, move the number in the same commit.
`scripts/install-channel-plugin.sh` runs the bun ones and refuses to install a bridge that
disagrees with the hub. It does NOT run the fleet trial: four conversations crossing is a property
of the hub and the adapter, which installing a bridge can neither break nor fix, so failing an
install on it told the operator his plugin was unsafe when his plugin was fine:

```
cargo test -p kickoff-channel the_real_plugin -- --ignored
```

**If you touch `plugins/kickoff-channel/`, bump its version in the same commit** — in
`.claude-plugin/plugin.json` and `package.json`. The installed copy is cached by version, so a
change that does not move the number never reaches a box that already has the plugin, silently;
that is how a bridge from 1 September was still running on 7 September while the source had grown
five times over. `the_channel_plugins_version_moves_with_its_content` turns the whole workspace red
until the number moves, which is the point.

## The domains, and who owns them

| Domain | Owner | Lives in |
| --- | --- | --- |
| the wire contract | `wire-protocol` | `crates/hub-proto/` |
| the transport and who the kernel says is on it | `write-safety` | `transport.rs`, `the_hub_does_not_know_what_a_socket_is.rs` |
| identity, claims, the tap ledger | `write-safety` | `hub.rs`, `registry.rs`, `no_live_write_call_site.rs` |
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
  `POST /session/{id}/prompt_async` verbatim, in the session a launcher bound it to — or, with no
  binding, the one the server lists for the project directory; a reply typed under a question — the
  hub sets `in_reply_to_ask` from the message he
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
- **Claude as attach's `--run` child is proven to the phone** (6 September): door, claim, tool server
  attached, a reply and a question delivered, clean exit — on the real hub, a throwaway project. The
  tap back into that turn under attach's door awaits a tap. The proof found three defects, all fixed:
  a `--check`'s `bye` before the pong is journaled as a goodbye, never audited as a refusal; when the
  engine exits under `--run`, attach withdraws every question the door still holds and the phone
  reads "the session that asked has ended" instead of dead buttons; the plugin starts from a
  read-only mount without `bun install` (`start.ts`). ATTACHING §13.1 has the Claude worked
  invocation: `--channels` last, prompt on stdin, trust pre-accepted, one ToolSearch turn.
- **A run of an address carries a lease, and a tap is answered for** (7 September). The hub
  mints a number for every claim it grants, stamps it on the `welcome` that grants it and on
  everything it sends afterwards, and a bridge stamps it back — so a run a later one replaced
  is refused `stale_generation` and told to stop, where before it was overwritten in silence
  and went on emptying its queue into a conversation its successor owned. That refusal is
  permanent for the run: the way back is a new run, never a redial — the tool server stops
  dialling and tells its agent to restart the session, and `kickoff-hub-attach` stops the whole
  run, engine and all, so whatever starts walls starts a new one — but that word never travels
  DOWN through attach's door: a producer holds no lease, and one told to stop dialling never finds
  the next run of the wall, which `Restart=always` brings up seconds later. Their sockets are
  ended instead, which every producer already reads as "wait, and dial again". And a bridge that promises
  `confirms: ["choice"]` now answers every tap `accepted` or `refused`, so the line on his
  phone is edited from `Sent: X` to `Taken: X`, or to why it was not taken — it said "Sent"
  for ever before, whatever the far side did with the answer.
- **A conversation is named by an id, never by a path** (8 September). The `welcome` carries
  `project_id` (the seed) and `conversation` (the id the secret resolved to); a `hello` need name
  neither a repo nor a pid, and an instance no longer embeds one. `projects --json` gains a tenth
  field, `seed`, appended. A room whose seed was re-enrolled at a moved path stands for itself
  rather than guessing at the nearest folder. Seed ids are still a path hash — an implementation
  detail, with random minting named as debt. `docs/ATTACHING.md` §3b is the table: for sixteen
  things, whether it is fleet identity or a local detail. **A dispatcher joins on the ids.**
- **"No `Command` in the binary" was false, and is now a guard.** Five `git` call sites run during
  enrolment and one re-runs this binary for the summariser. The true property —Nothing reachable
  from a message, a tap or a frame can start a process — is held by
  `tests/nothing_inbound_can_start_a_process.rs`: a fixed program name at every allowed site, a
  program built from a value refused even there, the test-only sites proved gated. Six evasions
  planted and killed, including a module mounted from outside the walk and a renamed file. Four
  documents that overstated it now say what the guard proves.
- **The PWA has a door, and it is the product's new front** (17–18 September). `kickoff-door` is a
  separate binary on loopback serving the hub's own ring of operator-visible events and taking the
  operator's writes; `docs/PWA-DOOR.md` is the contract, written to be implemented by a stranger,
  and `bash scripts/pwa-door-trial.sh` is the hermetic proof — one real hub over a real socket, the
  real gateway binary on a real port, Telegram counted instead of called. The surface org read the
  contract and called it near drop-in. Five mends followed their verdict: the hub's receipts are
  written by stage-and-rename rather than emptied where the door is already reading them; a refusal
  of his words names the line it refuses, and only on the arm where the words came through the door,
  because a phone line's name is a Telegram id the ring may never carry; the write door's answers
  name the conversation they resolved to, and a timed-out MESSAGE names the nonce its echo will
  carry (a tap gets none — no file and no ring line will ever say it); the ladder's first rung fills
  a missing lane from the question, scoped to the conversation the body named; and the door's own
  refusals say what a person can act on instead of naming wire fields back at him.
  **The defect worth remembering was in none of that.** The door remembered every question twice —
  once from the ring it reads at start, once from the line it served — and read one question as two
  conversations asking under one name, so it refused the tap. A client can only learn an ask id by
  reading the ring, so every client that could tap was a client that had made the door remember
  twice: the routing rung was broken for every real client, and no test saw it because no test had
  polled before it tapped.
- **What this repo publishes about another organisation is scrubbed** (18 September). The contract
  named their files, their line numbers, their helpers and once a verbatim line out of their test
  suite — checkable for their engineer, and published to everyone, because THIS REMOTE IS PUBLIC.
  The findings all stand; they are written as behaviour now, which their engineer can still find in
  one search of his own tree. Letters addressed to them left the repo (`.gitignore` covers
  `.kickoff/mail-drafts/`) and travel by agent-mail instead. Nothing had been pushed when this was
  caught, so nothing was ever published; `pwa-door-proposal.md` remains in history at `7d85dc9`.
- **A fleet trial runs hermetically: `bash scripts/fleet-trial.sh`** (9 September). Four
  conversations of one repo, live at once, each bound to its own engine session — a real hub over a
  real socket, four real adapters, fake Telegram and fake engines, nothing spent and nothing sent.
  It proves each conversation's words reach only its own session, a question from an unbound one
  reaches nobody, a tap answers only the session that asked, and four at once do not cross. The test
  is `four_rooms_of_one_repo_reach_only_the_session_their_own_note_names_and_the_room_paired_off_by_one_reaches_nobody`.
  It exists so another org can run a fleet against the real wire without a Telegram account; what it
  cannot prove is what only a live forum can — the Bot API's own refusals and what it really charges.
  **A filter that matches nothing is a pass to `cargo test`**, so the trial refuses a run that
  matched no test rather than reporting green. That shape was found in the installer, where it sat
  in the step that decides whether a bridge is installed at all; the installer's own proofs keep the
  refusal, and the trial itself is no longer run there.
- **The screen-scraper is deleted, not disabled.** `permission.rs`, `deliver.rs`, `mirror.rs`,
  `voice.rs`, `notify.rs`, `audit.rs` and `routing.rs` are gone, along with the `HERDR_TG_PANES`
  flag that briefly gated them. `there_is_no_way_from_telegram_to_a_keyboard.rs` pins the deletion.
- **The watchdog measures the control plane, not just Telegram.** It still shares no code or
  process with the hub, and it arms the first time it sees **either** of the hub's own files in
  `~/.local/state/herdr-tg/` — the stamp, or the note beside it. Arming on the stamp alone kept
  exactly the box this change is about silent for its whole life: a hub whose door never opened
  never earns a stamp, so there was nothing to arm on. What changed otherwise is what a stamp
  costs. `heartbeat.rs` holds **three** facts apart — the phone line answering, a connection coming
  out the far end of the accept loop, and the dispatcher still being handed his taps — and
  `stamp_if` touches the file only when all three were true inside ninety seconds. Before this, a
  hub whose socket never opened answered `get_me` every forty-five seconds and stamped a green file
  while every agent on the box talked to nobody; and after the first two legs, a second copy of the
  bot holding the update slot kept both of them perfectly true while every tap died in silence. The
  third leg is two shapes in one: a dispatcher that has **stopped looking** goes stale like any
  other leg, and one that is **looking and being refused** never goes stale at all, so a run of
  refusals that outlasts the freshness window is watched in its own right — one refused call is a
  blip, and two blips further apart than a run survives are two blips. **Withholding is the signal**
  — the only one a script that reads a modification time can hear, which is why a watchdog installed
  before this change starts alarming on a dead door with no update at all *on a box that had already
  stamped once*. A box that never stamped needs this copy of the script;
  `scripts/install-watchdog.sh` is what puts it there.
  `hub.health` beside it says which leg failed, in sentences, rewritten every tick; the watchdog
  reads it **only** to word an alarm it has already decided to raise, never to decide whether to
  raise one, and `tests/the_heartbeat_is_earned_not_scheduled.rs` holds the hub's sentences and the
  script's `case` patterns together in both directions. What is still proved by no leg is that a tap
  which arrived was **acted on** — a handler that took one and hung looks well here until the wedge
  backs up far enough to stop the stream being driven — and `heartbeat.rs` says so rather than
  implying it. The alarm still restarts nothing; that belongs to whoever dispatches.
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
- **The tracker shim `.kickoff/bin/mc` works again**, and the founder's PWA renders what it writes
  (`.kickoff/state/mission-control/mission-state.json`) as this project's lanes and plate. It was
  dead for about ten hours on 31 August, the note saying so outlived the outage by two weeks, and
  in the meantime nobody wrote the tracker: on 18 September its headline still announced Slice 3
  and its blocked list still said "3 commits unpushed" against an actual 79. A tracker nobody
  writes is worse than none, because his surface presents it as current. Write it, and read the
  exit code of what you ran.

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
