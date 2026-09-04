<!-- INTERFACE, v3, 4 September 2026. The one abstract surface an adapter attaches to: one
     configuration namespace, one wire, one document.

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
the real relay by `adapters/fanin/test-two-producers.ts`. It gets a forum topic of its own, delivery
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
* **`OPENCODE_URL`** is seam ②, the engine's own endpoint. The attach interface does not own engine
  endpoints; an adapter for a new engine names its own variable and documents it beside its own
  code. Do not put an engine endpoint in `KICKOFF_HUB_`.

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
| `OPENCODE_URL` | | unchanged | kept, outside the namespace, seam ② |

Eleven variables across five prefixes become eight in one, of which a normal adopter sets one.

**On our side this is done.** All three adapters read `plugins/kickoff-channel/attach.ts`, and it is
the only file any of them reads a variable in; `where.ts` beside it answers only what the MACHINE
says once a directory has been named, and `hub-link.ts` is the one wire (§8). The one other file in
the repo that reads these variables is `docs/examples/attach-from-the-document.ts`, and it does so
deliberately — it is a stranger's adapter, written from this document, importing nothing of ours.
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

**Anything you say between step 3 and step 7 is kept and replayed**, bounded at 64 frames or 256 KiB.
Over that bound you are dropped without becoming live. An adapter that opens with a question is the
whole point of the product, so the buffer exists; it is not a licence to stream into it.

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
| `ack` | `ref`, `status` (`accepted` \| `refused`), `reason?` | — |
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

*Gap:* there is no ack timeout on the wire. If the connection dies with frames in flight, no ack is
ever coming for them — **you must release your in-flight map on close and tell whoever was waiting**,
because nothing else will.

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
*Gap:* the wire has no way to say "this conversation cannot accept typed words". The operator can
type into a topic whose adapter silently drops it — which is exactly what the opencode event bridge
does today. If your adapter cannot inject text into its engine, say so out loud in the conversation
rather than dropping in silence.

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
and all three adapters share it. A test fails if any of them starts writing its own again.

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
    they were on their way.
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

`adapters/fanin/` is our relay. One process per addressable thing holds the claim; producers attach
to it over a local socket that **speaks hub-proto unchanged**, so a producer needs no second wire
contract and nothing in it changes but the address it dials.

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

One adapter, one conversation, in a container, talking to a hub on the host.

**Mount two things.**

| mount | as | why |
| --- | --- | --- |
| the host's `/run/user/<uid>/kickoff/` **directory** | the same path inside | Never the socket **file**: the hub unlinks and rebinds it on every start, and a bind-mounted file becomes a stale inode the moment the hub restarts. The directory is `0700`. |
| the token file, read-only | anywhere, e.g. `/run/secrets/hub.token`, mode `0600` | The credential is a file the filesystem protects, not a variable the environment leaks. |

**Set three things — four if you are behind a relay.**

```
KICKOFF_HUB_PROJECT_DIR=/workspace          # where the repo is mounted; for the audit record
KICKOFF_HUB_TOKEN_FILE=/run/secrets/hub.token
KICKOFF_HUB_ADDRESS=<the name your dispatcher minted>
KICKOFF_HUB_RELAY=1                         # only if a relay holds the claim (§9)
KICKOFF_HUB_RELAY_SOCKET=/run/user/<uid>/kickoff/fanin/<the relay's socket>
```

**`KICKOFF_HUB_RELAY_SOCKET` is not optional in a container.** §9's derived path is computed from a
git fact, and a container that has the repo mounted but no git binary — the ordinary case — cannot
compute it. Get the path from whoever started the relay; it is on the relay's own first line of
output.

**Run as the same uid as the hub.** This is not optional: the hub reads the peer credentials and
closes a connection from another uid **with no reply at all** — you will see a socket that accepts and
then does nothing, which is the least debuggable failure in this system.

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
| `KICKOFF_HUB_RELAY_SOCKET`, behind a relay | with no git in the container the derived path cannot be computed, so the adapter refuses to start and names the variable. |
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
   and importing nothing of ours; `adapters/fanin/test-two-producers.ts` runs it against the real
   relay, and that run is what found the ordering defect now fixed in §1. It is an example, not a
   suite: it covers the good-day path and none of §8. Our own suites are
   `bun test-against-a-fake-hub.ts`, `bun test-two-producers.ts`, `bun test-against-fakes.ts` and
   `cargo test -p herdr-tg the_real_plugin -- --ignored` — the last is the real bridge against the
   real hub with only Telegram faked. A third-party adapter has no equivalent, and a fake written
   from your own reading of this document proves only that you agree with yourself.
4. **The relay's local protocol is hub-proto, but its lifecycle rules are not in `hub-proto`.** §9
   lists the four differences; they live in `adapters/fanin/`, and if you write your own relay you are
   re-deriving them from prose.
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
* **Supervision and systemd units.** Real, and a separate slice.
* **The pre-pong 256 KiB bound.** Real, hub-side, and a separate slice.

## Changing this file

Bump the version in the header comment and say what moved. A variable may be added at any time. A
variable may not change meaning without a new name, because the whole point of one namespace is that
a name means one thing.
