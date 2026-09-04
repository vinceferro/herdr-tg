# fanin — one slot at the hub, served by more than one voice

The hub admits **one** live connection per addressable thing. On opencode two things want to speak
for one of them:

| voice | what it carries | who chooses to send it |
| --- | --- | --- |
| `plugins/kickoff-channel/server.ts` | `reply` · `ask` · `done` · `ask_resolved` | **the agent**, deliberately |
| `adapters/opencode-bridge/bridge.ts` | permission and question prompts | opencode, on the agent's behalf |

Both dialling the hub means the second is refused with `already_claimed`. So the joining happens on
**this** side of the seam: this process holds the claim, the others talk to it, and the hub sees one
`hello`, one pid, one claim — exactly what it saw before. `docs/INTERFACES.md` keeps a closed list of
six hub capabilities and a seventh would be a decision, not a refactor.

```
   tool server ─┐
                ├─► fanin ──(the same nine frames)──► hub ──► the phone
  event bridge ─┘
```

**The local socket speaks hub-proto**, unchanged. That is what makes "one code path chosen by
configuration" true rather than aspirational: a producer's queue, framing, ack handling and
three-outcome vocabulary are byte-identical, and only the address differs. There is no second wire
contract to review, and `crates/hub-proto` is not touched.

## Running one

```
KICKOFF_HUB_PROJECT_DIR=<the repo, or one of its lane worktrees> bun adapters/fanin/fanin.ts
```

One process per **addressable thing** — per repo, and per conversation of it. It derives everything
else: the repo and, when nobody dispatched one, the address from git; the secret by searching upward
from that directory; the hub socket from the uid; and its own door by hashing `(main worktree,
address)`. Nothing is guessed: a directory that is not inside a repository and was given no door of
its own is refused, and so is a second relay for an address that already has a live one.

Every variable it reads is in `docs/ATTACHING.md` §2 and is read by `plugins/kickoff-channel/
attach.ts`, the one reader all three adapters share. The ones that matter here:

| variable | for |
| --- | --- |
| `KICKOFF_HUB_PROJECT_DIR` | **required.** Which project this relay speaks for. |
| `KICKOFF_HUB_ADDRESS` | which conversation of it, when a dispatcher minted one. Otherwise git's name for the worktree, or none. |
| `KICKOFF_HUB_SOCKET` | override the hub's socket. Tests, and a container. |
| `KICKOFF_HUB_RELAY_DIR` | override where the relay doors live. Tests, and a container. |
| `KICKOFF_HUB_RELAY_SOCKET` | name this relay's door outright, instead of deriving it. |
| `KICKOFF_HUB_RELAY_GRACE_MS` | how long a departed producer has to come home, default 90 s. Refused rather than coerced when it is not a positive whole number. |

`KICKOFF_HUB_RELAY=1` is what makes a process a **producer**, so it is the one variable a relay must
NOT have: a relay told to attach to a relay would dial its own door. It refuses to start.

## Wiring opencode to it

opencode's config takes a stdio MCP server under `mcp`, which is exactly what the tool server
already is. Declare it in the **global** config, `~/.config/opencode/opencode.json`:

```json
{
  "mcp": {
    "kickoff-channel": {
      "type": "local",
      "command": ["bun", "<repo>/plugins/kickoff-channel/server.ts"],
      "environment": {
        "KICKOFF_HUB_PROJECT_DIR": ".",
        "KICKOFF_HUB_RELAY": "1",
        "KICKOFF_HUB_ADDRESS": "-",
        "KICKOFF_HUB_TOKEN_FILE": "-",
        "KICKOFF_HUB_SOCKET": "-",
        "KICKOFF_HUB_RELAY_SOCKET": "-",
        "KICKOFF_HUB_RELAY_DIR": "-",
        "KICKOFF_HUB_RELAY_GRACE_MS": "-",
        "CLAUDE_PROJECT_DIR": ""
      },
      "enabled": true
    }
  }
}
```

Three facts make that one entry cover every project and every lane, forever, and each was measured
rather than assumed:

* **opencode sets the MCP child's cwd to the session's own directory** — the directory the request
  named, including a linked worktree or a subfolder of one. So the tool server does not have to be
  told which project it is in; it reads it off the machine, exactly as it does under Claude Code.
* **A project-level `opencode.json` does not reach a lane worktree.** This repo gitignores that file
  (it carries a provider key), so it is never checked out into one, and a lane worktree got no MCP
  child at all. The global config has no such problem and needs no per-lane file.
* **`KICKOFF_HUB_PROJECT_DIR: "."` is the whole of "my cwd is the project".** A literal dot is a
  claim somebody wrote in a file; a bare fallback to cwd is a guess, and cwd is the *plugin* folder
  under Claude Code in both layouts — the defect that made every message an agent believed it had
  sent go nowhere. The dot also cannot be produced by opencode substituting a variable that was not
  there, which the empty string can.
* **`CLAUDE_PROJECT_DIR: ""` is now the second lock rather than the only one.** An opencode server
  started from inside a Claude Code session inherits that variable, and the child inherits the
  server's whole environment — so the tool server resolved *another repository's* secret, silently,
  because it really did find one. `KICKOFF_HUB_PROJECT_DIR` outranks it now, so a dispatcher's
  explicit word beats an engine's ambient one; blanking it costs nothing and closes the same door
  twice.
* **The six `-` entries are the same lock on the rest of the namespace.** `KICKOFF_HUB_` does not
  stop a variable crossing that boundary; it only gives the crossing one prefix. A Claude Code
  session dispatched with `KICKOFF_HUB_ADDRESS` hands that name to every opencode session started
  from inside it, which then speaks into a conversation nobody opened for it. opencode can only
  overlay a variable, never remove one, and the empty string is taken — it is what a failed
  `{env:VAR}` substitution produces — so `-` means "as if unset". `docs/ATTACHING.md` §2 is the
  rule; the point here is that a config which starts a second engine overlays **all** of them, not
  just the one it needs.

Then, per project or conversation, start a relay. The event bridge joins the same relay with
`KICKOFF_HUB_RELAY=1` and `KICKOFF_HUB_RELAY_SOCKET`; not a line of it changes.

## What it does that a straight pipe does not

1. **Answers `hello` itself**, with the `welcome` it holds — including the `lane` echo, so a
   producer's refuse-rather-than-impersonate check keeps working unmodified. A producer still
   presents its token and it is compared with the one this relay authenticated with.
2. **Rewrites envelope ids.** Two producers both mint `b1`, and an `ack` naming the wrong frame
   tells the wrong agent its message was lost.
3. **Namespaces `ask_id`.** Two producers can both mint `a1`, and the hub resolves a tap *by ask id*
   against a written record — so without this, a tap on one agent's question is delivered to the
   other agent. It is the worst thing that can go wrong here, and it has a test with a RED that
   shows exactly that happening.
4. **Answers the hub's `ping` itself**, never forwarding it, so a producer that has wedged cannot
   cost the lane its liveness and therefore its claim.
5. **Gives each producer a share of the queue, sized by how many are here.** A fixed cap is not a
   reservation: at 28 each, two producers hold 56 of a 64-slot queue and the *third* is refused
   permanently before it has said one word. Three is not exotic — opencode starts one MCP child per
   **directory**, so the repo root, a subfolder and the event bridge are three voices on one relay.
6. **Ends a producer's connection when its own link to the hub drops**, because the wire has no way
   to un-welcome anybody and a producer whose link is up says "said" for a message nothing carried.
   The producer's queue goes back to waiting, it says "not said yet", and it is welcomed again when
   there is something to be welcomed to.
7. **Keeps one `instance` across a hub reconnect *and* across a restart of itself**, so neither
   silently voids questions producers are still waiting on.
8. **Takes the buttons off a departed producer's questions** — see below, it is the job the hub
   cannot do while this relay stands in front of it.

## A producer's lifecycle became this relay's job, because it took it off the hub

The hub retires a dead asker's questions from two facts it reads off the **claim**: the `instance` in
`hello`, and the pid holding the socket. Behind a relay both of those are the *relay's*, for every
producer, for ever — so neither hub sweep can see a producer die, and the operator would be left
looking at a keyboard for a question whose agent no longer exists until it aged out two days later.
A tap on it is answered once, at the hub, into nothing.

So this process does the same job with the facts it does have, and they are the same two facts in a
different form:

* **A producer is known by the `instance` in its own `hello`, never by its socket.** A process keeps
  one for its whole life, so a producer that merely reconnected is the same producer and still gets
  the tap it is waiting on. One that restarted says a new one and is correctly a new voice.
* **When a producer's socket goes and nothing comes back under its name within 90 seconds**, its
  open questions are withdrawn — `ask_resolved{how: "withdrawn"}` — and the buttons come off. The
  window is that long because a producer's own redial backs off to a minute, and withdrawing a
  question that is about to be answered is the worse mistake.
* **Its instance and its open questions are written down beside the socket**, so a restart of this
  relay comes back as the same voice the hub already has those questions open under. Under
  `/run/user` that file is cleared by a reboot — which is exactly right, because a reboot takes the
  producers with it and the hub *should* void what they left.

The direct-to-hub path has no version of this: a channel plugin mints one instance for the life of
its session and the hub reloads its ledger from disk, so restarting `herdr-tg` under a live Claude
session leaves that session's questions answerable.

## What it deliberately does not do

* **It never lets a producer fall back to dialling the hub.** That is two writers racing for one
  claim, which is the whole thing the claim exists to prevent. A producer asked to use a relay and
  unable to find one says so, in the same three-outcome vocabulary, and waits.

## Test

```
bun test-two-producers.ts     # the design
bun test-what-breaks-it.ts    # what four reviewers found the design did not survive
```

Real tool-server processes and the real event bridge, against a fake hub that enforces one claim per
address, over real Unix sockets. The first suite's Part 1 proves the race is real before Part 2
proves the relay dissolves it. The second loses the hub under a producer, loses a producer with a
question open, opens 260 questions at once, runs three producers, and restarts the relay — every one
of them a case the first suite could not see. Both share one rig, in `test-harness.ts`, because two
fake hubs are two suites that come to disagree about what the wire is.

Nothing inside any producer is mocked: a fixture invented here would only prove this file agrees
with itself, which is how the event bridge came to read its payload out of the wrong field and pass
every test it had.
