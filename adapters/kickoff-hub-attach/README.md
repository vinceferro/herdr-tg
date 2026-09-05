# kickoff-hub-attach — one command that puts a worker on the phone

A Claude worker was always one command. An opencode worker used to be **three** hand-started
processes for one conversation — a relay that held the hub slot, `opencode serve`, and an event
bridge that relayed its prompts — and when one died the other two hung. This is all three jobs in
one process, and it can start the engine as its own child so a wall (bwrap or docker) has a single
entrypoint.

```
kickoff-hub-attach [--opencode <url>] [--run <command...>]
kickoff-hub-attach --check [--opencode <url>] [--run <command...>]
```

It holds the claim for one `(project, address)`, opens the door of `docs/ATTACHING.md` §9 so the
engine's own tool server attaches to it unchanged, watches an opencode server when told to, and can
prove an environment can reach the hub before anyone trusts it. Everything about *which* project,
*which* conversation, *where* the secret is and *what* to dial comes from the `KICKOFF_HUB_`
namespace (§2) through the one reader, `plugins/kickoff-channel/attach.ts`. There is no project
flag: it is `KICKOFF_HUB_PROJECT_DIR`, and `.` means "the directory I was started in".

## The flags

| flag | what it does |
| --- | --- |
| *(none)* | Hold the claim and open the door. This alone is what `adapters/fanin/` used to be. |
| `--opencode <url>` | Also watch the opencode server at `<url>` — its questions and permission prompts go to the phone as `ask`, a tap goes back to its own reply endpoint, and what the operator types in the topic goes to the session as a prompt, verbatim (the session the server lists for the project directory; a reply under a question goes to the session that asked it). When there is no session to hand the words to, the hub is told and puts one line in his topic. Without this flag an opencode worker is half a phone — no questions, no prompts, and typed words refused out loud rather than carried — and the start and `--check` both say so. This is what `adapters/opencode-bridge/` used to be, minus its process, plus the half of the phone it never had. |
| `--run <command...>` | Start `<command...>` as this process's child, in the project directory, with the namespace pinned so any adapter descending from it finds the door. When the child exits, say `bye`, close the door, and exit with the child's status. Everything after `--run` is the command. |
| `--check` | Prove this environment can reach the hub — one plain line per fact, then exit 0 if all hold, 1 otherwise. Sends `hello` and `bye` and nothing else; **creates no topic**. |

**Exit status.** `0` a clean end · `1` a `--check` that found something to fix · `2` a refusal to
start (the sentence names the variable on stderr) · `127` the `--run` command was not found ·
otherwise the child's own status, `128 + n` for a signal.

## Running one, on this box

Once per project, at a terminal: `herdr-tg enroll <repo>`. Once per box: `bun` and `opencode` on
`PATH`, and the shim `~/.local/bin/kickoff-hub-attach` that `scripts/install-attach.sh` writes.
Then, in the worktree the worker is for:

```
cd ~/scratch/oc-dogfood
KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach --check --opencode http://127.0.0.1:9711 --run opencode serve --port 9711
KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach         --opencode http://127.0.0.1:9711 --run opencode serve --port 9711
```

`--port` is not optional: `opencode serve` without it picks a random port and the watcher would be
watching nothing. The number appears twice on the line so the server and the watcher can never
disagree about it, and attach refuses an `--opencode` URL with no port. A dispatcher that minted an
address sets `KICKOFF_HUB_ADDRESS` in front of the same line.

**Under `--run`, paste nothing into `~/.config/opencode/opencode.json`.** attach pins the whole
`KICKOFF_HUB_` namespace into the engine and the tool server inherits it; the file as it stands
attaches, measured with the real engine. The eight-`-` block of `docs/ATTACHING.md` §2 is for the
hand-started shape this command retires, and under `--run` it would make the tool server derive a
door from git — wrong for a minted address and impossible in a wall (§13.3).

As a **wall's entrypoint** it is the same command with the engine as its child; the worked docker
and bwrap shapes kickoff copies are in `docs/ATTACHING.md` §13.1, and the entrypoint facts (signals,
reaping, the private door, PID 1) are §13.5. One of them is worth saying here: **bwrap's init
forwards no signal**, so a bwrap wall is stopped by signalling attach's own host pid — the child
of the pid `--info-fd` reports — never bwrap, and started with `--die-with-parent` so a dead
wrapper does not leave it squatting the claim.

As a supervised **opencode worker on a desk**, `deploy/kickoff-hub-attach@.service` is a
`systemd --user` template — `systemctl --user enable --now kickoff-hub-attach@<label>` reads
`~/.config/kickoff-hub-attach/<label>.env` (the §2 namespace plus one `OPENCODE_PORT`).

## `--check`, and why it makes no topic

The hub's admission is ordered: it sends `welcome`, then `ping`, and makes the topic **only after
the pong** (confirmed in `crates/herdr-tg/src/hub.rs`). So `--check` dials, is welcomed, sends `bye`
and closes **without ever ponging** — proving socket, uid, secret, an enabled project, a
well-formed and echoed address, and a free claim, while creating nothing. Two honest costs: it holds
the claim for one round trip (released the instant it closes), and it leaves one
`connected but never answered` line in the hub's audit log. It is what a wrapper runs before
trusting a wall, and the first thing to run when a worker is silent.

## What the door does that a straight pipe does not

It stands in for the hub to its producers — the engine's tool server, the in-process opencode
watcher, and any stranger's adapter — so the hub sees **one** `hello`, one pid, one claim:

1. **Answers `hello` itself** with the `welcome` it holds, lane echo included.
2. **Rewrites envelope ids**, so an `ack` names the frame the producer actually sent.
3. **Namespaces `ask_id`**, so a tap on one producer's question can never reach another's.
4. **Answers the hub's `ping` itself**, so a wedged producer cannot cost the lane its claim.
5. **Shares the queue by how many producers are attached**, so a chatty one cannot starve a quiet
   one's question.
6. **Ends a producer's connection when its own hub link drops**, because the wire cannot un-welcome
   and a producer whose link is up would otherwise report "said" for a message nothing carried.
7. **Keeps the producer lifecycle** across a hub reconnect and a restart of itself — who a producer
   is (`instance`, never the socket), what it is waiting on, and taking the buttons off a departed
   producer's questions after a grace window. That is `ledger.ts`.

## The wire is written in exactly one place

`relay.ts`, `opencode.ts` and `check.ts` each take the wire from
`plugins/kickoff-channel/hub-link.ts` — the same module the Claude tool server uses. A test
(`test-against-fakes.ts`) fails if any of them starts writing its own, or if any other file under
`adapters/` or `plugins/` mints a `hello`. A fork of that wire once drifted by thirteen already-fixed
defects; there is one implementation, on purpose.

## Test

```
bun test-two-producers.ts    # the design: two voices, one claim, ids and taps kept apart
bun test-what-breaks-it.ts   # what four reviewers found the relay did not survive
bun test-against-fakes.ts    # the opencode mapping, and the one-wire guard
bun test-check.ts            # --check proves reachability and makes no topic
bun test-run.ts              # --run: signals, status, the private door, PID 1
```

Real tool-server processes, the real stranger's adapter, real Unix sockets, a fake hub that enforces
one claim per address, and a fake opencode over real HTTP. Nothing inside a producer is mocked: a
fixture invented here would only prove this file agrees with itself, which is how the event bridge
once came to read its payload out of the wrong field and pass every test it had.
