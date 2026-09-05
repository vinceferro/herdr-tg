<!-- INTERFACE, v9, 5 September 2026. The one abstract surface an adapter attaches to: one
     configuration namespace, one wire, one document.

     v9 is v7 and v8 attacked — two reviewers against the built slices, every change a defect
     reproduced against the running code. The pre-pong hold is 65 frames, not 64: the queue plus
     the one frame a bridge puts back at the head of it on close (§6). The hub now sets
     `in_reply_to_ask` from the message he swiped to reply to, so the reply path offer 6 described
     is taken end to end rather than only on the adapter's side of a fake hub. The line the hub
     posts for refused words is threaded under the line it refuses. `kickoff-channel` refuses
     typed words on an engine that cannot read a channel message rather than writing them into
     one, and attach's door folds several producers' answers into the one the hub hears (§13.9).
     Every request the watcher makes of opencode has a deadline, so one the server takes and never
     answers cannot park every later line; words opencode took and the agent then could not act on
     are said so in the topic. Nothing on the wire changed.

     v8 closes the pre-pong bound §12 used to defer. The hub holds 64 frames or 4 MiB before the
     pong — exactly what a conforming adapter may have queued when it dials, where it held 256 KiB
     and refused every bridge that carried an ordinary backlog into a reconnect — and on every way
     a connection is refused before it is live, every frame the hub read is acked `no` before the
     socket closes. The bridge's half is rule 10 of §8, extended: a frame flushed whole on a
     connection that ends is a frame no ack is coming for, and it is handed back as unconfirmed.
     Nothing on the wire changed.

     v7 closes offer 6's gap and §13.9's first bullet: typed steering reaches an opencode session
     (`kickoff-hub-attach` carries a `message` to `POST /session/{id}/prompt_async`, verbatim, in
     the session the server lists for the project directory), and every `message` is answered on
     the wire with `ack{ref, status, reason?}`. The hub now reads that status — it read the status
     of no ack at all before — and a `refused` becomes one line in the topic he typed in. Nothing
     on the wire changed.

     v6 is v5 attacked — three reviewers against the built `kickoff-hub-attach`, every change a
     defect reproduced against the running code:

       §13.4 — `--check` blessed three environments the start then refused with exit 2 (a relay flag
               on attach itself, a TMPDIR no private door can be made under, an `--opencode` URL
               with no port); it printed a phantom "closed without a word" line after every refusal
               it had already named; it read a 0700 hub directory as "not mounted"; it spoke to a
               person in the register written for an agent. Every sentence in its table is now the
               one the code prints.
       §13.3 — the private door is `mkdtemp`, not a folder named by pid: under `--unshare-pid` every
               wall is PID 2. The pinned `KICKOFF_HUB_TOKEN_FILE` is the path attach FOUND, not `-`.
               The paste block of §2 is for the hand-started shape only; under `--run`, paste
               nothing. The start-time warning this section promised now exists.
       §13.5 — bwrap's init reaps and forwards NOTHING: a signal to bwrap ends bwrap and leaves the
               wall running with the claim held. How a bwrap wall is actually stopped is written
               down, and pinned by a test. A uid-remapped bwrap wall needs a fourth variable.
       §13.2 — "stuck" on `already_claimed` is elapsed time, not three refusals (which was three
               seconds, shorter than a predecessor's stop), and the producers hear "no" for what
               they queued before their sockets end.

     v5 BUILDS §13. `kickoff-hub-attach` is now code — `adapters/kickoff-hub-attach/`, the one
     process that holds the claim, opens the door, watches an opencode server, can be a wall's
     entrypoint, and proves reachability with `--check`. `adapters/fanin/` and
     `adapters/opencode-bridge/` are gone; both folded into it, every check they held moved across.
     The interface changes v4 named are in place: the `KICKOFF_HUB_TOKEN` refusal (a secret handed
     by value), a `KICKOFF_HUB_TOKEN_FILE` that is the secret rather than its path, and the
     container worked example (§10) now uses attach as its entrypoint, so the relay socket is no
     longer something a container must be told. §13 remains, describing what was built.

     v4 added §13 as a design; every reference to `adapters/fanin/` and `adapters/opencode-bridge/`
     outside §13 is now `adapters/kickoff-hub-attach/`.

     v3 is v2 attacked. Every change below is a defect somebody reproduced against the running code
     rather than a rewording:

       §2 — `-` now means "as if this variable were not set", for every variable in the namespace.
            A namespace does not stop a variable crossing an engine boundary, and an opencode-shaped
            config can only overlay, never remove; without a spelling for "ignore this" an inherited
            address or token file was unfixable, and the empty string could not be it. The operator's
            paste block therefore overlays all eight. `KICKOFF_HUB_PROJECT_DIR=""` moved into the
            refuse-when-empty list: the old claim that unset already refuses was false, because
            unset falls through to `CLAUDE_PROJECT_DIR` first. The three socket variables are
            documented as absolute-or-refused, and `<uid>` is now spelled out as the real uid and
            explicitly NOT `$XDG_RUNTIME_DIR`.
       §4 — `bad_lane` has three causes, not two, and `bad_lane` with no `lane` sent identifies one
            of them with certainty. `-` is named as the one address this interface takes for itself.
       §5 — the namespace is inherited exactly as `CLAUDE_PROJECT_DIR` was, and nothing lets an
            adapter check which project it attached as.
       §6 — every field now carries a type, because a wrong one on `hello` is a silent close and
            looks identical to a uid mismatch; one fully worked `hello` and one worked `welcome`.
            `topic_id` is ABSENT, not null. `hello` is not acked.
       §9 — `mainWorkingTree` is defined; it is a git fact and not `KICKOFF_HUB_PROJECT_DIR`. The
            claim that the opencode event bridge cannot attach to a lane relay was true of the fork
            and is not true of the code beside it.
       §10 — a container behind a relay must be told the relay socket, because it cannot derive one.
       §11 — the example does not implement §9's derivation, so it does not prove that claim.

     v2 was v1 built. Written to be implemented by a stranger — someone building an adapter for an
     engine neither org has heard of, holding this file and the `crates/hub-proto` docs and nothing
     else. Everywhere that stranger would still have to read our TypeScript, this file says so out
     loud instead of pretending otherwise.

     It maps offer-by-offer onto docs/CAPABILITIES.md. That file is the menu; this one is how you
     order from it. A change to either is a diff rather than a letter. -->

# Attaching to the hub

## 1. What an adapter is

An **adapter** is a process that holds one connection to the hub on behalf of one conversation. It
proves which project it belongs to with a secret from that project's directory, names which
conversation of that project it is speaking for, and then carries two things in opposite directions:
what an agent wants the operator to see, and what the operator taps or types back.

Everything else about it is yours. The hub does not know what engine you run, what a session is,
what a lane or a room or a proof is, or that you exist between reconnections.

**The smallest thing that counts as one** is a process that:

1. connects to a Unix socket,
2. writes one line of JSON — `hello`, carrying the secret,
3. waits for `welcome`, and then writes one more line — `say`, `ask` or `done`,
4. answers any `ping` with a `pong`, whenever one arrives,
5. reads the `ack` for what it said.

**Steps 3 and 4 are in that order for a reason, and it is not the obvious one.** The hub pings you
immediately and will not create your topic until you have ponged — so against the hub, either order
works. A **relay** (§9) never pings its producers at all, so an adapter that waits for a ping before
it speaks is welcomed and then sits silent for ever behind one. `welcome` is the signal that you may
speak; `ping` is a question you answer whenever it is asked.

That is about sixty lines in any language, and `docs/examples/attach-from-the-document.ts` is those
sixty lines — written from this file alone, importing nothing from this repository, and run against
the real door by `adapters/kickoff-hub-attach/test-two-producers.ts`. It gets a forum topic of its own, delivery
you can trust, and a question with buttons. Everything after §7 is what you add to make it survive a
bad day.

---

## 2. One namespace

**`KICKOFF_HUB_`.** One line of justification: the two paths an adapter cannot rename already spell
it — the socket lives under `/run/user/<uid>/kickoff/` and the secret at `<repo>/.kickoff/hub.token`
— and the prefix is specific enough that a promiscuous environment cannot collide with it by
accident. It is not up for a second discussion.

**One variable is required. The rest are overrides you will probably never set.**

| variable | required | default | what it is |
| --- | --- | --- | --- |
| `KICKOFF_HUB_PROJECT_DIR` | **yes** | — | The directory this adapter speaks for. An absolute path, or the single character `.` meaning "the directory I was started in, and whoever wrote this vouches for it". Any other relative path is refused, because resolving one against cwd is the guess this variable exists to replace. |
| `KICKOFF_HUB_ADDRESS` | no | derived, then none | Which conversation of that project this is. Minted by whoever dispatched. See §4. |
| `KICKOFF_HUB_TOKEN_FILE` | no | found by searching upward from the project directory | The absolute path to the secret; a relative one is refused. See §5. |
| `KICKOFF_HUB_SOCKET` | no | `/run/user/<uid>/kickoff/hub.sock` | The hub's own socket; an absolute path, a relative one refused. Only for an adapter that holds the claim itself. **`<uid>` is the process's real uid, and it is NOT `$XDG_RUNTIME_DIR`** — see below. |
| `KICKOFF_HUB_RELAY` | no | unset | `1` means this adapter does **not** hold the claim: it attaches to a relay that does. It is the only thing that says so — see §9. Any value but `1` or empty is refused rather than read as "no". |
| `KICKOFF_HUB_RELAY_SOCKET` | no | derived, see §9 | Where the relay's socket **is**; an absolute path, a relative one refused. The relay listens on it; a producer behind one dials it. It does not by itself make you a producer: that is `KICKOFF_HUB_RELAY`, and both sides of a relay read this one. |
| `KICKOFF_HUB_RELAY_DIR` | no | `/run/user/<uid>/kickoff/fanin` | Where derived relay sockets live; an absolute path, a relative one refused. `<uid>` as above. |
| `KICKOFF_HUB_RELAY_GRACE_MS` | no | `90000` | Relay only: how long a producer that vanished has to come home before its open questions come off the phone. |
| `KICKOFF_HUB_TOKEN` | **never** | — | There is no by-value secret and there never will be. Set to anything (other than `-`), it is a **refusal** naming the fix, because the environment is promiscuous and a secret in a variable is a secret in every child's environment. The credential travels as a PATH, `KICKOFF_HUB_TOKEN_FILE`. A `KICKOFF_HUB_TOKEN_FILE` that is 64 hex characters and no path is refused the same way. See §5. |

**A relative path is refused, never resolved.** That goes for all four path variables above. A
relative path is worked out against whichever folder the process happens to be sitting in, which
under a Claude Code plugin manifest is the plugin folder — the one directory this interface exists
to stop guessing from. Nothing listens at the answer, so the link reports itself as merely down and
every message an agent sends waits in a line that never moves.

**`<uid>` is the process's own real uid, read from the kernel — never `$XDG_RUNTIME_DIR`.** Your
platform's documentation will tell you to read that variable, and it is the wrong answer here for a
reason worth one sentence: `XDG_RUNTIME_DIR` is not on kickoff's list of variables that survive its
`env -i` boundary, so an adapter started by a worker would not see it, and the two sides would
derive different paths with neither being wrong. On a box where they agree the mistake is invisible;
under a systemd user unit with its own `RuntimeDirectory`, or in a container, it is not.

Two variables live outside the namespace on purpose:

* **`CLAUDE_PROJECT_DIR`** is Claude Code's, not ours. An adapter may read it as a project directory
  **only when `KICKOFF_HUB_PROJECT_DIR` is unset**. That ordering is a fix, not a detail — see §5.
* **The engine's own endpoint** is seam ②, and the attach interface does not own it. For
  `kickoff-hub-attach` watching opencode it is the value of the `--opencode` flag (§13.1), not an
  environment variable at all; the old `OPENCODE_URL` retired with the bridge. An adapter for a new
  engine names its own flag or variable and documents it beside its own code. Do not put an engine
  endpoint in `KICKOFF_HUB_`.

### The empty-string rule

A variable set to nothing is not a value. opencode substitutes a missing `{env:VAR}` in its config
with the empty string rather than failing, so an empty value here always means *a configuration
asked for something that was not there*.

* **One variable where empty genuinely means "no".** `KICKOFF_HUB_RELAY` unset means "I hold the
  claim myself", which is a real answer, so empty means that too.
* **Every other one refuses outright when it is empty**, and never falls through to its default:
  `KICKOFF_HUB_PROJECT_DIR`, `KICKOFF_HUB_ADDRESS`, `KICKOFF_HUB_TOKEN_FILE`, `KICKOFF_HUB_SOCKET`,
  `KICKOFF_HUB_RELAY_SOCKET`, `KICKOFF_HUB_RELAY_DIR`, `KICKOFF_HUB_RELAY_GRACE_MS`. A configuration
  that set one meant to replace the default, and handing the default back is handing back exactly
  the thing it was replacing. `KICKOFF_HUB_ADDRESS=""` is the case that matters most: the default is
  "speak as the project", and silently taking it for a dispatcher whose variable failed to expand
  puts a lane's words in the project's topic and takes the project's claim. `KICKOFF_HUB_TOKEN_FILE=""`
  is the next: a container told exactly where its secret is goes looking for one instead, and
  attaches with whatever it finds.
* **`KICKOFF_HUB_PROJECT_DIR=""` is in that list and it is the one that reads as an exception.** An
  earlier draft of this document left it out, on the argument that unset already refuses so empty is
  that same refusal. That argument is false in exactly the environment the rest of this interface
  exists for: unset does not refuse on its own, it falls through to the engine's `CLAUDE_PROJECT_DIR`
  first. An adapter that sets the namespace variable from a substitution that failed, in a process
  descended from a Claude Code session, then attaches as **whatever repository that session named**
  — silently, because it really does find a secret there. That is incident 2 of §5, arriving through
  the one variable the rule had exempted.
* `KICKOFF_HUB_RELAY_GRACE_MS` set to something that is not a positive integer is a refusal, not a
  zero. `Number("")` is `0` and `Number("x")` is `NaN`; one withdraws every question instantly and
  the other never withdraws any.

Fail closed and say which variable, in words. A refusal that names the variable costs one line; a
silent default costs a conversation nobody can find.

### `-` means "as if this variable were not set"

The namespace does not stop a variable crossing an engine boundary; it only gives the crossing one
prefix instead of five. **Every `KICKOFF_HUB_` variable is inherited by every descendant of every
process**, including an engine started from inside a session of another engine — and a config of
the shape opencode uses can only *overlay* a variable, never remove one.

So there has to be a spelling for "ignore whatever an outer process left here", and the empty string
cannot be it, because empty is what a failed substitution produces. **The value `-` is that
spelling**, for every variable in the namespace, and an adapter treats it exactly as it treats an
unset one:

| what you write | what the adapter does |
| --- | --- |
| the variable is absent | its default |
| the variable is `-` | its default — identically, no warning, no difference |
| the variable is `""` | refuses and names it (above) |
| anything else | uses it |

A lone hyphen is never a path, never a socket, never a number of milliseconds, and never a
conversation anybody would mint. The hub itself *would* address a conversation called `-`; this
interface takes the name for its own use before the hub ever sees it, which is the one name §4's
shape rules do not account for.

**Any configuration that starts a second engine overlays all eight**, not only the project
directory. Getting one of them wrong is not a loud failure: an inherited `KICKOFF_HUB_ADDRESS` makes
a session speak into a conversation nobody opened for it and take that claim, and an inherited
`KICKOFF_HUB_TOKEN_FILE` attaches it as another repository entirely — silently, because it really
does find a secret. §5 is the two incidents that paid for this paragraph.

### Migration — what changed, and what each old name became

The operator granted a clean break in his own words: *"we can define a clean interface and make
kickoff adopt it and migrate easily."* So the old names are **dropped from the code path, not kept
as aliases.** Carrying eleven aliases into a clean interface is the thing the break exists to avoid,
and every consumer is one edit away. This is a deliberate departure from "list the old names as
accepted aliases" — they are listed here, and marked, but they are not read.

| old | prefixes | becomes | status |
| --- | --- | --- | --- |
| `KICKOFF_CHANNEL_PROJECT_DIR` | 1 of 5 | `KICKOFF_HUB_PROJECT_DIR` | dropped |
| `KICKOFF_CHANNEL_CWD_IS_PROJECT=1` | | `KICKOFF_HUB_PROJECT_DIR=.` | dropped |
| `KICKOFF_CHANNEL_VIA_FANIN=1` | | `KICKOFF_HUB_RELAY=1` | dropped |
| `KICKOFF_FANIN_PROJECT_DIR` | | `KICKOFF_HUB_PROJECT_DIR` | dropped |
| `KICKOFF_FANIN_DIR` | | `KICKOFF_HUB_RELAY_DIR` | dropped |
| `KICKOFF_FANIN_GRACE_MS` | | `KICKOFF_HUB_RELAY_GRACE_MS` | dropped, and its parse becomes fail-closed |
| `OPENCODE_BRIDGE_REPO` | | `KICKOFF_HUB_PROJECT_DIR` | dropped, **and its bare-cwd default dies with it** |
| `KICKOFF_HUB_SOCKET` | | `KICKOFF_HUB_SOCKET` | kept, **narrowed**: it means the hub and only the hub. Pointing it at a relay is no longer how you join one — `KICKOFF_HUB_RELAY_SOCKET` is |
| `KICKOFF_HUB_TOKEN_FILE` | | `KICKOFF_HUB_TOKEN_FILE` | kept, and generalised from one adapter to all of them. It was already the right shape |
| `CLAUDE_PROJECT_DIR` | | unchanged | kept as an **engine-owned** read, demoted below the namespace |
| `OPENCODE_URL` | | `--opencode <url>` | retired, and the endpoint is a flag on `kickoff-hub-attach`, not a variable — seam ② |

Eleven variables across five prefixes become eight in one, of which a normal adopter sets one.

**On our side this is done.** Both adapters that remain — the Claude tool server
`plugins/kickoff-channel/server.ts` and the one command `adapters/kickoff-hub-attach/` — read
`plugins/kickoff-channel/attach.ts`, and it is the only file either of them reads a variable in;
`where.ts` beside it answers only what the MACHINE says once a directory has been named, and
`hub-link.ts` is the one wire (§8). The one other file in the repo that reads these variables is
`docs/examples/attach-from-the-document.ts`, and it does so deliberately — it is a stranger's
adapter, written from this document, importing nothing of ours.
If it ever needed to import `attach.ts`, this document would have failed.
**The edit on kickoff's side** is one: wherever the dispatcher starts an agent, it now exports
`KICKOFF_HUB_PROJECT_DIR` and — this is the new part — `KICKOFF_HUB_ADDRESS`.

### The operator's own opencode config

`~/.config/opencode/opencode.json` sets three of the dropped names and **may not be edited by
anyone but him.** Between keeping those three working and telling him what to paste, this document
**chooses the paste**, for two reasons: keeping them is the alias-carry the clean break was granted
to avoid, and the failure if he does not paste is loud and safe — the tool server refuses to
connect and every tool call returns a sentence saying the session never said which project it
belongs to. A missed paste is a session that visibly cannot reach him, never a session whose words
go to the wrong topic.

**This paste is for the hand-started shape only** — `opencode serve` started by hand beside a
relay, the shape `kickoff-hub-attach` retires. Under `kickoff-hub-attach --run` (§13) **paste
nothing**: attach pins all eight variables into the engine, the tool server inherits them, and his
file as it stands — the three dropped names are inert, `CLAUDE_PROJECT_DIR: ""` is harmless under a
pinned `KICKOFF_HUB_PROJECT_DIR` — attaches unchanged. The block below would *break* that shape:
its six `-` entries overlay the pinned door, address and token path with "derive it yourself",
which is exactly wrong for a minted address (the tool server derives git's name, looks for another
door, and the agent reads "not said yet" for ever) and for a wall (no git to derive from, so it
refuses outright). §13.3 has the seven measured cases. If he wants one file for both shapes, the
only block that works in all of them is one with no `-` overlays — the file he has.

Replace the `environment` block of the `kickoff-channel` entry with exactly this, and **leave the
`command` line alone** — the path to `server.ts` is unchanged and nothing here renames or moves it:

```json
"kickoff-channel": {
  "type": "local",
  "command": ["bun", "<the same absolute path you already have>/plugins/kickoff-channel/server.ts"],
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
```

**Every one of the eight is there on purpose, and six of them say `-`.** This one entry covers every
project and every lane he has, and it is also the entry that runs when he starts opencode *from
inside a Claude Code session* — which inherits whatever that session was dispatched with. Overlaying
only the two that this configuration positively needs would leave the other six to arrive from
outside: an address minted for a different conversation, or a path to a different repository's
secret. `-` is how a config that can only overlay says "ignore what you were handed"; see the rule
above.

`CLAUDE_PROJECT_DIR: ""` **stays**, and it is the one line here that is not ours. It stops being
load-bearing — `KICKOFF_HUB_PROJECT_DIR` now outranks it — but it costs nothing and it is the second
lock on the incident in §5. One entry still covers every project and every lane: opencode sets the
child's cwd to the session's own directory, which is what `.` means.

**If he pastes half of it**, the failure is loud but the diagnosis used to be wrong: naming the
folder while missing `"KICKOFF_HUB_RELAY": "1"` makes the tool server hold the claim itself and race
his own relay for it, and the sentence an agent read then sent him hunting for "another session" that
was in fact the relay. An adapter that finds something listening on the relay socket for its own
conversation while it is not a producer says **that** instead, and names the line that is missing.

### The running session

A Claude Code channel plugin reloads only when its session does. The session live while this was
written holds the old code and must keep working: it reads `CLAUDE_PROJECT_DIR`, which is still set
by the engine, dials the hub directly, and sends the same `hello` it always sent. Nothing in this
interface changes the wire, the socket path, or the frames, so an old adapter and a new hub, or an
old adapter and a new relay, behave exactly as they did. **A restart to pick up new names is fine
and expected; a running process misbehaving is not, and nothing here does that.**

---

## 3. The four things that identify a connection

| thing | where it comes from | who owns it |
| --- | --- | --- |
| the **project** | the secret, resolved by the hub | enrolment, at a terminal |
| the **address** | the `lane` field of your `hello` | whoever dispatched you |
| the **instance** | a string you mint once per process | you |
| the **pid** | the socket's own credentials | the kernel |

The hub builds the conversation's address from **the project the secret resolved to** plus **the
name you sent**, and never from anything else on the wire. That construction is the whole security
argument: a `project_id` you put in `hello` is not consulted, so naming an address can only ever
reach a conversation of the project you already proved you are.

---

## 4. The address

### Who mints it

**Whoever dispatches the agent.** Set `KICKOFF_HUB_ADDRESS` and it goes on the wire verbatim. The
hub carries it and interprets nothing: worktree, room, function, ticket number — it has no opinion,
and it will never grow one.

When nobody sets it, an adapter falls back in this order:

1. **A default it can read off the machine.** The two adapters in this repo ask git: in a linked
   worktree they use git's own name for that worktree (the last segment of `--git-dir`), which git
   guarantees unique across a repository. This is a **default for a session nobody dispatched** — a
   developer opening a session by hand — and nothing more. It is not part of this interface; your
   adapter is free to have no such default at all.
2. **Nothing.** Send no `lane` field. That is the project speaking for itself, and it is byte for
   byte what every adapter shipped before addresses existed sent.

Never send `"lane": null`. Omit the field.

**To speak as the project from inside a worktree**, point `KICKOFF_HUB_PROJECT_DIR` at the main
working tree. Facts are derived from the directory you name, not from the process's cwd, so naming
the main tree derives no address and you get the project's own voice. There is no separate "no
address" spelling and there does not need to be.

### What the hub refuses

`refused{reason: "bad_lane"}`, checked after the secret resolves so that a caller with no valid
secret learns only "unknown project":

* **empty** — sending none is how you say "no address"; an empty one is a name the hub cannot
  address and guessing what was meant is how the wrong agent gets an answer;
* **over 64 bytes**;
* **exactly `.` or `..`**;
* **containing `/` or `\`** — so `CEO/steering` is **not addressable**. Adopters minting names freely
  will hit this. Nothing joins an address onto a path today, and the cheapest moment to close that
  door is before anyone is tempted;
* **containing any control character** — a tab or a newline forges a line in the audit file, which
  is one tab-separated record per line and interpolates its subject exactly as given. Any other
  control character reaches a topic title and the journal, where it is invisible and can reorder
  what a person reads.

**One more name is unavailable, and it is this interface's doing rather than the hub's.** `-` means
"as if this variable were not set" (§2), so `KICKOFF_HUB_ADDRESS=-` derives a default instead of
naming a conversation. The hub would happily address a conversation called `-`; you cannot ask for
one through this interface, and there is no reason to want to.

**`bad_lane` is permanent.** The same name is refused every time, so retrying it is a spin. Rename or
stop.

There are two more, unrelated causes that arrive wearing the same name, and what a person must do
differs for each — so a single sentence covering all three is a sentence that misdirects two thirds
of its readers.

1. **A hub older than your adapter** does not know the field, and a relay standing in front of it
   folds that condition onto `bad_lane` because the closed refusal set has nothing closer. Restart
   the hub.
2. **A relay holding a conversation you did not name** refuses you the same way (§9). You can tell
   this one apart with certainty: the hub only checks a name that is *there*, so `bad_lane` in
   answer to a `hello` that carried **no** `lane` cannot have come from the hub at all. Name the
   conversation the relay carries, or point at the relay for the project itself.
3. **A name the hub will not address**, which — if you checked the shape rules above before you
   dialled, as this section exists to let you do — is the rarest of the three.

Whatever you say, do not send the reader to delete and recreate a git worktree unless the name you
sent was actually derived from one. That instruction has already cost somebody uncommitted work.

### Uniqueness

**Uniqueness belongs to the minter, and the hub will not de-duplicate for you.** It is safe because
an address is scoped to the project the secret resolved to: two adopters minting `engineering` for
two different projects never collide, because the claim key is the pair. Two of *your* dispatches
minting `engineering` for one project do collide, and what you get is `already_claimed` on the
second — a refusal that names the reason and changes nothing. That is your bug, and the hub's job is
to say so rather than to prevent it.

### The echo, and why you must check it

`welcome` carries back the address the hub admitted. **If you named one and it does not come back,
refuse and disconnect.**

An unknown field inside a known frame is ignored on purpose — it is what lets a new adapter talk to
an old hub at all — but here it means the old hub admitted you **as the whole project**. You take the
project's one claim and its topic, your words land in its conversation, and the project's own
session is then refused. Nothing else in `welcome` can tell that apart from being given a place of
your own: the `project` title is registry-owned and you cannot predict it.

---

## 5. The credential

### The rule

**Configuration travels in the environment. The credential does not.** The environment carries a
*path* to the secret; the filesystem enforces `0600`. Never a token value in a variable, in an argv,
or in an image.

### The two incidents that paid for it

1. **`HERDR_PANE_ID` reached a bridge three levels down** from a terminal that never meant to talk to
   it. The environment is promiscuous: it is inherited by every descendant of every process, across
   engines and wrappers, and a variable set for one purpose arrives somewhere nobody was thinking
   about.
2. **`CLAUDE_PROJECT_DIR` crossed an engine boundary.** An opencode server started from inside a
   Claude Code session inherits it, and its MCP child inherits the server's whole environment — so
   the tool server resolved **another repository's secret**, silently, because it really did find
   one. opencode can only overlay a variable, never remove one, which is why the operator's config
   sets it to the empty string deliberately.

That second one is why `KICKOFF_HUB_PROJECT_DIR` now **outranks** `CLAUDE_PROJECT_DIR` instead of
sitting below it. A dispatcher's explicit word beats an engine's ambient one, and the blanking trick
becomes a second lock rather than the only one.

### The namespace is inherited too, and that is the third incident waiting to happen

Nothing about the prefix `KICKOFF_HUB_` stops it crossing the boundary incident 2 crossed. A Claude
Code session dispatched with `KICKOFF_HUB_ADDRESS=engineering` that starts an opencode server hands
that server, and its MCP child, the word `engineering` — for a session nobody dispatched, in a
different worktree, speaking for a conversation nobody opened. Behind a relay it is worse than a
wrong topic: the producer derives its relay socket from the address, so the two sides derive
different sockets and everything queues against a door nothing is listening at.

**The escape is `-`, and §2 is where it is specified.** Two things follow that a stranger has to be
told rather than left to work out:

* **A config that starts a second engine overlays the whole namespace**, not the one variable it
  cares about. Six of the eight in the operator's own entry say `-` for exactly this reason.
* **An adapter cannot detect that it got this wrong.** There is nothing on the wire that tells you
  which project you attached as beyond the registry title in `welcome`, which you cannot predict
  (§7, offer 8). The address echo (§4) catches a wrong *conversation*; nothing catches a wrong
  *project*. A wrong token is invisible from the inside — it finds a real secret, the hub admits it,
  and the words land in another project's topic. Get the token file right at the door, because
  there is no second chance to notice.

### Where the secret is, and how it is found

`herdr-tg enroll <repo>` writes it to `<repo>/.kickoff/hub.token` at mode `0600`. It refuses outright
if git would commit that file, because a secret in a public history cannot be untracked. **No message
can enrol anything**; it is a terminal-only act, on purpose.

An adapter finds it in one of two ways:

* **Told.** `KICKOFF_HUB_TOKEN_FILE` is used verbatim, with no search. This is the container answer.
* **Searched.** Upward from `KICKOFF_HUB_PROJECT_DIR`, because an engine routinely names a subfolder
  rather than the repo top. The search is **bounded by the top of the working tree**, so it can never
  wander into another repository, with exactly one legal crossing: from a linked worktree to the main
  working tree, because the secret is gitignored and is therefore never checked out into a worktree.
  **Without git the search checks the named directory and stops** — so an adapter on a machine with no
  git needs the secret at `<project dir>/.kickoff/hub.token`, or needs to be told the path.

**Resolve it afresh on every connection attempt, never once.** The operator may run `herdr-tg enroll`
while your adapter is running, and that is the documented recovery from `unknown_project`. An adapter
that caches "no secret" and stops retrying makes that recovery a lie.

### What the secret proves

**The project, and nothing else.** It does not name a conversation, it does not carry a display name,
and the `project_id` field beside it is never read. Put anything there; ours puts
`unknown-until-the-hub-says`, which is honest about what it is for.

---

## 6. The handshake, as a sequence

Everything here is one JSON object on one line, LF-terminated, UTF-8. Every object carries `v`
(protocol version, currently `1`, a **JSON number**), `id` (a **string**: opaque, yours, monotonic
per connection) and `t` (a **string**, the kind), flat, in one object:

```
{"v":1,"id":"f1","t":"say","text":"built it"}
```

**Types are not decoration here.** The tables below give one for every field, because getting one
wrong on `hello` produces the single worst failure mode in this system: a first line that will not
decode is not refused, it is *closed in silence* — the same thing you see when your uid does not
match (§10). No `refused` frame, no log you can read, just a socket that accepted, took a line and
went quiet. A stringly-typed environment makes `"pid":"12345"` the natural guess and it is fatal, so
here is one worked `hello`, complete:

```
{"v":1,"id":"f1","t":"hello","project_id":"unknown-until-the-hub-says","token":"<64 hex chars from the token file>","instance":"12345-1757000000000","repo":"$HOME/project","pid":12345,"lane":"engineering"}
```

### The steps

| # | who | what | if it goes wrong |
| --- | --- | --- | --- |
| 1 | you | `connect()` to `KICKOFF_HUB_SOCKET`. | Nothing listening: the hub is not running. Retry with backoff. |
| 2 | hub | Takes your peer credentials off the socket. **A connection from another uid is closed with no reply at all** — a refusal would confirm something is listening. The close happens after it has read your `hello`, so what you observe is a socket that accepted, took a frame, and went quiet. | You must run as the same user as the hub. |
| 3 | you | `hello` — **within 5 seconds**, or the connection is dropped in silence. | |
| 4 | hub | Admits or refuses. **A first frame that is not `hello` is refused `unknown_project`. A first line that will not decode closes silently. A first frame over the ceiling is refused `frame_too_large`.** | |
| 5 | hub | `welcome{project, lane?, topic_id?, limits}`. Check the address echo (§4). | |
| 6 | hub | `ping`. **Its own envelope `id` is the nonce.** | |
| 7 | you | `pong{ref: <the ping's id>}` — **within 5 seconds**. | No pong: you never become live, no topic is created, the connection ends. This is deliberate — a channel plugin that is not allowlisted boots and exits in a tenth of a second, and would otherwise leave an empty topic bound forever. |
| 8 | hub | Creates the topic and greets it, so it is visible in the operator's list. | |
| 9 | both | Frames, each answered by exactly one `ack` — **every frame after `hello`.** `hello` itself is never acked; `welcome` or `refused` is its answer. Build a watchdog on "a missing ack means something" without that exception and it tears down every healthy connection at the first frame. | |
| 10 | you | `bye{reason}`, then **wait for the kernel to take it** before exiting. | See the note on `bye` below. |

**Anything you say between step 3 and step 7 is kept and replayed**, bounded at 65 frames — the 64
a conforming adapter may have queued when it dials (§8, rule 10) plus the one frame it was half-way
through writing when the last connection ended, which goes back to the head of its queue — so a
full legal backlog carried into a reconnect is held whole. Over that bound the connection ends without becoming
live, and **every frame the hub read is acked `no` (`why: too-fast`) before the socket closes**. What
the kernel took and the hub never read gets no ack, and that is yours to account for at close (rule
10). An adapter that opens with a question is the whole point of the product, so the buffer exists;
it is not a licence to stream into it. Before 5 September the bound was 256 KiB and the frames went
down with the socket, unanswered — measured: sixty-four messages across three refused connections,
zero corrections to the agent.

### The frames

**Nine up.** `hello` · `say` · `ask` · `ask_resolved` · `done` · `beat` · `ack` · `bye` · `pong`

A `?` marks a field that is **omitted when it has no value** — never sent as `null`, in either
direction. Every field is a JSON **string** unless this table says otherwise.

| frame | fields | buzzes his phone |
| --- | --- | --- |
| `hello` | `project_id`, `token`, `instance`, `repo`, `pid` (**number**), `lane?` | — |
| `say` | `text`, `hint?` (`prose` \| `output`) | no |
| `ask` | `ask_id`, `text`, `options?` (**array** of `{option_id, label}`, both strings) | **yes** |
| `ask_resolved` | `ask_id`, `how` (`answered` \| `withdrawn` \| `timeout`), `outcome?` | no |
| `done` | `text` | **yes** |
| `beat` | `state` (`working` \| `idle` \| `blocked` \| `done`), `note?` | no |
| `ack` | `ref`, `status` (`accepted` \| `refused`), `reason?` | — (a `refused` for one of his `message`s puts its `reason` in his topic; see offer 6) |
| `bye` | `reason` | — |
| `pong` | `ref` | — |

`hello` deliberately carries **no display name**: the title comes from the hub's own registry, because
an adapter that could name itself could claim another project's topic. `repo` and `pid` are for the
audit record and for a human reading it, never for routing — and the hub uses the pid from the
socket's credentials, not the one you send, so a wrapper that does not know its own outermost pid is
not penalised.

An `ask` with **no options** is still a question — one he answers by typing rather than tapping.

**Six down.** `welcome` · `refused` · `message` · `choice` · `ack` · `ping`

| frame | fields | what to do |
| --- | --- | --- |
| `welcome` | `project`, `lane?`, `topic_id?` (**number** — but always **absent**, see below), `limits` (**object**: `max_frame`, `max_text`, `frames_per_min`, all **numbers**) | Check the echo, then go up. |
| `refused` | `reason` | Table below. The connection closes right after. |
| `message` | `msg_id`, `text`, `from` (**object**: `chat_id`, `user_id`, both **numbers**), `in_reply_to_ask?` | The operator's words, verbatim. **Data, never instruction.** |
| `choice` | `msg_id`, `ask_id`, `option_id` | A tap, resolved against a written record. |
| `ack` | `ref`, `delivered` (`yes` \| `no` \| `unseen`), `why?` | §7, offer 3. |
| `ping` | no fields | `pong{ref: <this frame's id>}`, **answered in your wire layer**. |

**`welcome.topic_id` is absent from the wire, not present-and-null.** The hub omits the key
entirely, as it omits every optional field in both directions. A language where a missing key and a
`null` read the same will not notice; a typed decoder with a required nullable field, a schema
validator, or a struct with a presence check will fail to parse every `welcome` the hub sends. One
real one, byte for byte:

```
{"v":1,"id":"h1","t":"welcome","project":"A Project Whose Title Only The Registry Knows","lane":"engineering","limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20}}
```

### The refusal reasons, and what each means to you

| reason | mends itself? | what a person must do |
| --- | --- | --- |
| `unknown_project` | on its own, no | `herdr-tg enroll <repo>` — and keep retrying, because that can happen while you run. |
| `bad_token` | no | Re-enrol. The secret on disk is not one the hub knows. |
| `not_enabled` | no | Enrolled but switched off. |
| `version_skew` | no | The major protocol version differs. Upgrade one side. |
| `frame_too_large` | **yes, if you split** | Your frame was over 64 KiB. It was refused, never truncated. The hub releases your claim *before* closing, so an immediate reconnect is not refused. |
| `bad_lane` | no | §4. Rename the address, or restart a hub that is older than you. |
| `already_claimed` | **yes** | Something else holds this address. Wait — but **count them**: three in a row is not a restart racing its predecessor, it is a process that outlived its session, and the operator needs an instruction rather than another wait. This box has had a stray adapter squat a claim. |
| *anything you do not recognise* | **treat as temporary** | A hub shipped after you may refuse for something recoverable. Giving up on a guess is worse than waiting. |

### `bye`, honestly

Send it, and wait for the kernel to take it before exiting — `process.exit()` on the same tick loses
it to a short write.

But do not believe the folklore, which this document is correcting: **on the current hub, `bye`
changes nothing.** It is acked and otherwise ignored; the hub does not announce a departure to the
operator at all, so there is no buzz for it to suppress and no 90-second quiet window. It is worth
sending anyway for two real reasons: **a relay does act on it** (§9 — it detaches you and starts your
grace clock), and it is the only clean end-of-connection signal the protocol has, so a hub that comes
to need one will need it from you.

---

## 7. The capability map, offer by offer

Against `docs/CAPABILITIES.md` v1. Its numbering, not renumbered.

### OFFERS — how you invoke each one

**1 · A conversation of its own.** Connect with a secret and an address (§4, §5, §6). The topic is
minted after your pong and greeted so it appears in the operator's list.
*Gap:* `welcome.topic_id` is **absent** — the key is not on the wire at all — and **no later frame
ever carries it**, so an adapter can never learn which topic it got. Nothing needs it — you must not
address anything with it — but if you were hoping to print it in your own log, you cannot.

**2 · Exclusivity.** You get it whether you ask or not: one live connection per `(project, address)`.
Invoke nothing; branch on `refused{reason}`. A dead claim is evicted (the hub checks `/proc/<pid>` of
the incumbent's socket), a live one is never displaced. To have two processes speak for one address,
see §9.

**3 · Delivery you can trust.** Every frame you send **after `hello`** gets **exactly one**
`ack{ref, delivered, why}` — including `beat`, `pong` and `bye`, so that a missing ack means
something. **`hello` is the exception and it is not acked at all**: the hub consumes it in the
handshake, before the acking loop exists, and answers it with `welcome` or `refused` instead. Do not
let a watchdog built on "a missing ack means something" wait for one; it will tear down every
healthy connection at the first frame, and the sentence above is the trap. Invoke it by keeping a
map from the envelope id you minted to what that frame *was*, in plain words, and by acting on all
three values:

* `yes` — it landed. But check `why` too: **`yes` with `why: "clamped"` means it arrived clipped.**
  The operator sees a message ending in `… (clipped)`; you would otherwise go on referring to a part
  he never read.
* `no` — it did not land, and it will not be retried. `why` is one of `too-fast`, `no-topic`,
  `telegram-refused`.
* `unseen` — it went out and could not be confirmed, and **it is never retried.** Telegram has no
  idempotency key, so re-sending a question would leave two live keyboards for it, both tappable
  forever. Folding `unseen` into success is a defect this project has already shipped and fixed;
  branch on `=== "yes"`, never on `!== "no"`.

*A rule, not a gap:* there is no ack timeout on the wire, and an ack names an id minted for ONE
connection — so a frame the kernel took whole on a connection that then ends is a frame no ack is
ever coming for. **Release your in-flight map on every close and tell whoever was waiting that its
fate is unknown** — not that it was lost (the hub may have delivered it and lost the ack with the
socket), and never re-send it (a question that did land has live buttons; a second copy is two
menus). The hub keeps its half: on every way a connection is refused before it is live, every frame
it read is acked `no` first, so what stays unknown is only what the kernel took and the hub never
read. `hub-link.ts` does the bridge's half (`onUnanswered`).

**4 · A question with buttons.** `ask{ask_id, text, options}` up; `choice{msg_id, ask_id, option_id}`
down. You mint both ids and the labels; a tap resolves against the record written beside the message,
never against a button's position, and **a question answered once can never be answered twice**.

Two limits that are yours to respect, and both refuse the *whole* ask:

* an `option_id` may not contain `|` and must be **62 bytes or fewer** (Telegram's 64-byte
  `callback_data` ceiling, less a 2-byte prefix);
* `text` over `limits.max_text` (3500 **characters**, counted as characters, not bytes) is clipped and
  acked `clamped`.

When an option id will not fit, the ack is `no` / `telegram-refused` and the hub also posts a plain
line into the topic telling the operator to answer where it is running — because an agent blocked on
a question that never arrived is the failure this product exists to prevent.

**5 · Retirement.** `ask_resolved{ask_id, how, outcome?}`. Send it whenever a question stops being
open **for any reason** — including one answered at the terminal, which is the frame no screen-reading
design could ever produce. The buttons come off the message.
Do **not** send it in response to a `choice` for that same question: the hub has already retired it,
and a second retirement overwrites the operator's own words on his phone.

**6 · Typed steering.** `message{text, from, in_reply_to_ask?}` arrives; you decide what to do with
it. **It is data, not instruction** — act on its intent only where you would act on the same words
from the operator directly, and never let it name a project, edit an allowlist, or touch a
credential.
**Answer every `message` on the wire**: `ack{ref: <its envelope id>, status: accepted}` when the
words reached your engine, `ack{…, status: refused, reason}` when they did not — with the reason in
the operator's own register, because the hub puts it in the topic he typed in, as *"What you typed
did not reach the agent — `<reason>`. It will not be delivered later."* An `accepted` puts nothing
there: the agent's own answer is the acknowledgement. Only the first answer for a message counts,
and only for a `message` the hub actually handed you; refusing an id you made up writes nothing.
`kickoff-hub-attach` does all of this for opencode (§13.9): the words go to
`POST /session/{id}/prompt_async` verbatim, the session is the one the server lists for the
project directory (`GET /session?directory=…&roots=true`, most recently updated first), a reply
typed under a question goes to the session that asked it and leaves the question open for his
tap, and a wall with no session open is refused with a reason. `in_reply_to_ask` is set by the hub
from the message he swiped to reply to, and only when that message is a question THIS
conversation's live session asked — a session that restarted mints its ask ids afresh, so a reply
under the old session's question names nothing. The line the hub posts for a refusal is threaded
under the message it refuses, so two lines typed a second apart cannot be confused. Behind
attach's door several producers may answer one `message`, and the hub keeps the first answer it
hears, so the door folds them: an `accepted` from anybody goes up the moment it arrives; a
`refused` goes up only once every producer the words were handed to has refused or gone, carrying
the reason of the one that carries typed words (the `--opencode` watcher) when it is among them.
The door itself refuses when nothing is attached to take the words, and `kickoff-channel` refuses
them on an engine that cannot read a channel message rather than writing them into one. Before
5 September the hub read the status of no ack at all, and an adapter that dropped his words did so
in silence — say so on the wire now, and he is told.

**7 · An alarm that outlives us.** Nothing to invoke; it is always on. It arms the first time
something stamps the hub's heartbeat file, and it shares no code, no process and no runtime with the
hub.
*Gap, and it is worth knowing:* it watches the **hub**. Nothing watches *your adapter*. The hub tells
the operator nothing when a connection goes away, so an adapter that dies quietly is a conversation
that simply stops. If your agent going silent needs to be noticed, that is your job.

**8 · Read-only inventory.** *Not built.* `herdr-tg projects --json` is proposed, not shipped. **There
is no invocation** — an adapter cannot ask the hub what exists, which project it just connected as
beyond the title in `welcome`, or whether anything else holds an address. Every question of that
shape is answered by whoever dispatched you, or not at all.
*And this is a safety gap, not only a convenience one:* the address echo (§4) catches a wrong
conversation, and nothing catches a wrong **project**. Attach with a token file belonging to some
other repository and everything looks right from inside — a real secret, an admission, a title you
could not have predicted anyway. §5 says how a token file arrives wrong without anybody choosing it.

### REQUIRES — how you satisfy each one

**1 · Enrolment, at a terminal, per repo.** `herdr-tg enroll <repo>`. Not something your adapter can
do, arrange, or work around; §5.

**2 · An address unique within its project.** §4. Set `KICKOFF_HUB_ADDRESS`, keep it unique yourself,
keep it inside the shape rules, and check the echo.

**3 · An adapter that speaks hub-proto and answers a ping.** §6 for the sequence, §8 for the twelve
ways a wire implementation gets it wrong. Answer `ping` **in your wire layer**, not in your
application layer: liveness is what keeps your claim, and it must not depend on anything upstairs
being awake.

**4 · One connection per address.** §9.

---

## 8. Writing the wire — the twelve rules

There is **one** implementation of this wire in the repo — `plugins/kickoff-channel/hub-link.ts` —
and every file that speaks the wire shares it: the Claude tool server, and attach's `relay.ts`,
`opencode.ts` and `check.ts`. A test fails if any of them starts writing its own again, or if any
other file under `adapters/` or `plugins/` so much as mints a `hello`.

That rule was bought. `adapters/opencode-bridge/bridge.ts` carried a fork of an early version for a
day, and in that day it drifted by twelve invariants — every one of them a defect that had already
been found and fixed on the other side, still live in the copy because nobody had reason to look at
it twice.

For you, that list is more useful as a checklist than as history. If you implement this wire in
another language, these are the twelve things to get right, and each of them cost somebody a real
debugging session:

1. **The trailing newline is appended in exactly one place.** Omitting it made the far side hang
   forever with no error and no close — measured at 5.01 s, zero bytes read, connection still open.
   Only a client-side timeout catches that.
2. **Read exactly one line. Never to EOF.** Reading to EOF surfaces a reset *after* a good frame has
   arrived, and loses the frame with it.
3. **Bound one frame, not the conversation.** A cumulative ceiling ends a healthy stream in silence
   that reads as a disconnect the peer never performed. 64 KiB, terminator included, restored after
   every complete line.
4. **Short writes are normal, and the kernel drops what it will not take.** Measured: 131 MB offered,
   245 KB accepted, the rest gone and reported as sent. Nothing leaves your queue until its last byte
   is accepted, and you need a **drain handler** — a frame the kernel refused must not sit until you
   happen to send another one. An engine's prompts are sporadic; a permission ask can sit unsent
   indefinitely with the agent blocked.
5. **A half-written frame is re-sent whole, from the start.** Never resumed. Keeping the byte offset
   across a reconnect writes the *tail* of a frame whose head died with the old socket — and that
   headless line is the first thing the hub reads, which closes **silently**, with no refusal, so you
   never learn why.
6. **Drop connection-scoped state on close.** The `hello`/`pong`/`bye` you owe *this* connection die
   with it; a stale one written onto the next connection is two `hello`s the protocol does not have.
7. **Reset your backoff when you are `welcome`d, not when you connect.** A hub that accepts and then
   refuses is otherwise redialled at a flat one second, for ever. `already_claimed` is not permanent,
   so a squatting claim becomes a permanent 1 Hz hammer.
8. **An unknown `refused` reason is temporary.** Treating it as permanent means a hub shipped after
   you stops you for the life of the process.
9. **"No secret" is temporary too.** The documented recovery is `herdr-tg enroll` *while you are
   running*. An adapter that sets a permanent flag and never retries makes that recovery impossible —
   and says so only on a stderr nobody reads.
10. **Your send function must return what actually happened**, and the caller must use it. "Written to
    a live socket", "parked in a queue for a link that has never come up" and "refused for size" are
    three different things, and every one of them was once reported to an agent as success. Bound the
    queue, and when the link goes down for good, **hand the queued frames back** to whoever was told
    they were on their way. And when a connection ENDS — for any reason — every frame the kernel took
    whole that has no ack yet will never get one: hand those back too, as *unconfirmed*, and never
    re-send them. One that did reach the hub was delivered, and a second copy is a second message on
    his phone, or a second live menu for one question.
11. **Nothing but `hello`, `pong` and `bye` may go out before `welcome`.** A connected socket proves
    only that something accepted; the hub can still refuse and close, and a frame written into a
    doomed connection is a frame you reported delivered and then threw away.
12. **Answer `ping` in the wire layer**, and never depend on a caller upstairs being awake for it.

Two more that are not defects but are easy to miss: an **unknown frame kind is ignored**, not fatal;
and **a line that will not decode is one bad frame**, not a dead peer — the hub survives one from you
and you must survive one from it, because that is exactly what a peer one version ahead sends.

---

## 9. Two things behind one address

The hub admits one live connection per address. If two processes must speak for one conversation,
**join them on your side** — the hub never learns there were two, and this is not a seventh hub
capability.

`adapters/kickoff-hub-attach/` is our relay — the door it opens is `relay.ts` inside it. One process
per addressable thing holds the claim; producers attach to it over a local socket that **speaks
hub-proto unchanged**, so a producer needs no second wire contract and nothing in it changes but the
address it dials.

* The **relay** gets `KICKOFF_HUB_PROJECT_DIR` and, if the conversation has one,
  `KICKOFF_HUB_ADDRESS`. It listens on `KICKOFF_HUB_RELAY_SOCKET`, or on the derived path below.
* Each **producer** gets the same `KICKOFF_HUB_PROJECT_DIR` and the same `KICKOFF_HUB_ADDRESS`, plus
  `KICKOFF_HUB_RELAY=1`. That flag, and only that flag, is what makes a process a producer;
  `KICKOFF_HUB_RELAY_SOCKET` says where the door is and is read by the relay too, so setting it
  alone does not put you behind one. Without it the path is derived, below. A producer must
  **never** fall back to dialling the hub when it cannot find its relay: that is two writers racing
  for one claim, which is the whole thing the claim exists to prevent. Refuse and say so.

**The derived path**, documented so a second implementation can meet ours:

```
<KICKOFF_HUB_RELAY_DIR>/<first 16 hex chars of sha256(mainWorkingTree + "\0" + address)>.sock
```

with `address` the empty string when there is none. Hashed because `sun_path` caps at 108 bytes and a
repo path plus a name goes past it easily; NUL-joined so that no two `(repo, address)` pairs can be
spelled two ways onto one socket.

**`mainWorkingTree` is a git fact, and it is NOT `KICKOFF_HUB_PROJECT_DIR`.** It is the top of the
repository's *main* checkout — `dirname` of what `git rev-parse --git-common-dir` answers, resolved
against the directory you asked about, which is the repo top in a main checkout and the main
checkout's top when you are in a linked worktree. `KICKOFF_HUB_PROJECT_DIR` is routinely a subfolder
of that (§5 says why the secret search goes upward), and pointing it at a subfolder derives a
*different* socket from the one the relay is listening on. Both sides then look right and never
meet.

So: **both sides derive it from facts they already have only if both sides have git.** An adapter
that cannot run git plumbing — a container (§10), or any language you would rather not shell out
from — must be **told** the path with `KICKOFF_HUB_RELAY_SOCKET`, and that is the supported answer
rather than a workaround. `docs/examples/attach-from-the-document.ts` does exactly this and refuses
to guess; it does not implement the derivation, so the worked example proves the relay path but not
this formula.

### What a relay does that a pipe does not

It answers `hello` itself with the `welcome` it holds (address echo included, so your
refuse-rather-than-impersonate check keeps working unmodified); rewrites envelope ids, because two
producers both mint `f1` and an ack naming the wrong frame tells the wrong agent its message was
lost; **namespaces `ask_id`**, because the hub resolves a tap by ask id and without this a tap on one
agent's question is delivered to the other; answers the hub's `ping` itself so a wedged producer
cannot cost the address its claim; shares the queue by how many producers are actually attached; and
**ends your connection when its own link to the hub drops**, because the wire has no way to un-welcome
anybody and a producer whose link is up would otherwise say "sent" for a message nothing carried.

It also takes on the lifecycle job that standing in front of the hub took *away* from the hub. The
hub retires a dead asker's questions from the `instance` in `hello` and the pid on the socket — behind
a relay both are the relay's, for every producer, for ever. So:

* **You are known by the `instance` in your own `hello`, never by your socket.** Keep it for the life
  of your process; change it when you restart. A producer that reconnects under the same instance is
  the same voice and still gets the tap it is waiting on.
* **When your socket goes and nothing comes back under your name within the grace window** (default
  90 s), your open questions are withdrawn and their buttons come off.

### Four things a relay does not inherit from the hub

Written down here because none of them is stated anywhere else:

1. **It never pings its producers.** A producer's liveness behind a relay is unchecked, and a
   producer's own `pong` is dropped as an answer to a ping nobody sent.
2. **It refuses a producer whose address does not exactly equal the one it holds** — `bad_lane`,
   including a producer that sends **no** address against a relay holding one. An adapter that
   cannot be told an address therefore cannot join a relay that holds one; every adapter in this
   repo can be, through `KICKOFF_HUB_ADDRESS`, so what remains is a configuration mistake rather
   than a hole: set the same address on the producer and its relay and it attaches. This is also
   the one refusal a producer can attribute with certainty — see §4, cause 2.
3. **It checks your secret against the one it authenticated with**, so a producer still needs the
   token file even though the relay holds the claim. Defence in depth over the socket directory's
   permissions, and it costs nothing because the frame already carries it.
4. **It answers `version_skew` to a `hello` with no `instance`.** A small overload of a reason that
   otherwise means a protocol mismatch; do not be confused by it, and always send an `instance`.

---

## 10. A container, worked

One conversation, in a container, talking to a hub on the host — with `kickoff-hub-attach` as the
container's entrypoint (§13.1). attach holds the claim, opens a door of its own, and starts the
engine as its child, so the wall has one process to run and one to stop.

**Mount two things.**

| mount | as | why |
| --- | --- | --- |
| the host's `/run/user/<uid>/kickoff/` **directory** | the same path inside | Never the socket **file**: the hub unlinks and rebinds it on every start, and a bind-mounted file becomes a stale inode the moment the hub restarts. The directory is `0700`. |
| the token file, read-only | anywhere, e.g. `/run/secrets/hub.token`, mode `0600` | The credential is a file the filesystem protects, not a variable the environment leaks. |

**Set three things**, and run attach as the entrypoint with the engine after `--run`:

```
KICKOFF_HUB_PROJECT_DIR=/workspace          # where the repo is mounted; for the audit record
KICKOFF_HUB_TOKEN_FILE=/run/secrets/hub.token
KICKOFF_HUB_ADDRESS=<the name your dispatcher minted>

# entrypoint (docker: behind --init, which reaps AND forwards signals; bwrap: without --as-pid-1,
# whose init reaps and forwards NOTHING — §13.5 says how a bwrap wall is actually stopped):
kickoff-hub-attach --opencode http://127.0.0.1:9700 --run opencode serve --port 9700 --hostname 0.0.0.0
```

**The relay socket is NOT one of them, under `--run`.** §9's derived path is a git fact, and a
container with the repo mounted but no git binary — the ordinary case — cannot compute one. attach
does not need it to: with a child to hand it to, it makes a **private** door of its own under the
temporary directory and pins it into the child's environment, so the tool server the engine spawns
finds it with nothing configured (§13.3). Nothing outside the wall dials that door, so nothing
outside the wall needs its name. (Without `--run` — a bare producer in a container — you would still
be told the door with `KICKOFF_HUB_RELAY_SOCKET`, off the relay's own first line of output.)

**Run as the same uid as the hub.** This is not optional: the hub reads the peer credentials and
closes a connection from another uid **with no reply at all** — you will see a socket that accepts and
then does nothing, which is the least debuggable failure in this system. Before that, the directory
is `0700`, so a foreign uid cannot even look inside it; `--check` says so in words.

**And if the wall remaps the uid** (`bwrap --unshare-user --uid 0`), the hub still reads the
operator's uid — measured — but the *path* is derived from the tenant's: `/run/user/0/kickoff/hub.sock`,
where nothing is. Either keep the operator's uid (do not pass `--uid`/`--gid`), or bind the directory
at `/run/user/<tenant uid>/kickoff/`, or set a fourth variable, `KICKOFF_HUB_SOCKET`, to where it
really is.

**What happens if each is missing:**

| missing | what you see |
| --- | --- |
| the socket directory mount | connect fails, nothing is listening. Retry with backoff; the hub may simply be down. |
| the socket **file** bind-mounted instead of its directory | it works until the hub restarts, then connect fails for ever against a stale inode. |
| matching uid | the connection is accepted and then closed in silence. No `refused`, no log you can see. |
| the token mount | `unknown_project` if you send something wrong, or your own "no secret at `<path>`" if you check first. **Keep retrying** — this mends when the file appears. |
| `KICKOFF_HUB_TOKEN_FILE` | the search starts at `KICKOFF_HUB_PROJECT_DIR`; with no git in the container it checks that one directory and stops. Set the variable. |
| `KICKOFF_HUB_PROJECT_DIR` | refuse to start, and say which variable. Never guess from cwd. |
| `KICKOFF_HUB_ADDRESS` | you connect as **the project itself** — taking its claim and its topic. Inside a container there is usually no git to derive a default from, so this is the variable to get right. |
| `KICKOFF_HUB_RELAY_SOCKET`, a bare producer behind a relay (no `--run`) | with no git in the container the derived path cannot be computed, so the adapter refuses to start and names the variable. Under `--run`, attach makes its own private door instead, so this is not needed. |
| a JSON type wrong in `hello` (`"pid":"12345"`) | **exactly what a uid mismatch looks like**: accepted, one line read, then silence. A first line that will not decode is closed with no `refused` frame at all. §6 has one worked `hello` with every type in it; check yours against it before you go looking for anything else. |

One caution this document flags rather than asserts: the hub identifies your process by the pid it
reads from the socket, and evicts a dead claim by looking for `/proc/<pid>`. An adapter in its own PID
namespace may therefore be judged by a pid that means something different on the host side. It has not
been measured here. If you run adapters in PID namespaces, measure it before you rely on eviction.

---

## 11. Where this document cannot make a stranger self-sufficient

Named rather than papered over, because the test this was designed against is an adopter with this
file and the `crates/hub-proto` docs and nothing else.

1. **The engine half is not specified and cannot be.** This file covers seam ① — adapter to hub. What
   your adapter *reads* from your engine, and how it injects the operator's answer back into an
   agent's turn, is yours. `docs/INTERFACES.md` describes both of ours as worked examples; neither is
   a contract.
2. **The exact sentences an agent should read are not here.** The three-outcome vocabulary — reached
   / queued / permanent — is in `plugins/kickoff-channel/server.ts`, and it is the least portable and
   most argued-over part of this system. If your agent-facing strings can promise something that is
   only true on some engines, read that file before writing them.
3. **There is no conformance suite you can run**, and there is one worked example.
   `docs/examples/attach-from-the-document.ts` is the smallest adapter of §1, written from this file
   and importing nothing of ours; `adapters/kickoff-hub-attach/test-two-producers.ts` runs it against
   the real door, and that run is what found the ordering defect now fixed in §1. It is an example,
   not a suite: it covers the good-day path and none of §8. Our own suites are
   `bun test-against-a-fake-hub.ts` and, under `adapters/kickoff-hub-attach/`, `bun
   test-two-producers.ts`, `bun test-what-breaks-it.ts`, `bun test-against-fakes.ts`,
   `bun test-check.ts`, `bun test-run.ts`, plus `cargo test -p herdr-tg the_real_plugin -- --ignored`
   — the last is the real tool server against the real hub with only Telegram faked. A third-party
   adapter has no equivalent, and a fake written from your own reading of this document proves only
   that you agree with yourself.
4. **The relay's local protocol is hub-proto, but its lifecycle rules are not in `hub-proto`.** §9
   lists the four differences; they live in `adapters/kickoff-hub-attach/` (`relay.ts` and
   `ledger.ts`), and if you write your own relay you are re-deriving them from prose.
5. **`limits` is told to you, not enforceable in advance.** `max_text` counts characters, `max_frame`
   counts bytes, and the option-id ceiling is in neither — it is Telegram's, and you learn it by
   being refused.
6. **The relay's derived socket path is git arithmetic, and the worked example does not do it.**
   §9 gives the formula and names its input exactly, but the example refuses unless
   `KICKOFF_HUB_RELAY_SOCKET` is set rather than deriving one — so nothing here demonstrates that a
   second implementation's derivation meets ours. If you implement the formula, check your answer
   against the path a running relay prints before you trust it.

---

## 12. Not in scope

Said plainly so nobody looks for it here.

* **`crates/`.** This is adapter-side. The hub already accepts an address and nothing on the wire
  changes.
* **The conversations redesign** (`docs/CONVERSATIONS.md`), rooms, enrolment and topic cleanup.

(Supervision and systemd units used to be listed here; they are built — `kickoff-hub-attach` and its
`deploy/kickoff-hub-attach@.service` template, §13. So was the pre-pong 256 KiB bound; it is 65
frames now and the hub acks what it refuses, §6.)

---

## 13. One command

<!-- BUILT, 4 September 2026. This section was a design; it is now code. Every file it names under
     `adapters/kickoff-hub-attach/` exists; `adapters/fanin/` and `adapters/opencode-bridge/` are
     gone, folded into it, every check they held moved across. It reads as a design in places (the
     tense of "becomes", the table of "what the build must touch") because it was written before the
     build; that is left as the record of the decisions, and where it says a file "is today" the
     file is now `adapters/kickoff-hub-attach/`. Written against one test: an adopter who reads this
     section and nothing else can start a worker. -->

The operator looked at what it takes to put one opencode agent on his phone and asked, in his
words, *"is there a way I don't have to type all of that stuff?"* — because a Claude worker is one
command and an opencode worker is **three hand-started processes** for one conversation:

| today | what it does | who starts it |
| --- | --- | --- |
| `adapters/fanin/fanin.ts` | holds the one slot at the hub for this address | he does, by hand |
| `opencode serve` | the engine; its MCP tool server attaches to the relay | he does, by hand |
| `adapters/opencode-bridge/bridge.ts` | watches the server's event stream, relays its prompts | he does, by hand |

That is a design smell, not a documentation one. This box has the corpse of it right now: an event
bridge still retrying against a relay socket whose relay died hours ago, because when one of three
hand-started processes goes, the other two hang.

**`kickoff-hub-attach` is one process that does all three jobs**, and can start the engine as its
child so that a wall — bwrap or docker, the machinery the operator is building — has exactly one
entrypoint. It reads the namespace of §2 through the one reader, holds the claim, exposes the door
of §9 so the engine's tool server attaches to it unchanged, watches the opencode server when told
to, and proves an environment can reach the hub before anyone trusts it.

Three things do not change. **The Claude path**: `plugins/kickoff-channel/server.ts` dials the hub
directly, is already one command, and is not touched — it ships to the session the operator is in
as this is written. **The wire**: `crates/` and `hub-proto` are untouched, and
`plugins/kickoff-channel/hub-link.ts` stays the one implementation, guarded by a test. **The
stranger's adapter**: `docs/examples/attach-from-the-document.ts` must still attach to attach's door
without a line of it changing, and it is run twice below — once as a producer at the door and once
as the "engine" a wall starts.

### 13.1 The command line

```
kickoff-hub-attach [--opencode <url>] [--run <command...>]
kickoff-hub-attach --check [--opencode <url>] [--run <command...>]
```

Everything about *which* project, *which* conversation, *where* the secret is and *what* to dial
comes from the environment, §2, and only from there — `plugins/kickoff-channel/attach.ts` is the
only file that reads a variable, and this command adds no way round it. So a project directory is
never a flag: it is `KICKOFF_HUB_PROJECT_DIR`, and `.` means "the directory I was started in, and
whoever typed this vouches for it".

| flag | what it does | default |
| --- | --- | --- |
| *(none)* | Hold the claim for `(project, address)` and open the door. This alone is what `adapters/fanin/` is today, and it is what a Claude worker in a wall needs. | — |
| `--opencode <url>` | Also watch the opencode server at `<url>`: its questions and permission prompts go to the phone as `ask`, a tap goes back to the server's own reply endpoint. This is what `adapters/opencode-bridge/` is today, minus its process. The URL is the flag's value and nothing else; `OPENCODE_URL` retires with the bridge. | not watching |
| `--run <command...>` | Start `<command...>` as this process's child, in the project directory, with the namespace pinned in its environment so that any adapter descending from it finds the door (§13.3). When the child exits, say `bye`, close the door, exit with the child's status (§13.5). Everything after `--run` is the command; nothing after it is read as a flag. | no child |
| `--check` | Prove this environment can reach the hub, one plain line per fact, then exit 0 if every fact holds and 1 if any does not. Sends `hello` and `bye` and nothing else; creates no topic (§13.4). `--opencode` and `--run` may stay on the line — the check reads them for what it can verify and starts nothing — so a wrapper runs its real line with `--check` in front of it. | — |

**Exit status.** `0` a clean end; `1` a `--check` that found something to fix; `2` a refusal to
start, with the sentence naming the variable on stderr — the same `2` the relay and the bridge use
today; `127` the `--run` command could not be found; otherwise **the child's own status**, with a
child killed by signal *n* reported as `128 + n`, which is what `docker stop` and systemd read.

**Its first lines** say what it is speaking for, and where the door is — because §10 says a
container behind a relay is told the door "from the relay's own first line of output", and that has
to stay true:

```
kickoff-hub-attach: speaking for /home/<you>/scratch/oc-dogfood · lane-0904-1200
kickoff-hub-attach: the door is /run/user/<uid>/kickoff/fanin/9dcb04a8b0e28966.sock
kickoff-hub-attach: connected as "oc-dogfood · lane-0904-1200"
```

The name in quotes is the hub's own title for the conversation — the project's title, a separator,
and the address, which the hub clips from the left with an ellipsis when it is long
(`…fy-0904-check`). It is printed once, as the hub sent it.

Every line it prints for itself is prefixed `kickoff-hub-attach:`; the child's output passes
through unprefixed, so a journal shows both and a reader can tell them apart.

#### Worked invocation 1 — a local worker on this box

Once, per project, at a terminal: `herdr-tg enroll <repo>`. Once, per box: bun and opencode on
`PATH`, and the shim `~/.local/bin/kickoff-hub-attach` that `scripts/install-attach.sh` writes
(§13.6). Then, in the worktree the worker is for:

```
cd ~/scratch/oc-dogfood
KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach --check --opencode http://127.0.0.1:9711 --run opencode serve --port 9711
KICKOFF_HUB_PROJECT_DIR=. kickoff-hub-attach         --opencode http://127.0.0.1:9711 --run opencode serve --port 9711
```

The second line is the worker: the claim, the door, the server, the watcher, and the tool server the
server spawns — one process tree, one command, and when the server dies the whole thing says
goodbye and exits with its status. The address is git's name for the worktree, or none in the main
tree, exactly as §4 says; a dispatcher that minted one sets `KICKOFF_HUB_ADDRESS` in front of the
same line. `--port` is not optional: `opencode serve` without it picks a random port, and the
watcher would then be watching nothing. The number appears twice on the line so the server and the
watcher can never disagree about it.

#### Worked invocation 2 — a wall's entrypoint

This is the shape kickoff's wrapper copies. docker is shown because it is the one everyone can
read; the bwrap differences are the two sentences after it.

```
docker run --rm --init \
  --user "$(id -u):$(id -g)" \
  -v "/run/user/$(id -u)/kickoff:/run/user/$(id -u)/kickoff" \
  -v "$repo/.kickoff/hub.token:/run/secrets/hub.token:ro" \
  -v "$worktree:/workspace" \
  -p "127.0.0.1:$hostport:9700" \
  -e KICKOFF_HUB_PROJECT_DIR=/workspace \
  -e KICKOFF_HUB_ADDRESS="$address" \
  -e KICKOFF_HUB_TOKEN_FILE=/run/secrets/hub.token \
  <image> kickoff-hub-attach --opencode http://127.0.0.1:9700 --run opencode serve --port 9700 --hostname 0.0.0.0
```

Line by line, each is one of §10's rules or one of the measurements in `docs/TAXONOMY.md`:

* `--init` — the wall's own reaper. attach forwards signals and propagates the child's status; it
  does not reap orphans, and refuses to be PID 1 with a child (§13.5). docker's init (tini) also
  forwards the signal `docker stop` sends, so the wall stops the way a desk does. **bwrap is not
  docker here**: without `--as-pid-1` its init sits at PID 1 and reaps — so do not pass
  `--as-pid-1` — but it forwards **nothing**, and a signal to the bwrap process ends bwrap and leaves
  the wall running with the claim held. §13.5 says how a bwrap wall is stopped; it is not by
  signalling bwrap.
* `--user "$(id -u)"` — the hub reads the peer's uid **in its own namespace** and closes any other
  uid without a word, and the directory is `0700` and the socket `0600` besides. A foreign uid fails
  before any of that: it cannot stat inside the directory, and the check says "this user may not
  look inside … run as the same user as the hub" when it sees that; a foreign uid that can (root
  in a rootful container) is closed in silence, and the check names that too. Under
  `bwrap --unshare-user --uid 0` the tenant sees itself as uid 0 and the hub still reads the
  operator's uid, measured — but the tenant derives `/run/user/0/kickoff/hub.sock`, where nothing
  is; §10 gives the three ways out (keep the uid, bind the directory at the tenant's path, or set
  `KICKOFF_HUB_SOCKET`). The check does not compare uid numbers, for exactly that reason.
* the **directory** `/run/user/<uid>/kickoff/`, never the socket file — the hub unlinks and rebinds
  its socket on every start, and a bind-mounted file is a stale inode after the first restart.
* the secret as a **file**, read-only, and its **path** in `KICKOFF_HUB_TOKEN_FILE`. The value never
  travels in the environment; attach refuses if anyone tries (§13.4).
* the worktree at `/workspace`, and `KICKOFF_HUB_PROJECT_DIR` naming it. A linked worktree's `.git`
  is a file pointing at the main repository's `.git/worktrees/<name>` — which is **not** in the
  wall — so git answers nothing here, no address can be derived, and `KICKOFF_HUB_ADDRESS` is the
  variable to get right: without it the wall speaks as the whole project and takes its claim.
* no `KICKOFF_HUB_RELAY_SOCKET`. Under `--run`, when git cannot derive a door and nothing named one,
  attach makes a **private** door in a folder of its own and hands it to the child (§13.3). Nothing
  outside the wall needs to reach that door, so nothing outside the wall needs to know its name.
  The wrapper therefore sets **three** variables, the three §10 always required — four when it
  remaps the uid, as the bullet above says.
* `--hostname 0.0.0.0` and `-p` — so kickoff on the host can reach the server to open sessions and
  send prompts. The watcher still dials `127.0.0.1:9700` inside. Under bwrap without
  `--unshare-net` the wall shares the host's loopback and there is no `-p`; then every wall needs a
  port of its own, and kickoff mints it the way it mints the address.
* the image holds bun, opencode, `plugins/kickoff-channel/` and `adapters/kickoff-hub-attach/` —
  the tool server is a bun script the engine spawns, so bun is in the image whatever attach is. It
  holds **no** copy of the operator's `~/.config/opencode/opencode.json`: that file carries his
  provider keys, and its `environment` block is built for the host, not a wall (§13.3).

### 13.2 What becomes of `adapters/fanin/` and `adapters/opencode-bridge/`

The criterion the operator set is *"a clean house, and an house that makes sense"*, and the test of
it is that a stranger opening `adapters/` finds **one thing to run**. Two directories each with a
`start` script is two things. So both go, and their logic becomes modules of the one command:

```
adapters/kickoff-hub-attach/
  main.ts            the command: flags, the order of operations, signals, exit  (run this)
  relay.ts           the door — producers, envelope ids, ask-id namespacing, the queue share
  ledger.ts          who the producers are and what they wait on; survives a restart
  opencode.ts        the watcher — events to asks, taps to replies
  check.ts           --check
  run.ts             --run, and the ONE place anything is spawned
  README.md          the operator's page: what to type
  package.json       start and test
  test-harness.ts    the shared rig (fake hub, real producers, a real worktree)
  test-two-producers.ts · test-what-breaks-it.ts · test-against-fakes.ts   moved, §13.7
  test-check.ts · test-run.ts                                                new, §13.7
```

| today | becomes | why |
| --- | --- | --- |
| `adapters/fanin/fanin.ts` | `relay.ts` (the door, lines "Writing to a producer" through "The door") + the startup, signal and `bye` code folded into `main.ts` | The relay's logic is the product; its process was only a process. |
| `adapters/fanin/ledger.ts` | `ledger.ts`, moved unchanged | It already knows nothing about sockets. |
| `adapters/fanin/test-harness.ts` | moved; `FANIN` becomes `ATTACH`, `startFanin` becomes `startAttach` | Same rig, same spawn, new path. |
| `adapters/fanin/README.md` | folded into the new `README.md` | One page for one command. |
| `adapters/fanin/package.json` | gone | |
| `adapters/opencode-bridge/bridge.ts` | `opencode.ts` — the mapping (`onOpencodeEvent`), `answer()`, `watch()` and the open-question record; its `HubLink`, startup and signals go | What is genuinely opencode's stays; the wire was already shared. |
| `adapters/opencode-bridge/README.md` | folded into the new `README.md` | |
| `adapters/opencode-bridge/package.json`, `.gitignore` | gone | |
| `adapters/opencode-bridge/test-against-fakes.ts` | moved; spawns `main.ts --opencode <fake url>` | §13.7 |

**How the watcher joins the relay: as a producer, at the door, in the same process.** Today the
bridge attaches to the relay with two variables and not a line of its own changing, proven by
`test-two-producers.ts` Part 2c. Collapsing the process boundary changes nothing on the wire: the
watcher in `opencode.ts` holds a `HubLink` whose socket is attach's own door and whose `hello`
carries the same secret, the same address and an instance of its own. The relay half greets it,
numbers it, namespaces its ask ids, shares the queue with it and — when the hub link drops — ends
its connection exactly as it ends every other producer's, so the watcher's queued asks go back to
waiting rather than being reported as said. One routing path for a tap, one greeting path, one
ledger, and the machinery that exists for two producers is not duplicated for a third that happens
to live in-process. The relay's refusal "a relay told to attach to a relay would dial its own door"
stays for attach as a *whole* — `KICKOFF_HUB_RELAY=1` in attach's own environment is still refused
— because that sentence is about who holds the claim, and the watcher does not.

What this costs, said plainly: the watcher's asks carry a producer number like anyone else's, and
the `already_claimed` rule that lived in the bridge moves into attach's hub link, where the relay
never had one — the relay waited on a squatter for ever, saying only "the hub would not take this
relay". The rule changed on the way: the bridge counted **three refusals in a row**, and with the
link's backoff that is a claim held for three *seconds* — shorter than the ten a predecessor attach
gets to stop, so an ordinary restart tripped it. A squatter is a claim held longer than anything
legitimate holds one, so attach calls a run stuck by **elapsed time** (30 seconds; with the backoff
that is the sixth refusal, at about 31), and at that moment answers every frame its producers had
queued with an `ack` saying no *before* it ends their sockets — the first version ended the sockets
first, and the "no" had nothing to travel on. The watcher keeps no counter of its own: every
refusal it sees is the door relaying the hub's, and the door decides.

### 13.3 How the engine's tool server finds the door

The tool server is the same `plugins/kickoff-channel/server.ts` under both engines, and it reaches
the door in the same way it always has: `KICKOFF_HUB_RELAY=1` plus a door it is either **told**
(`KICKOFF_HUB_RELAY_SOCKET`) or **derives** (§9's formula, from git). attach's job is to make sure
one of those is true, and it does it two ways at once.

**Under `--run`, attach pins the whole namespace in the child's environment.** The eight variables
of §2, every one set explicitly, so nothing is derived twice and nothing is inherited from above:

```
KICKOFF_HUB_PROJECT_DIR=<attach's project directory, absolute>
KICKOFF_HUB_ADDRESS=<the address attach holds, or - when it holds none>
KICKOFF_HUB_TOKEN_FILE=<the path attach was told, or the one it found — never - while it holds one>
KICKOFF_HUB_SOCKET=<the hub socket attach dials>
KICKOFF_HUB_RELAY=1
KICKOFF_HUB_RELAY_SOCKET=<the door>
KICKOFF_HUB_RELAY_DIR=-
KICKOFF_HUB_RELAY_GRACE_MS=-
```

The engine inherits that, and the tool server it spawns inherits the engine's. A Claude engine's
plugin, which has no `environment` block of its own, therefore finds the door with no further
configuration — and so does any adapter a stranger writes, including
`docs/examples/attach-from-the-document.ts`, which is why §13.7 runs it as a `--run` child, once
from a directory with no git and once from a lane worktree. The lane is why the token path is the
one attach *found*: a lane holds no secret of its own, attach finds the main tree's by the search,
and a child that does not search (§5 says being told is cheaper) would otherwise refuse "no secret
at `<lane>/.kickoff/hub.token`" while attach above it had just authenticated with that very file.

**On this box, under `--run`, the operator pastes nothing.** His file as it stands — an
`environment` block carrying three dropped names and `CLAUDE_PROJECT_DIR: ""` — works: opencode
gives its MCP child the server's whole environment with the block overlaid on top, the block
touches no `KICKOFF_HUB_` name, so the tool server inherits the pinned door and attaches. Measured
with the real engine, seven ways:

| the engine's config, and how it was started | what the tool server did |
| --- | --- |
| his file, hand-started beside a bare attach | refuses: "never said which project" — the shape attach retires |
| **his file, `--run`** | **attached, `said`, the hub logged the `say`** |
| his file, `--run`, in a directory with no git (a wall's shape) | attached, `said` |
| the §2 paste block, hand-started | attached, `said` |
| the §2 paste block, `--run`, address = git's name | attached, `said` |
| the §2 paste block, `--run`, **a minted address** | the tool server derives git's door, dials one nothing opens, `reply` says "not said yet … the relay is not running" for ever |
| the §2 paste block, `--run`, **no git** | the tool server refuses: "cannot work out where that relay is" |

So the eight-`-` block is the hand-started shape's answer to inheritance, and under attach it is
the wrong one: attach's pinning **is** the un-inherit that block exists for, with the right values
in it. When a dispatcher mints an address that is not git's name, or a wall has no git, a config
that overlays the pinned door with `-` makes the tool server look for a door nothing opens — the
split-brain nobody would diagnose in under an hour. attach cannot read the engine's config, so it
cannot refuse this; **`--check` prints the fact in words (§13.4), and attach prints the same
sentence as a warning when it starts** — the same function produces both, so they cannot drift.
The one thing this section retires is a variable the block never carried: `OPENCODE_URL`, which
becomes the value of `--opencode`.

**In a wall, the config is kickoff's, and its entry needs no `environment` block at all:**

```json
"kickoff-channel": {
  "type": "local",
  "command": ["bun", "/opt/herdr-tg/plugins/kickoff-channel/server.ts"],
  "enabled": true
}
```

There is nothing to overlay because there is nothing to un-inherit: attach is the outermost thing
in the wall, it pinned all eight, and the tool server takes them as they are. The `-` block exists
for a config that must serve every project and every lane on a host where an outer session may
have left variables behind; a wall has one project, one address and no outer session. Hand it to
the engine however kickoff prefers — the wall's own `~/.config/opencode/opencode.json`, or
`OPENCODE_CONFIG=<path>`, or `OPENCODE_CONFIG_CONTENT='{"mcp":{…}}'` on the environment; the
installed opencode reads all three, checked against its binary. **Do not mount the host's file
into the wall**: its `-` block would make the tool server derive a door from git, there is no git
to ask in a wall, and it would refuse to attach with a sentence about `KICKOFF_HUB_RELAY_SOCKET`
that the block itself prevents anyone from setting.

**Where the door is, in order:**

1. **Told.** `KICKOFF_HUB_RELAY_SOCKET`, verbatim, as today.
2. **Derived from git.** §9's formula, as today. This is the host case, and it is what makes the
   paste block meet attach with no variable set.
3. **Made, under `--run` only.** When neither of the above can say, and there is a child to hand it
   to, attach makes a folder of its own under the temporary directory — `mkdtemp`, mode `0700`,
   `kickoff-hub-attach-<six random characters>/` — and puts the door there: nothing else needs to
   dial it, so nothing else needs to know its name, and two walls cannot collide on it *because the
   name is random*. The first version named the folder by pid, and under `bwrap --unshare-pid`
   attach is PID 2 in every wall, so two walls sharing the host's `/tmp` derived one folder and the
   second died "another attach is already holding it". It is unlinked on exit; a wall killed
   outright leaves its folder behind, and no later wall reuses it. A temporary directory that is
   not absolute, or that does not exist, or that puts the path past the 108-byte ceiling, is a
   refusal naming `TMPDIR` — this box has already met the literal string `%h/.cache/tmp` there —
   and `--check` makes the same refusal without making the folder. Without `--run` there is no
   third option and attach refuses as the relay does today, naming `KICKOFF_HUB_RELAY_SOCKET`.

The private door of option 3 is never derived from the mount path: two walls with different
projects mounted at `/workspace` and the same address name would derive one door under the
mounted `fanin/` directory and the second would refuse "another attach holds it" — true, and the
wrong reason.

### 13.4 `--check`

**What it proves, and how it proves it without a topic.** The hub's admission is ordered, and the
order is what makes this clean (`hub.rs`, `admit` and `serve_connection`): peer credentials off the
socket, uid first — another uid is closed without a reply — then version, then secret to project,
then enabled, then the address shape; then the claim is taken and **`welcome` is sent**; then
`ping`; and **the topic is created only after the pong**, inside the `if live` branch. A connection
that ends before the pong is released and audited, and nothing reaches Telegram. So `--check`
connects, sends `hello`, treats the arrival of `welcome` as proof — socket reachable, uid admitted,
secret resolved to an enabled project, address well-formed and echoed, claim free — sends `bye`,
and closes **without ever ponging**. Confirmed in the code, not assumed.

Two costs, so that nobody is surprised by them. The check **holds the claim for the length of one
round trip**; the hub releases it the instant the connection ends, so a wrapper that runs the check
and then starts the worker is not refused. And it leaves one line in the hub's audit log —
*"connected but never answered; it is probably not allowed to talk to me"* — which is the hub's
honest reading of a deliberate check. Telling the hub the difference would be a new frame, which is
`crates/`, which is out of this slice; the line is noted here so that whoever reads that log knows
what a check looks like.

**What it prints.** One line per fact, in the order the facts are established, each beginning `ok`
or `NOT`; a `NOT` line carries the sentence that says what to do. The last line is the count. Exit
`0` when every line is `ok`, `1` otherwise. In order:

| fact | `ok` reads | `NOT` reads |
| --- | --- | --- |
| the configuration | *(no line of its own)* | the reader's own sentence, in the register `main.ts` dies with, e.g. `NOT  nothing named a project directory (KICKOFF_HUB_PROJECT_DIR), so there is no way to reach the operator from here` · `NOT  KICKOFF_HUB_ADDRESS is "CEO/steering", which cannot be addressed: it has a slash in it, and a conversation name cannot contain one` — the five shape rules of §4, before dialling · `NOT  KICKOFF_HUB_PROJECT_DIR is set to the empty string; a variable set to nothing is not a value` |
| attach as a producer | *(no line)* | `NOT  KICKOFF_HUB_RELAY is set on attach itself; it belongs on a producer that attaches to attach, not on attach` — the start refuses this, so the check does; it arrives by inheritance, since every `--run` child has it pinned |
| the project | `ok   speaking for /workspace (not inside a repository)` — or `(the main tree of a repository)`, `(a linked worktree of <main>)` | *(a refusal is the configuration row above)* |
| the address | `ok   the conversation: lane-0904-1200 (named by KICKOFF_HUB_ADDRESS)` — or `(git's name for this worktree)`, or `ok   the conversation: the project itself` | *(a refusal is the configuration row above)* |
| the secret | `ok   the secret: /run/secrets/hub.token (told by KICKOFF_HUB_TOKEN_FILE)` — or `(found above /home/<you>/proj)` | `NOT  no secret at /run/secrets/hub.token; mount the project's .kickoff/hub.token there, or run: herdr-tg enroll <dir>` |
| the secret, by value | *(no line)* | `NOT  KICKOFF_HUB_TOKEN is set, and the secret never travels as a value; put it in a file and name the file with KICKOFF_HUB_TOKEN_FILE` — and a `KICKOFF_HUB_TOKEN_FILE` that is 64 hex characters and no path reads `NOT  KICKOFF_HUB_TOKEN_FILE looks like the secret itself; it takes the path to the file` |
| the hub's socket | `ok   the hub's socket: /run/user/<uid>/kickoff/hub.sock is there` | `NOT  nothing at /run/user/<uid>/kickoff/hub.sock; the hub is not running, or the directory /run/user/<uid>/kickoff/ is not mounted here (mount the directory, never the socket file)` · a directory this uid cannot look into (it is `0700`): `NOT  this user may not look inside /run/user/<uid>/kickoff/ (it is the hub's, mode 0700); run as the same user as the hub` — the first thing a foreign uid hits, before any connect |
| reached | `ok   reached the hub` | `EACCES`: `NOT  the hub's socket refused this user; run as the same user as the hub` · `ECONNREFUSED`: `NOT  a socket file is there but nothing is listening behind it; the hub is not running` |
| admitted | `ok   admitted as "oc-dogfood · lane-0904-1200"` — the hub's own title for the conversation, once | closed after `hello` with no frame: `NOT  the hub took the hello and closed without a word; either this process is not running as the hub's user, or the hello was malformed, and from outside the two cannot be told apart` — and *only* then: the close that follows a `refused` is not reported, so a refusal is exactly one line · accepted and mute: `NOT  the hub accepted the connection and said nothing for 6 seconds; it is running but wedged — restart herdr-tg` · `unknown_project`: `NOT  the hub does not know this project; run: herdr-tg enroll <dir>` · `bad_token`: `NOT  the secret is not one the hub knows; re-run: herdr-tg enroll <dir>` · `not_enabled`: `NOT  this project is enrolled but switched off` · `version_skew`: `NOT  this command and the hub do not speak the same version; upgrade one of them` · `bad_lane`: `NOT  the hub will not address a conversation called <x>; if the hub is older than this command, restart herdr-tg` · `already_claimed`: `NOT  another connection holds this conversation right now; if it is your own worker, run the check before it and not beside it; if nothing of yours is running, a stray process is squatting the claim` · echo missing: `NOT  the hub did not give <x> a place of its own; it is older than this command` · anything else: `NOT  the hub refused for a reason this command does not know (<reason>)` |
| the door | `ok   the door: <path> (worked out from git), free` — or `(named by KICKOFF_HUB_RELAY_SOCKET), free`, or under `--run` with no git `ok   the door: will be made in a private folder under <TMPDIR> when the worker starts, and handed to its engine` | `NOT  another attach already holds the door at <path>; this conversation has a worker already` · no git, no `--run`: `NOT  this folder is not inside a repository and nothing named a door; set KICKOFF_HUB_RELAY_SOCKET` · under `--run` with no git: `NOT  TMPDIR is "%h/.cache/tmp", which is not an absolute path, so a private door cannot be made under it; set TMPDIR to a real directory` (or "does not exist", or "past the 108-byte socket limit") — the start's own refusal, made here without making the folder |
| the tool server | `ok   a tool server that works out its door from git here finds this one` — or, with no git, `ok   a tool server here must be told the door, and a worker started with --run tells it` / `ok   a tool server here cannot work out a door from git, so it must be given the same KICKOFF_HUB_RELAY_SOCKET=<door>` — or, when attach's door is not git's: under `--run`, `ok   the tool server the engine spawns is told this door by --run; a config that overlays KICKOFF_HUB_RELAY_SOCKET with - would look for <other> instead and never find this one`; told without `--run`, `ok   the door was named by KICKOFF_HUB_RELAY_SOCKET, so a tool server that works one out from git would look for <other>; give the engine the same KICKOFF_HUB_RELAY_SOCKET=<door>` | derived from a minted address, without `--run`: `NOT  a tool server that works out its door from git here would look for <other>; either use the worktree's own name as the address, or give the engine a config that names KICKOFF_HUB_RELAY_SOCKET=<door>`. Whichever line prints, attach prints the same sentence as a warning when it starts, from the same function. |
| the engine's address, with `--opencode` | `ok   the engine's address: http://127.0.0.1:9711` | `NOT  --opencode http://127.0.0.1: names no port; the watcher would dial the wrong server. Give it the port opencode serve was given, e.g. --opencode http://127.0.0.1:9711` — the start refuses the same URL; the unit's `${OPENCODE_PORT}` unset is how it arrives |
| the engine, with `--run` | `ok   the engine: opencode, found at <path>` | `NOT  the engine: opencode is not on PATH` |
| PID 1, with `--run` | `ok   not PID 1` | `NOT  this process is PID 1 and nothing reaps for it; put the wall's own init in front (docker run --init; bwrap without --as-pid-1, which reaps and forwards nothing — stop a bwrap wall by signalling attach itself)` |
| the count | `everything a worker here needs is in place` | `<n> thing(s) to fix before a worker here can reach him` |

The uid line is deliberately **not** a comparison of numbers. Under `bwrap --unshare-user` this
process sees uid 0, the mounted socket's owner reads as the overflow uid, and the connection
succeeds once the socket is where the tenant derives it — `KICKOFF_HUB_SOCKET`, or the directory
bound at `/run/user/0/kickoff/` — because the hub reads the credential in its own namespace. A
numeric check would say `NOT` to a wall that works; the connect and the `welcome` say what is true.

`--check` is what a wrapper runs before trusting a wall, and it is also the first thing a person
runs when a worker is silent: every failure this document has spent ten sections describing —
the silent close, the stale inode, the missing mount, the empty variable, the door nothing listens
at — prints as one line that names the fix.

### 13.5 `--run`

**The order of operations.** Read the namespace, and refuse with the variable's name if it is
wrong. Refuse if this process is PID 1 and there is a child to start. Open the door — dialling any
socket file already there, so that a live attach for this address is refused with exit 2 and a
leftover file is unlinked, exactly as the relay does today. Start the hub link. **Spawn the child.**
Start the watcher, if `--opencode`. The door is bound before the child exists, so the first tool
server the engine spawns finds it; the hub link need not be up — a child whose hub is down is told
"not said yet" by its tool server, which is the honest sentence, and the link keeps dialling.

**The child** runs in the project directory, inherits attach's stdin, stdout and stderr, and gets
attach's environment plus the eight pinned variables of §13.3. attach never reads stdin itself: it
is not an MCP server and nothing on that stream is for it.

**Signals.** attach installs handlers for `SIGTERM` and `SIGINT` — both, because a process with no
handler for them at PID 1 ignores them, measured on this box with bun, and a wall that cannot be
stopped is a wall that gets killed with its questions still open. On either, it forwards the same
signal to the child, waits up to ten seconds, then sends `SIGKILL`, and proceeds as if the child
had exited on its own. It does **not** reap orphans: a grandchild the engine leaves behind reparents
to PID 1, and if PID 1 is a JavaScript runtime it stays a zombie for ever — also measured. Waiting
on a process this runtime did not start means reaching into libc from the process that holds the
hub's claim, for one job an init does in a kilobyte of C, so this design declines it: `--run`
refuses at PID 1 and names the fix, and `--check` says the same thing before it gets that far.

**Behind `docker --init`** attach is PID 2, tini forwards the signal `docker stop` sends, and every
signal and every exit behaves as it does on a desk; docker ends the container when PID 1 exits.

**Behind bwrap's init it does not**, measured on this box (bubblewrap 0.12) and pinned by
`test-run.ts` so it cannot drift again. bwrap's init reaps zombies and does nothing else: it
forwards no signal, and it ignores `SIGTERM` itself. So —

* a `SIGTERM` to the bwrap process **ends bwrap (143) and reaches nothing inside**: attach and the
  engine keep running, the door stays bound, the claim stays held — the corpse squatting the claim
  that this section opens with, and the successor's `already_claimed` run follows;
* **stop a bwrap wall by signalling attach's own host pid** — the *child* of the pid bwrap reports
  on `--info-fd` (that pid is the init). attach then forwards, waits, says `bye`, and the wall is
  empty; bwrap returns the child's status;
* pass **`--die-with-parent`**, so a wall whose wrapper dies is killed with it rather than left
  squatting. That is `SIGKILL` to the wall — no `bye` — and the hub releases the claim when the
  socket closes, which is the right outcome for a wrapper that is gone;
* do **not** read bwrap's return as "the wall is empty": when attach exits, bwrap returns at once
  while the engine's orphans stay inside with bwrap's init until they exit on their own.

Said plainly, the decision: the wrapper owns the host-pid lookup. The alternative — `--as-pid-1`,
with attach at PID 1 signalable by its host pid and orphans dying with it — costs zombies for the
life of the wall, since attach does not reap, and an agent's shell spawns constantly; attach keeps
refusing `--run` at PID 1. The wrapper is kickoff's (§13.9); these four bullets are what it copies.

**When the child exits**, for any reason: the ledger is written down first (the questions open at
this instant are what the next run has to route taps for); `bye` goes on the hub link if it is up,
and attach waits for the kernel to take it — the same 200 ms both processes use today, because
`process.exit()` on the same tick loses the frame to a short write; the door is unlinked, and a
private door's folder with it; and attach exits **with the child's status** — its exit code, or
`128 + n` for a signal. A child that could not be started at all is `127` and one line saying so.
attach's own exit takes the door with it, so the next start of the same address finds either
nothing or a leftover file, never a live socket with nobody behind it.

**Where the spawn is, and why it may be there.** In `run.ts`, one call site, and the comment on it
says this: *the ADAPTER may spawn; the HUB never does.* The hub's closed list of capabilities
(`docs/INTERFACES.md`) puts "spawning, supervising, or killing anything" under things the hub is
not, and its argument is that no string from the wire may ever reach a command line. Nothing here
contradicts that: the command attach runs comes from **its own argv**, typed by a person or written
by a wrapper, and no frame from the hub can add to it, change it or start it — a `choice` reaches
an opencode reply endpoint or a tool server's turn, never `run.ts`. The hub still has zero
`Command` in its binary, and the test that pins the deletion of the keystroke path does not know
`adapters/` exists. This is seam ④'s own proposal, arrived at from the other side: the thing that
starts an engine is an adapter, holding a hub connection like any other, and the hub cannot tell
it from one that does not.

### 13.6 The unit

`deploy/kickoff-hub-attach@.service`, a `systemd --user` **template**, so that a worker on a box is
supervised the way `herdr-tg.service` already is: restarted on a crash, started at login, its
output in a journal. It reads the same namespace and nothing else; the only thing it adds is the one
number that is the engine's rather than the hub's.

```ini
[Unit]
Description=kickoff-hub-attach — the worker "%i", wired to the hub
Documentation=https://github.com/vinceferro/herdr-tg/blob/main/docs/ATTACHING.md
After=default.target
# Never give up. The same reasoning as herdr-tg.service: the operator is on a phone, and a
# start-limit burst would leave a worker dead until he reaches a keyboard.
StartLimitIntervalSec=0

[Service]
Type=simple
# The whole of one worker's configuration: docs/ATTACHING.md §2, KEY=VALUE, one file per instance.
# `-` so that a missing file is attach's own sentence ("nothing named a project directory") on the
# journal rather than systemd's opaque refusal to start.
EnvironmentFile=-%h/.config/kickoff-hub-attach/%i.env
# OPENCODE_PORT is the one line in that file that is not §2's: the port belongs to the engine, not
# the hub. It appears twice on the line so the server and the watcher can never disagree about it.
# `opencode serve` without --port picks a random port, so it is not optional here — and it is
# checked before the start, because an unset ${OPENCODE_PORT} expands to NOTHING: the watcher would
# then dial port 80 while `opencode serve --port ''` listens on 4096, and the worker would hold the
# claim, get its topic, and deliver nothing. attach refuses the port-less URL too; this line is the
# one that names the variable and the file. (`$$` is a literal `$` for the shell.)
ExecStartPre=/bin/sh -c 'test -n "$$OPENCODE_PORT" || { echo "OPENCODE_PORT is not set; put it in %h/.config/kickoff-hub-attach/%i.env" >&2; exit 2; }'
ExecStart=%h/.local/bin/kickoff-hub-attach --opencode http://127.0.0.1:${OPENCODE_PORT} --run opencode serve --port ${OPENCODE_PORT}
# SIGTERM goes to attach ONLY; it forwards to the engine, waits, and says bye. SIGKILL to whatever
# is left after TimeoutStopSec. The default, control-group, would hit the engine and attach at the
# same instant and the goodbye would never be said.
KillMode=mixed
TimeoutStopSec=20
Restart=always
RestartSec=5
# No hardening block, on purpose. This unit runs an agent's shell, and every line of
# herdr-tg.service's hardening is a line that agent would trip. The wall is the safety, and the
# wall is kickoff's; this unit is for a worker on the operator's own desk.

[Install]
WantedBy=default.target
```

**How an instance is named.** The instance is a short label the operator picks for one worker —
`oc-dogfood`, `herdr-tg-main` — and the label names one file, `~/.config/kickoff-hub-attach/<label>.env`,
that holds that worker's namespace. The label is not the address and not the directory; those are
in the file, which is why two workers for two worktrees of one project are two labels and two
files. A file, written out in full because an environment file is not a shell and expands nothing:

```
KICKOFF_HUB_PROJECT_DIR=/home/<you>/scratch/oc-dogfood
OPENCODE_PORT=9711
```

Add `KICKOFF_HUB_ADDRESS=` when a dispatcher minted one; leave it out to take git's name for the
worktree. Every other §2 variable may appear and means what §2 says. On this box the user manager's
`PATH` already carries mise's shims, checked, so `bun` and `opencode` resolve; on a box where it
does not, `PATH=` goes in the same file — a `--user` manager does not inherit a login shell's
environment, which the watchdog unit already had to learn.

```
scripts/install-attach.sh                       # writes ~/.local/bin/kickoff-hub-attach and installs the unit; starts nothing
systemctl --user enable --now kickoff-hub-attach@oc-dogfood
journalctl --user -u kickoff-hub-attach@oc-dogfood -f
```

The unit is the **opencode** worker. A Claude worker on a desk is the operator's own interactive
session with the channel plugin, which is not a service and is not this unit's business; a Claude
worker in a wall is `--run claude …` under the entrypoint of §13.1, which the unit does not need
either.

### 13.7 The tests

**Survive unchanged, not a line.** `plugins/kickoff-channel/test-against-a-fake-hub.ts` and
`cargo test -p herdr-tg the_real_plugin -- --ignored` — the Claude path, which this section does
not touch, and the proof that the two shared files it does touch (below) changed nothing that path
uses. Everything under `crates/`. And `docs/examples/attach-from-the-document.ts`, the stranger,
which is run twice: once as a producer at attach's door, where it is heard beside the tool server
and the watcher with one claim at the hub, and once as the "engine" attach starts with `--run` in a
directory with no git, where it inherits the private door and is heard. **If that file has to change,
the interface changed, and that needs saying loudly** — it is the alarm this section is designed
against.

**Move, subject renamed, checks kept.** Every behavioural check in `adapters/fanin/` survives,
because attach *is* the relay:

* `test-two-producers.ts`, 37 checks: Part 1 (two producers dialling the hub race), Part 2 (one
  claim, both reached, verbatim text, distinct ids for one minted `a1`, a tap to the asker only, an
  ack naming the producer's frame, typed words to every producer, a ping answered while all are
  silent, one goodbye not taking the lane down, the queue share under a flood), Part 3 (wrong lane,
  wrong secret, no hello, a second attach refused with exit 2) and Part 4 (a producer whose door is
  gone says "waiting" and names the door). One rename:
  `a_second_relay_for_one_conversation_refuses_to_start_rather_than_racing_the_first` becomes
  `a_second_attach_for_one_conversation_refuses_to_start_rather_than_racing_the_first`.
* `test-what-breaks-it.ts`, 17 checks: A (the hub goes away under attach), B (a producer dies with
  a question open), C (260 open questions), D (three producers, no hub), E (attach restarts and comes
  back under the same instance).
* `test-against-fakes.ts`, 23 checks, now spawning `main.ts --opencode <fake url>` against the fake
  hub: the handshake (secret in `hello`, no display name, nothing before `welcome`), an unknown
  refusal waited out with growing backoff, the `already_claimed` counter counting a run, and every
  mapping check — question to ask with published labels, tap to the v2 reply endpoint carrying the
  label, no second retirement on a tap, a re-tap posting nothing, permission to three buttons, a
  bogus option posting nothing, reject to the permission endpoint, a keyboard answered elsewhere
  retired reading both `properties` and `data`. Every one of them reads the ask id off the wire and
  never predicts it, so the producer number attach now prefixes changes nothing they assert.
  `every_adapter_speaks_the_same_wire_from_the_same_file` keeps its name and changes its list to
  `plugins/kickoff-channel/server.ts`, `adapters/kickoff-hub-attach/relay.ts`,
  `adapters/kickoff-hub-attach/opencode.ts` and `adapters/kickoff-hub-attach/check.ts`, and gains
  one rule: no other `.ts` under `adapters/` or `plugins/` that is not a test or the harness may
  carry `t: 'hello'`.

**Retired, because the subject is gone.** One check:
`the_real_event_bridge_attaches_to_the_relay_without_a_line_of_its_own_changing` proved that a bridge
*process* joins a relay *process* unmodified, and there are no longer two processes. The property
it protected — the event voice and the chosen voice share one claim and their ids stay apart — is
covered in the same part of the same suite by
`the_event_voice_and_the_chosen_voice_share_one_claim_inside_one_process`, which starts attach with
`--opencode` against a fake server, has the tool server say one thing and the server raise one
question, and checks the fake hub saw one `hello`, one `say`, one `ask`, and two ids.

**New.** Two suites, and each RED is watched failing for its reason before it goes green:

* `test-check.ts` — the facts of §13.4, one at a time, against fakes: every line `ok` and exit 0
  when all is well, and the fake hub saw exactly `hello` then `bye` — **no `pong`, nothing else** —
  which is the proof that no topic could have been made; no socket; a socket file with mode `0` so
  `connect()` fails `EACCES` and the line says "run as the same user as the hub"; a hub that reads
  the `hello` and closes; each refusal reason and its sentence; a bad address refused before the
  hub saw anything; `KICKOFF_HUB_TOKEN` set, refused before the hub saw anything; a token file that
  is the secret itself; a live attach on the door; a held claim.
* `test-run.ts` — the lifecycle of §13.5: `--run sh -c 'exit 7'` exits 7 after the hub saw `bye`;
  `--run sleep 60` sent `SIGTERM` exits 143 with the child gone; a child that traps `SIGTERM` is
  killed after the wait and attach exits 137; `--run env` shows the eight pinned variables and the
  door; the child's cwd is the project directory; the stranger as the engine, with no git and no
  door named, is welcomed through a private door and heard, with one claim at the hub; a command
  that does not exist exits 127; the stranger again from a *lane* worktree, where the pinned token
  path is the one attach found; and, when `bwrap` is on the box, attach under
  `bwrap --unshare-pid --as-pid-1` refuses `--run` with the PID 1 sentence, under the default
  reaper runs, two walls sharing one TMPDIR each get a private door, a `SIGTERM` to bwrap itself
  is measured to reach nothing inside while attach's own host pid stops the wall with a `bye`, and
  `--die-with-parent` kills the wall and releases the claim without one. When `opencode` is on the
  box, the real engine runs under `--run` with the operator's own `environment` block copied
  read-only from his file (or none, where there is no file), a session is opened, and the tool
  server the engine spawns is seen attaching at attach's door with the pinned door in its
  environment (read from `/proc`), and a tool server given exactly that environment is heard at
  the hub. Each is skipped, and says so, where its binary is absent.
* `test-what-breaks-it.ts` gains one: a hub that answers every `hello` with `already_claimed` is
  given more than thirty seconds before the door calls it stuck, and a producer that queued a frame
  meanwhile hears `ack no` for it before its socket ends.
* `test-check.ts` also holds the three refusals the start makes that the first check did not (a
  relay flag on attach, a TMPDIR that is not a path, an `--opencode` URL with no port), every
  refusal reason beside a live door with exactly one line each, a `0700` directory, the reader's
  register, a mute hub, a told door, and the hub's composed title printed once.

### 13.8 What the build must touch outside the new directory

Listed so that nobody discovers it in a diff.

* **`plugins/kickoff-channel/attach.ts`** — the one reader, shared with the Claude path — gains
  one refusal: `KICKOFF_HUB_TOKEN` set to anything, and a `KICKOFF_HUB_TOKEN_FILE` that is the
  secret rather than a path, each with the sentence in §13.4. No correctly configured session sets
  either, so `server.ts` does not change behaviour and its two suites are the proof. §2 gains the
  row, marked *never*.
* **`plugins/kickoff-channel/hub-link.ts`** — the one wire, shared with the Claude path — gains two
  optional things, neither of which `server.ts` uses: a `once` mode that dials one time and never
  redials, and a callback that reports how a dial ended — the error's code (`ENOENT`, `EACCES`,
  `ECONNREFUSED`) or "closed before `welcome`" — which the module today discards at line 454, the
  defect `docs/TAXONOMY.md` §8 already named. The sentences the agent reads (`whenUnreachable`,
  `whenDropped`) do not change.
* **`docs/ATTACHING.md`** — §1, §9 and §11 name `adapters/fanin/`; §2 names `OPENCODE_URL` and
  says "all three adapters"; §10's worked container gets attach as its entrypoint (the shape in
  §13.1) and loses the sentence that `KICKOFF_HUB_RELAY_SOCKET` is not optional in a container,
  which stops being true under `--run`; §12 stops listing supervision as a separate slice.
* **`docs/CAPABILITIES.md`** REQUIRES 4, **`docs/INTERFACES.md`** and **`CLAUDE.md`**'s layout name
  the two directories that go.
* **`deploy/kickoff-hub-attach@.service`** and **`scripts/install-attach.sh`**, new. The installer
  writes the two-line shim (`exec bun <repo>/adapters/kickoff-hub-attach/main.ts "$@"`), copies the
  unit to `~/.config/systemd/user/`, runs `daemon-reload`, and **starts nothing** — the same
  discipline as `install-channel-plugin.sh`, which proves the plugin before installing it and
  never opens the door itself.

### 13.9 Not in this section, said plainly

* **The wrapper.** bwrap and docker lines are kickoff's; §13.1's second invocation is the shape it
  copies, not a script this repo ships.
* **The launcher** that starts a wall from a tap — seam ④ — is kickoff's adapter. attach is what it
  starts.
* ~~**Typed steering into opencode.**~~ Built, 5 September: the watcher carries a `message` to the
  session as a prompt (`opencode.ts`, `carry`), answers the hub with `ack{status, reason?}`, and
  the door refuses out loud when nothing is attached to take the words. §7, offer 6, has the rule.
  Every request of the server has a ten-second deadline, so one it takes and never answers cannot
  park every line typed after it; words opencode took and the agent then could not act on
  (`session.error`) are said so in the topic, once; a wall started with `--run opencode …` and no
  `--opencode` is half a phone — no questions, no prompts, typed words refused out loud — and the
  start and `--check` both say so.
* **`session.idle` as `beat`.** Still acked and dropped by the hub, as `docs/TAXONOMY.md` §7 records.
* **Reaping at PID 1.** Declined, §13.5, and the wall's init does it.
* **Telling the hub a check from a real connection.** A new frame, `crates/`, another slice.

## Changing this file

Bump the version in the header comment and say what moved. A variable may be added at any time. A
variable may not change meaning without a new name, because the whole point of one namespace is that
a name means one thing.
