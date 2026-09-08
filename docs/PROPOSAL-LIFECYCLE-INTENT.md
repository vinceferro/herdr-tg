<!-- PROPOSAL, 8 September 2026. Nothing was built to write this and no file outside this one was
     changed. It answers the six questions in codex-steering's bounded design checkpoint of
     6 September (`20260906T193613Z`), plus the operator's architectural addendum, which asks for a
     typed lifecycle-intent contract that was planned and never built.

     CITATIONS ARE PINNED TO HEAD `ba9454b`, ALL OF THEM. Every claim about existing behaviour
     carries a file:line that was read there, not assumed, and all 104 of them were re-checked
     mechanically against that commit — in range, and each quoted phrase inside the range it names.
     Pinned rather than cited at the working tree because, while this was being written, the
     fleet-identity slice began landing across twenty-one files in the tree (`welcome.project_id`,
     `welcome.conversation`, optional `hello.repo`/`hello.pid`), moving line numbers under a reader
     mid-document. Where that in-flight work bears on this proposal it is named as in flight and
     never cited as shipped. A reader on a later commit should expect the numbers to have moved and
     the sentences not to have.

     Where PLAN-v2 §Slice F2 and the code disagree, the code wins, and §8 says where.

     No home path, chat id, user id, session id or secret appears here. -->

# The lifecycle-intent contract

**A controller declares in advance what it can be asked to do. The operator picks one of those
things off a keyboard. The hub carries the pick, fenced, idempotent, and audited — and carries
nothing that could name an image, a command, a mount, a secret, an environment variable or a host
path, because it has no field for one and will not grow one.**

That is the whole proposal. Everything below is either the answer to one of the checkpoint's six
questions, the shape a stranger would implement from, or a line this hub will not cross.

**Where the checkpoint's six questions are answered.** They are not §1–§6 in order, because its
sixth asks for tests rather than a design and those belong after the design they attack:

| the checkpoint's question | answered in |
|---|---|
| 1 · the binding object | §1 |
| 2 · bidirectional confinement | §2 |
| 3 · the engine capability handshake | §3 |
| 4 · the decision / command envelope | §4, with the wire in §6 and the discipline fields in §7 |
| 5 · the claim lifecycle | §5 |
| 6 · adversarial contract tests | §10 |

---

## 0. What is being answered, and what is deliberately not

The checkpoint (`20260906T193613Z`) is explicit about scope: *"Stay inside `herdr-tg`; do not
implement Kickoff's controller, runners, Caddy/frontdoor, memory partitioning, worktrees, scheduler,
or Mission Control."* This document holds to that. It designs one wire widening and the hub-side
rules around it. It designs no controller, starts no process, and names no unit.

The operator's addendum adds the second half: a typed lifecycle-intent contract suitable for a
future Kickoff controller — **start** a predeclared AgentSpec, **scale** an allowed domain,
**drain**, **stop**, **restart**, **inspect** — every request carrying an authenticated operator
identity, a project/domain scope, an idempotency key, an expected generation where relevant, an
explicit accepted/refused/completed outcome, and an audit record.

`docs/CAPABILITIES.md:335-339` already lists this as **OPEN 1**, in these words:

> **Who starts a container.** The hub has no `Command` in the binary today, and seam ④ of
> `docs/INTERFACES.md` proposes a launcher that is simply another adapter: it holds a hub
> connection, offers what it can start as an `ask`, and acts on the `choice` itself. Whether the
> actor should instead be the hub binary is the operator's call and is being brainstormed across
> both orgs. Listed here rather than under REFUSES because it is genuinely open.

This proposal **does not** reopen that. It takes the settled half — the actor is a controller, never
the hub — and asks the narrower question OPEN 1 leaves behind: *given that a controller acts, what
does the hub carry so that what it carries can be trusted?* Today the answer is `ask` and `choice`,
which work and are typed by nothing. This proposes the typed form.

---

## 1. The binding object

> *"Accept a versioned, generation-fenced binding containing stable project/domain IDs, `run_id`,
> generation, exact engine session, expected agent, canonical directory/project identity, and
> expiry/lease metadata. Repository paths, ports, Telegram topic IDs, and engine session IDs are
> addresses — not stable workload identity."*

### What already holds today

**The hub is not, and must not become, a party to the binding.** `docs/CAPABILITIES.md:324-331`
(REFUSES 6) is a settled line, not an omission:

> **A session of an engine is the same kind of word.** Which session, turn or process on the far
> side takes the operator's words is the adapter's to decide and to enforce (OPEN 5); no frame
> carries a session, a directory or an engine's name, and `hub-proto` has no field for one. A hub
> that learned which session a conversation meant would have to be told when it was replaced, and it
> is the wrong party to tell.

So the checkpoint's binding object splits cleanly in two, and only one half is ours:

* **The engine-facing binding** — session, directory, agent, generation, floor — is BUILT and lives
  entirely outside the wire. `docs/ATTACHING.md:2447` (§13.10) is its authority; the reader is
  `adapters/kickoff-hub-attach/plan.ts:297` (`readBindingFile`). Its key set is **closed** —
  `adapters/kickoff-hub-attach/plan.ts:242` (`THE_KEYS_IT_HAS`) — over exactly
  `version`, `conversation`, `canonical_project_dir`, `session_id`, `agent`, `generation`,
  `verified_at`, bounded at 4096 bytes (`plan.ts:265`, `MOST_A_BINDING_CAN_BE`). An unknown key is
  refused rather than ignored, because an unknown key may be a *narrowing* and obeying the rest
  while dropping it delivers his words on a rule nobody checked (`docs/HARDENING-HANDOFF.md:383`).
  `docs/CAPABILITIES.md:366-395` (OPEN 5) says in terms that this file **is not the contract between
  the two orgs** and that neither side should build on it; the durable form is the typed binding
  itself.
* **The hub-facing identity** — which conversation, which run of it — is BUILT and already carries
  everything the checkpoint asks a *hub* to know:

  | the checkpoint's word | what the hub holds now | where |
  |---|---|---|
  | stable project id | `ProjectId`, minted once at enrolment, opaque, never a counter | `crates/hub-proto/src/ids.rs:51-57` |
  | stable domain id | `LaneId`, an ADDRESS and never a credential | `crates/hub-proto/src/ids.rs:80-91` |
  | the pair, as one thing | `Addr { project, lane: Option<LaneId> }` | `crates/herdr-tg/src/hub.rs:2005-2008` |
  | `run_id` | `Claim.instance`, the run of the worker | `crates/herdr-tg/src/hub.rs:1575` |
  | generation | `Envelope.generation`, minted on the claim, stamped on every frame both ways | `crates/hub-proto/src/frame.rs:84` (HEAD), `crates/herdr-tg/src/hub.rs:1571-1574` |
  | lease metadata | the generation IS the lease; there is no second field | `crates/hub-proto/src/frame.rs:47-76` (HEAD) |

  And the construction that makes it safe is one sentence at `crates/herdr-tg/src/hub.rs:1995-2000`:

  > Built ONLY from the project a SECRET resolved to plus the lane the bridge named. That
  > construction is the entire security argument: the project half never comes from the wire, so a
  > bridge naming a lane can only ever reach a lane of the project it has already proved it is. A
  > lane is an address, never a credential.

**The checkpoint's own distinction is already the code's.** *"Repository paths, ports, Telegram topic
IDs, and engine session IDs are addresses — not stable workload identity."* The hub agrees on every
one: `hello.repo` is "for the audit record and for a human reading it. Never for routing"
(`crates/hub-proto/src/frame.rs:422`, HEAD); `welcome.topic_id` is documented "A bridge does not need
it and **must not use it to address anything**" (`crates/hub-proto/src/frame.rs:571-572`, HEAD); and
there is no port and no session field anywhere in the crate.

### What would have to be added

**Nothing, for the binding itself.** The one thing the lifecycle contract needs that the hub does not
have is a *third* stable id: the **spec id**, naming which predeclared AgentSpec an intent is about.
That is minted by the controller and opaque here — the same treatment `AskId` and `OptionId` already
get, for the reason `crates/hub-proto/src/ids.rs:8-9` gives:

> There is deliberately no parsing and no validation. A bridge mints these; the hub stores and
> echoes them. Anything the hub tried to read out of an id would be a fact it invented.

So: `SpecId` and `IntentId`, two more `opaque_id!` newtypes in `crates/hub-proto/src/ids.rs`. No
parsing, no validation, no meaning here. **This is the whole of the binding-object change on the hub
side**, and it is the reason an image name can never travel: a `SpecId` is a handle the controller
dereferences against its own approved table, and the hub cannot dereference it because it has no
table and no way to get one.

**One thing this proposal will NOT do, and it is worth naming as a refusal rather than a gap.** The
checkpoint lists *"exact engine session"* and *"expected agent"* among the binding's fields. Those
must not reach `hello`, `welcome`, or any intent frame. `docs/CAPABILITIES.md:328-329` forbids it,
and the reason is operational rather than stylistic: a hub that learned which session a conversation
meant would have to be told when it was replaced, and nothing in this system is positioned to tell
it. The controller enforces the session; the hub carries the intent. See §5.

---

## 2. Bidirectional confinement

> *"Typed messages, questions, permission prompts, replies, interrupts, and retirement must all stay
> inside the exact bound session/generation. Missing or stale binding refuses explicitly; there is no
> only-session/most-recent fallback."*

### What already holds today

The hub's half is **built and fenced in both directions**, and there are two distinct fences, not
one. This matters because they answer different questions and a design that conflates them gets one
of them wrong.

**The reclaim fence** — a run whose generation has been replaced cannot come back — runs at
`crates/herdr-tg/src/hub.rs:2727-2748`, inside the same critical section as the claim, and it is read
*before* the incumbent is:

> A run whose generation has been replaced is not a rival for the address — it is over. Told "already
> claimed" it would redial for ever, because that refusal is the one a bridge is supposed to wait out;
> and with nothing holding the address it would simply be let back in, which is a second voice in a
> conversation that has moved on.

Only a number *behind* the highest is refused (`hub.rs:2740-2747`); one *ahead* is admitted and the
mint climbs past it (`hub.rs:1786-1810`), because refusing that would lock a project out of its own
hub with no way back from a phone.

**The delivery fence** — a run that keeps talking after a later one took the address has every frame
answered `no` — runs at `crates/herdr-tg/src/hub.rs:5274-5320`, in the per-connection read loop
rather than inside `handle`, and the comment says exactly why:

> this loop is the one place every frame of a connection passes through, and it goes on running AFTER
> the claim has been released — draining what was queued behind a send that was in flight. That drain
> is where a run which is already over finishes its backlog into a conversation a later run now
> holds, and the claims map cannot see it because by then the map is empty.

It is `a_newer_run_holds` (`hub.rs:2880-2888`), which reads the generations map and never the claims
map, precisely so the answer survives an empty map.

**Downward confinement is per-address and total.** `deliver_under` (`hub.rs:2631-2656`) looks the
connection up by `Addr` and stamps the outgoing frame with *that claim's* generation. A tap is
resolved by the address written into the record, never re-derived — `hub.rs:2547-2556`:

> Looked up by the ADDRESS the record carries, so the lane travels in the record rather than being
> re-derived here. Keyed on the project alone this read either handed back some arbitrary lane's
> connection — a `Choice` delivered into a turn that never asked anything, with no error anywhere —
> or missed and called a lane not connected while it sat waiting.

And there is **no most-recent fallback anywhere in the hub**. `addr_for_topic`
(`crates/herdr-tg/src/hub.rs:3665-3681`) is explicit about refusing one:

> A lane's topic answers with the LANE. There is deliberately no falling back to the project when a
> lane's topic is not found: that would put what the operator typed at a worktree into the project's
> own turn, which is an agent reading an instruction meant for someone else.

**A tap on a question a restarted session asked is refused, not delivered** (`hub.rs:2557-2564`,
`TapRefusal::Restarted`), with a sentence the operator can read (`hub.rs:864-866`).

### What would have to be added

Three things, all of them small, and one of them is a widening the checkpoint should see plainly.

1. **An intent is confined the same way, by construction.** It is delivered through `deliver_under`
   at `Addr { project, lane: domain }` and nowhere else, so it inherits the address fence and the
   generation stamp with no new code. It must NOT be routed by anything on the frame — that is the
   sceptics' first attack in PLAN-v2 §F2 and it is the right one.
2. **The expected-generation check** is new hub logic, and it is the only place the hub compares a
   number the operator's side supplied against one it minted. §6 gives its exact table.
3. **The widening, said out loud.** Today the *only* thing the hub sends a bridge on its own
   initiative, absent an ask, is a `ping` (`crates/hub-proto/src/frame.rs:650-656`, HEAD). An
   `intent` is the first frame the hub sends **because a person asked it to** that is not the answer
   to a question the bridge itself posed. That is a genuine change in the direction of authority and
   it is why every field in §3 is closed and every one is justified individually.

**What this proposal does not claim.** `docs/HARDENING-HANDOFF.md:541-544` is honest that the hub's
tap ordering guarantees less than it looks like:

> The hub marks the record and delivers the choice in **two steps**, so a `choice` for an ask you
> have already resolved can still arrive. `docs/ATTACHING.md` asks the adapter to refuse it and says
> so plainly. The absolute ordering guarantee is deliberately **not published**.

The same is true of an intent, and for the same reason. The idempotency key in §4 is what makes that
survivable; it is not a proof that duplicates never happen.

---

## 3. The engine capability handshake

> *"Expose the minimum adapter contract Kickoff needs — engine/version, exact-session support,
> prompt/event endpoints, interrupt/drain behavior. Do not absorb Runner/container lifecycle into
> Herdr."*

### What already holds today

**There is a capability handshake on this wire, it works, and it is one field.** `hello.confirms`
(`crates/hub-proto/src/frame.rs:439-464`, HEAD) is a bridge naming which of the hub's own frames it
will answer for. The design is exactly right to copy, and its doc comment is the specification:

> A promise, and the hub holds nothing open waiting for an answer it was not promised. It has to be
> on the wire because the alternative is inferring it from a version number no bridge sends: a hub
> that assumed every bridge answers would sooner or later tell the operator his tap was never taken,
> about a bridge that took it and had no word for saying so.

Three properties of it are load-bearing and this proposal inherits all three:

* **Unknown names are ignored rather than refused** — `crates/hub-proto/src/frame.rs:448-451` (HEAD):
  *"a bridge that promises to confirm something this hub never sends has promised nothing, which is
  harmless, where a refusal would be a project that cannot connect because it was too new."*
* **It is read by a helper and never by asking whether the field was present** —
  `promises_to_confirm` at `crates/hub-proto/src/frame.rs:148-150` (HEAD), whose doc at `:140-147`
  names the failure: a reader that asks whether the promise was *present* rather than whether it
  *names this frame* tells the operator his answer was never taken by a session that took it. The
  hub reads it once, at admission, and stores the answer on the claim
  (`crates/herdr-tg/src/hub.rs:4853`, `crates/herdr-tg/src/hub.rs:1587-1591`).
* **An empty list is no promise** — `crates/hub-proto/src/frame.rs:151-162` (HEAD) — because an
  adapter that builds the list by filtering writes the empty one every time it promises nothing.

### What would have to be added

**One more optional field on `hello`, on exactly the `confirms` pattern**, and nothing else.

```
hello.controls? : [ { spec_id, domain?, allowed: [string], max? } ]
```

That is the whole capability handshake this proposal asks for. Note what it does **not** carry, and
why each omission is deliberate rather than an oversight:

| the checkpoint asked for | why it is not here |
|---|---|
| engine / version | `docs/CAPABILITIES.md:323` (REFUSES 5) — choosing an engine belongs to whoever dispatches. The hub renders nothing differently per engine and would be storing a string it never reads. |
| exact-session support | `docs/CAPABILITIES.md:328-329` (REFUSES 6) — no frame carries a session. Session enforcement is the adapter's, and `docs/ATTACHING.md` §13.10 already specifies it. |
| prompt / event endpoints | A URL is an address on the controller's own machine. The hub never dials anything; `docs/CAPABILITIES.md:276-280` (REQUIRES 3) has exactly one transport and it is a local socket the *bridge* dials. |
| interrupt / drain behaviour | Expressed as `allowed: ["drain", "stop"]` or the absence of them. A controller that cannot drain simply does not list it, and the hub then never offers the operator a button for it. |

**The echo, and the duty it creates.** `welcome.controls?` echoes back the controls the hub actually
admitted. This is not decoration; it is the `lane` echo's exact argument, at
`crates/hub-proto/src/frame.rs:575-586` (HEAD):

> An unknown field inside a known kind is ignored on purpose, which is what lets a new bridge talk to
> an old hub — but it also means an old hub admits a lane silently as the project itself […] A bridge
> that named one and gets no echo has learned the hub is older than it is, and must refuse rather
> than impersonate.

So: **a controller that declares controls and gets no echo has learned the hub is older than it is,
and must not act as a controller.** A hub that never sends an intent is indistinguishable from a hub
that has not decided to yet, and a controller that assumed the former would sit waiting for ever
while the operator's phone showed nothing.

**And the door's cost, named rather than solved.** `docs/HARDENING-HANDOFF.md:566-570` records a real
one for `confirms`:

> `confirms` is the **connection's**, made once for the whole door, while producers attach and detach
> behind it.

`controls` inherits it exactly. A controller behind `adapters/kickoff-hub-attach/relay.ts`'s door
declares through the door's own `hello` or not at all, and the relay's union of several producers'
declarations is out of scope here — `docs/ATTACHING.md:1480-1482` already says a promise belongs to
the connection that made it. **A controller should hold its own connection**, which is what the
existing seam-④ sketch always assumed (`docs/INTERFACES.md:245-256`).

---

## 4. The decision / command envelope

> *"Use operation ID, idempotency key, expected generation/revision, causation/correlation IDs,
> approval scope, and expiry. Delivery is at-least-once and duplicate-safe; do not claim
> exactly-once across the hub/adapter boundary."*

### What already holds today

* **Every frame the hub sends has an id, and exactly one answer comes back for it.**
  `mint_frame_id` (`crates/herdr-tg/src/hub.rs:2620-2622`) mints it *before* the frame goes down,
  deliberately: *"so the one caller that waits to hear what became of a frame can write the id down
  BEFORE the frame is on the wire."*
* **The hub already keeps a record of a thing it sent and has not been answered for.** `Down`
  (`crates/herdr-tg/src/hub.rs:1914-1923`) holds the frame id, the address, the chat and message it
  is about, and which of the operator's actions it was; `His` (`hub.rs:1927-1934`) is the two-variant
  discriminant. The list is bounded at 256 (`hub.rs:1979`) *"so a bridge that never answers must not
  turn a record nobody will read into a leak."* A window of twenty seconds
  (`TAP_CONFIRM_WINDOW`, `hub.rs:1987`) is how long a bridge that promised has before the operator is
  told it has not answered.
* **At-least-once is already the honest word here.** `Delivered`
  (`crates/hub-proto/src/frame.rs:165-181`, HEAD) has three values and not two, and the reason
  generalises to intents exactly:

  > Two values would be a lie. Telegram has no idempotency key, so a send that times out may or may
  > not have landed, and there is no way to ask. `Unseen` is that state named.

* **The operator is authenticated before anything is resolved.** `standing_of`
  (`crates/herdr-tg/src/hub.rs:2481-2503`) fails closed on the shape of the id itself before any list
  is read (`hub.rs:2486-2488`), and `Standing::may_command` (`hub.rs:844-846`) is the bot-wide list
  and nothing narrower.

### What would have to be added

The envelope, in full, is §6. Its four discipline fields and the failure each prevents are §7.

**One naming hazard, called out because it is one letter from a fatal collision.**
`crates/hub-proto/src/frame.rs:64-66` (HEAD) is a hard rule:

> **No payload field in either direction may be named `generation`**;
> `a_generation_rides_on_every_frame_in_both_directions_and_is_named_exactly_once` fails the day one
> is.

That test is real and lives at `crates/hub-proto/src/frame.rs:1780` (HEAD). `expected_generation` is
a different key and does not collide — but it sits beside the one key on this wire that cannot be
shadowed, on a frame whose whole job is fencing. **The guard must be widened to a prefix-aware check
or the field renamed**; §9 lists it as an open decision rather than settling it here.

---

## 5. The claim lifecycle

> *"Acquire/renew/withdraw must be generation-fenced so an old wall cannot retarget, acknowledge,
> retire, or speak for a newer run. A disconnect reports communication state only; it must not claim
> the workload itself is stopped/failed/healthy."*

### What already holds today

**Acquire is fenced, in one critical section, and it was not always.**
`claim_the_address` (`crates/herdr-tg/src/hub.rs:2702-2818`) does the fence, the incumbent check, the
mint and the reservation under one lock, and the comment at `hub.rs:2681-2694` records what it cost
to learn that:

> The check and the reservation used to be two […] Two bridges arriving inside that window were both
> admitted and the second silently replaced the first — measured at roughly one round in three when
> the two `hello`s land within about 100 µs on a multi-thread runtime.

**Renew is not a verb here, and that is correct.** There is no renewal frame. The lease is granted
once per claim on the `welcome`'s own envelope and holds for the life of the connection
(`crates/hub-proto/src/frame.rs:47-58`, HEAD; `crates/herdr-tg/src/hub.rs:1571-1574`). A run that
loses its socket redials and is re-welcomed with a fresh number, and the backlog it carried in is
safe because those frames were written before it could read the new welcome and so carry the old
number, which the delivery fence lets through (`crates/herdr-tg/src/hub.rs:5288-5296`). **Nothing in
this proposal adds a renewal**, because a renewal is a second way to hold an address and the whole
value of the current design is that there is one.

**Withdraw is fenced by generation as well as pid.** `release_this_run`
(`crates/herdr-tg/src/hub.rs:3143`) drops the claim only if it is still the one holding it. The
generations file (`hub.rs:1668`, `GENERATIONS_FILE`) survives the process precisely so a floor is not
lost across a restart — `hub.rs:1670-1690` says the failure it prevents is a clock stepping
*backwards*, which would re-hand a number already given, and *"a hub that re-hands a number it has
already given is a hub whose fence points the wrong way: the run it fences off is the live one."*

**A disconnect already reports communication state only.** This is the property the checkpoint asks
for and the hub already holds it, in three places:

* `Delivered::Unseen` (`crates/hub-proto/src/frame.rs:178-180`, HEAD) exists so the hub never claims
  a rung it did not observe.
* `presence.rs` is believed by a reader *only* when the lock's holder is alive, is a herdr-tg, and
  wrote it — otherwise `null` (`CLAUDE.md`, and the writer at `crates/herdr-tg/src/hub.rs:2905-2960`
  removes the file rather than leaving a stale one: *"Unknown is the honest answer; nothing connected
  is not."*).
* `docs/CAPABILITIES.md:240` (offer 7): *"**The alarm tells him and restarts nothing.** Restarting
  belongs to whoever dispatches."*

### What would have to be added

**Nothing to the claim lifecycle itself.** An intent hangs off a claim that already exists; it does
not create, renew or end one. Two additions to the *record* beside it:

1. `Claim` gains `controls`, read once at admission exactly as `confirms_choices` is
   (`crates/herdr-tg/src/hub.rs:1587-1591`, filled at `hub.rs:4853`). Read from the claim and never
   remembered elsewhere — `deliver_tap` already states the rule at `hub.rs:3263-3265`: *"the promise
   belongs to the connection that made it, and a run that has since been replaced cannot have its
   successor nagged for it."*
2. `His` (`crates/herdr-tg/src/hub.rs:1927-1934`) gains a third variant for an intent awaiting its
   outcome. One list and not two, for the reason already written at `hub.rs:1906-1911`: *"a bridge
   answers both with the same `ack{ref}`, one id counter mints both […] Two lists keyed the same way
   is two places for the same id to be looked up and one of them to win."*

**And one thing the hub must refuse to conclude.** An intent that was `accepted` and whose connection
then dropped has an unknown outcome — not a failure. The operator must be told the communication
state and nothing about the workload: *"It was handed on, and then the connection ended. I do not
know what became of it."* Reading a disconnect as a failure is the exact error
`docs/CAPABILITIES.md:240` and `Delivered::Unseen` were both written to prevent.

---

## 6. The proposed contract, in full

Additive in both directions. `v` stays `1` (`crates/hub-proto/src/frame.rs:36`, HEAD). Every optional
field is **omitted when it has no value, never sent as `null`** — `docs/ATTACHING.md:908-909` states
the rule for the whole wire, and every optional field in the crate already carries
`skip_serializing_if` for it.

**Frame counts move from nine up / six down to ten up / seven down.** That is a documented count in
four places (`CLAUDE.md`; `docs/ATTACHING.md:906` and `:945`; `docs/CAPABILITIES.md:275`) and each
would need the same edit.

### 6.1 Up — `hello.controls?`

```json
{"v":1,"id":"f1","t":"hello",
 "project_id":"unknown-until-the-hub-says","token":"<64 hex from the token file>",
 "instance":"<this run>","repo":"<a path, audit only>","pid":1234,
 "confirms":["choice"],
 "controls":[
   {"spec_id":"spec-coordinator","allowed":["start","stop","restart","inspect"]},
   {"spec_id":"spec-worker","domain":"engineering","allowed":["start","scale","drain","stop"],"max":4}
 ]}
```

### 6.2 Down — `welcome.controls?`, the echo

```json
{"v":1,"id":"h1","t":"welcome","project":"A Title",
 "limits":{"max_frame":65536,"max_text":3500,"frames_per_min":20},
 "controls":[
   {"spec_id":"spec-coordinator","allowed":["start","stop","restart","inspect"]},
   {"spec_id":"spec-worker","domain":"engineering","allowed":["start","scale","drain","stop"],"max":4}
 ]}
```

Echoed **as admitted**, not as sent: an op name this hub does not know is absent from the echo, and a
controller reads the difference. A `hello` that sent no `controls` gets a `welcome` with no
`controls`, byte for byte the welcome this protocol has always sent.

### 6.3 Down — `intent`, the seventh down-frame

```json
{"v":1,"id":"h44","t":"intent","generation":1757000000123,
 "intent_id":"i-7a1c","idempotency_key":"k-3f9e2b18",
 "op":"scale","spec_id":"spec-worker","domain":"engineering","count":3,
 "expected_generation":1757000000098,
 "from":{"chat_id":-1000000000000,"user_id":1000000},
 "valid_for_ms":120000}
```

Both numbers in `from` above are placeholders; no real chat or person is named anywhere in this
document.

### 6.4 Up — `intent_outcome`, the tenth up-frame

```json
{"v":1,"id":"f77","t":"intent_outcome","generation":1757000000123,
 "intent_id":"i-7a1c","status":"accepted"}
```

```json
{"v":1,"id":"f81","t":"intent_outcome","generation":1757000000123,
 "intent_id":"i-7a1c","status":"completed","reason":"three are running"}
```

### 6.5 The closed field set, and why each field is safe for the hub to carry

Every field, both frames, with the argument for each. A field not in this table is not in the
contract.

| field | on | who mints it | why the hub may carry it |
|---|---|---|---|
| `spec_id` | `hello.controls`, `intent` | the controller | **Opaque, and the hub cannot dereference it.** Same treatment as `AskId`/`OptionId` (`crates/hub-proto/src/ids.rs:8-9`): stored and echoed, never read into. It cannot smuggle an image because the hub has no table to resolve it against and the controller resolves it only against its own approved one. Length-bounded and shape-checked exactly as a lane is, so it cannot forge an audit line. |
| `domain?` | `hello.controls`, `intent` | the controller | **An address, never a credential** — the whole `LaneId` argument (`crates/hub-proto/src/ids.rs:80-91`), and it is checked by the same rule (`lane_is_addressable`, `crates/herdr-tg/src/hub.rs:2067-2081`). Absent means the conversation itself, exactly as `hello.lane` absent does (`crates/hub-proto/src/frame.rs:425-426`, HEAD). |
| `allowed` | `hello.controls` | the controller | **A list of names the HUB owns**, filtered against a table compiled into this binary. A name the hub does not know is dropped; the control survives. The controller cannot widen the set by naming something. |
| `max?` | `hello.controls` | the controller | A bound the controller sets on itself. The hub only ever compares `count` against it and refuses; it never raises it. |
| `op` | `intent` | **the hub** | Never a string from the wire. It is a variant the hub selected from its own closed set because the operator tapped a button the hub drew from an admitted control. |
| `count?` | `intent` | the hub | A small integer from a button the hub drew, bounded by `max`. |
| `intent_id` | `intent`, `intent_outcome` | **the hub** | The correlation. Minted like a frame id (`crates/herdr-tg/src/hub.rs:2620-2622`) and written down before the frame is on the wire. |
| `idempotency_key` | `intent` | **the hub** | §7.1. Minted by the hub so a controller cannot choose its own collisions. |
| `expected_generation?` | `intent` | **the hub** | A number this hub minted (`crates/herdr-tg/src/hub.rs:1786-1810`). It is never believed from anywhere else. |
| `from` | `intent` | **the hub** | Reuses the existing `From { chat_id, user_id }` (`crates/hub-proto/src/frame.rs:665-668`, HEAD), which the hub already sends on every `message` and whose doc says it is *"for the audit record and for the allowlist decision that has already been made by the time this frame exists."* No new exposure; one type; one spelling in the audit. |
| `valid_for_ms` | `intent` | **the hub** | A **duration, not a wall-clock instant**. A deadline compares two clocks; a duration compares one. The hub already reasons in milliseconds (`now_millis`, `crates/herdr-tg/src/hub.rs:1899-1904`) and keeps its audit timestamps deliberately dependency-free (`hub.rs:1381-1384`). |
| `status` | `intent_outcome` | the controller | A closed set of four: `accepted`, `refused`, `completed`, `failed`. §7.3. |
| `reason?` | `intent_outcome` | the controller | One short sentence in **his** words. Clamped, and **never written to the audit** — `HubAudit::file` already refuses to write a name from a phone into the log for exactly this reason (`crates/herdr-tg/src/hub.rs:1290-1293`): *"This file is one record per line, and a name from a phone can carry a newline — which would be a second record of the sender's choosing."* |

### 6.6 The exhaustive list of what the hub REFUSES to carry

Not "does not currently carry". **Refuses**, in the sense `docs/CAPABILITIES.md:311` uses: *"Each is
a line, not an omission."* There is no field for any of these on any frame in `hub-proto`, and this
contract adds none.

| refused | why, and where the line already is |
|---|---|
| **an image, image reference, tag or digest** | It would be a string from a phone naming what runs. `docs/CAPABILITIES.md:317-320` (REFUSES 3): inbound content SELECTS from what the machine already knows; it never NAMES something new. The approved image lives in Kickoff's spec and the hub carries only the handle to it. |
| **a command, argv, entrypoint or shell fragment** | The hub causes no process to exist. `docs/INTERFACES.md:278`; `docs/HUB-AND-KICKOFF.md:152-160` (Q2, answered *no*): *"it is not the process scheduler and must never gain an arbitrary command-execution surface."* An `op` is a variant, not a string. |
| **a mount, volume or bind spec** | Mounts are a property of the transport, decided by whoever builds the wall. `docs/CAPABILITIES.md:242` (offer 9) is explicit that the file path pair *"is therefore a capability of the transport, not of the wire"*, and the hub mints every path it writes. |
| **a secret, token or credential of any kind** | `docs/ATTACHING.md:747-749`: *"Configuration travels in the environment. The credential does not. […] Never a token value in a variable, in an argv, or in an image."* `KICKOFF_HUB_TOKEN` is a permanent refusal (`docs/ATTACHING.md:406`). A credential on an intent would be the same mistake with a new name. |
| **an environment variable, name or value** | An env var is a command-line by another spelling, and it is the standard way an image is retargeted without changing the image. Nothing in the contract is a free-form key/value pair, which is what makes this enforceable rather than aspirational. |
| **a host path, directory or cwd** | `docs/CAPABILITIES.md:328-329` (REFUSES 6): *"no frame carries a session, a directory or an engine's name."* `hello.repo` is audit-only and explicitly never for routing (`crates/hub-proto/src/frame.rs:422`, HEAD). The canonical directory lives in the launcher's binding file, outside the wire (`docs/ATTACHING.md:2447`). |
| **an engine name, a model, or a repository** | `docs/CAPABILITIES.md:323` (REFUSES 5): *"Choosing a model, an engine, or a repository. Those belong to whoever dispatches."* |
| **an engine session id** | `docs/CAPABILITIES.md:326-331` (REFUSES 6), and codex-steering's own `20260906T194228Z`: *"No session field should enter the hub wire; that boundary remains correct."* |
| **a port, URL, host or socket path** | The hub dials nothing. `docs/CAPABILITIES.md:276-280` (REQUIRES 3): one transport, a local socket, and the bridge is the party that dials. |
| **a systemd unit name, or any other name of a thing to run** | `docs/HUB-AND-KICKOFF.md:110-121` proposes the hub running `systemctl --user start kickoff@<project>`; Q2 at `:150-160` answers **no** and supersedes it. §8 records that the two sections of that document contradict each other. |
| **a uid, gid, capability set, seccomp profile or privilege flag** | Admission is on the peer's uid as the kernel reports it (`docs/CAPABILITIES.md:276-280`), which is a fact the hub reads and never one it is told. A privilege named on the wire is a privilege the sender chose. |
| **any free-form map, blob, or `extra` field** | A single open field defeats every row above at once. The field set is closed, and an unknown field inside a known kind is ignored (`crates/hub-proto/src/frame.rs:22-25`, HEAD) rather than stored — so a controller cannot smuggle one through and the hub cannot accidentally forward it. |

### 6.7 The shape a stranger implements from

For a controller author who has read `docs/ATTACHING.md` §6 and nothing of this repo:

1. Dial the hub and send `hello` as §6 already says, adding `controls` — one entry per
   (spec, domain) pair you are willing to be asked about, with the ops you can actually perform.
2. Read `welcome`. **If it carries no `controls` echo, you are not a controller on this hub.** Say so
   in your own logs and behave as an ordinary bridge. Do not act, and do not wait.
3. Compare the echo to what you sent. An op missing from the echo is one this hub does not know; you
   will never be asked for it. A control missing entirely means its shape was refused.
4. Handle `intent`. Answer **every one** with `intent_outcome{intent_id, status}`, `accepted` first
   and then exactly one terminal status (`completed`, `failed`, or `refused` on its own if you never
   accepted). `accepted` means *the work has started*, not *I read the frame* — the same standard
   `docs/ATTACHING.md:1344-1347` (rule 13) already sets for a `choice`.
5. Treat delivery as **at-least-once**. If `idempotency_key` is one you have already acted on, answer
   from your record and **do not re-execute**.
6. Resolve `spec_id` against your own approved table and nothing else. If it names a spec you do not
   have, answer `refused` with one sentence a person can read. The hub will not have checked it — it
   cannot.
7. `intent` is also a frame after `hello`, so the hub acks it like any other. That ack is
   bookkeeping; do not answer it.

---

## 7. Idempotency, expected generation, and the outcome vocabulary

Each with the failure it prevents, because a discipline field whose failure nobody can name is a
field somebody deletes.

### 7.1 The idempotency key

**Minted by the hub**, from the offer message, the option tapped, and the generation shown — so two
taps on one button are one key, and the same button drawn again after the world moved is a different
one. Recorded before the frame goes down, on the discipline `resolve_tap` already holds at
`crates/herdr-tg/src/hub.rs:2585-2591`:

> Fail closed: if the answer cannot be written down, it must not be sent. An unrecorded answer is one
> that can be given again.

A repeated key is answered **from the record**, and the frame is not re-delivered.

**The failure it prevents.** The operator taps *Restart*, the receipt is slow, he taps again. Without
the key that is two restarts. `Delivered::Unseen` (`crates/hub-proto/src/frame.rs:178-180`, HEAD)
already tells us the hub cannot know whether the first one landed, so the key is the only thing that
can make the second one safe. **Exactly-once is not claimed** across the boundary — the checkpoint is
right that it cannot be — but duplicate-safe is achievable and this is how.

### 7.2 Expected generation

Which ops require it, and what each answer is:

| op | `expected_generation` | why |
|---|---|---|
| `start`, `inspect` | **refused if present** | There is nothing running to have a generation. Accepting one would be accepting a number that means nothing, which is how a check becomes decorative. |
| `scale`, `drain`, `stop`, `restart` | **required** | Each of them changes something that is running, and "which run" is the whole question. |

Compared against the live claim's generation at `Addr { project, lane: domain }`, or — when nothing
is connected there — against the highest this hub has handed out (`highest_for`,
`crates/herdr-tg/src/hub.rs:1839-1841`). An address never claimed returns `0` from that map
(`hub.rs:1840`), which is **not** a generation: `crates/hub-proto/src/frame.rs:68-76` (HEAD) says a
zero *"is read as absent"*. So a domain the hub has never seen must be its own refusal and never a
comparison against zero.

**The failure it prevents.** The operator is shown three workers, walks away, the wall restarts, he
comes back and taps *Scale to 1*. Without the fence that scales the *new* run to one. With it he is
told, in words: *"That has changed since you were shown these buttons. Look again."*

This is the same fence the wire already runs twice — the reclaim fence at
`crates/herdr-tg/src/hub.rs:2727-2748` and the delivery fence at `hub.rs:5274-5320` — applied to a
number the operator's *view* was built from rather than to a connection.

### 7.3 The outcome vocabulary

Four words, and it is four rather than two for the reason `Delivered`
(`crates/hub-proto/src/frame.rs:165-181`, HEAD) is three rather than two: a vocabulary that cannot
express a state the system really enters is a vocabulary that lies about it.

| status | means | the operator reads |
|---|---|---|
| `accepted` | the work has started, and a terminal status will follow | *On its way.* |
| `refused` | it is not happening and will not be later | *It was refused.* (+ the sentence) |
| `completed` | it finished, and did what was asked | *Done.* (+ the sentence, when there is one) |
| `failed` | it started and did not finish | *It did not work: <sentence>.* |

**The failure `accepted` prevents.** Two words — done / not done — force a controller that has begun
a thirty-second restart to choose between lying twice. Split, the operator sees his tap land, and the
line is edited when the truth arrives. Edits cost nothing against the send ceiling
(`docs/CAPABILITIES.md:445`; `docs/RATE-PROBE.md` §3), which is what makes this affordable.

**The failure `failed` prevents, distinct from `refused`.** *Refused* means nothing happened; *failed*
means something did and did not finish. Collapsing them tells the operator nothing changed when
something did — the single worst thing this system can tell him, because his next action is chosen on
it.

**Legal transitions, enforced at the hub.** `accepted` → one of `completed` / `failed`. A terminal
status may arrive first, with no `accepted` before it. Nothing follows a terminal status; a second
one is dropped with one audit line. An outcome for an intent the hub never sent writes nothing in his
topic and one line in the audit — `HubAudit::refused` (`crates/herdr-tg/src/hub.rs:1282-1286`) is
already the shape, and its doc says why: *"A branch that sends nothing still writes a line, so silence
in this file always means the process stopped rather than that the hub decided something quietly."*

### 7.4 The audit record

One line per intent sent and one per outcome, in the existing tab-separated format
(`crates/herdr-tg/src/hub.rs:1337-1350`), with `subject(addr)` (`hub.rs:1374-1379`) so a search for a
project finds every line its domains wrote. Fields: the op, the spec id, the intent id, the
idempotency key, the expected generation, and the user id — the last for the reason
`HubAudit::stranger` gives at `hub.rs:1316-1322`, that an id in this file is one the operator can
copy. **`reason` is never written**, per §6.5.

---

## 8. The partition: what this builds here, what stays Kickoff's

### Built in `herdr-tg`

| | |
|---|---|
| `crates/hub-proto/src/ids.rs` | two `opaque_id!` newtypes: `SpecId`, `IntentId` |
| `crates/hub-proto/src/frame.rs` | `hello.controls?`, `welcome.controls?`, `HubFrame::Intent`, `BridgeFrame::IntentOutcome`, a `Control` struct, an `IntentStatus` enum, an `Op` enum, one new `RefusedReason` |
| `crates/herdr-tg/src/hub.rs` | controls admitted and shape-checked; `Claim.controls`; `Hub::intend`; `intent_outcome` handling; a third `His` variant; the audit lines |
| a new module | the intent ledger: the record, the idempotency map, the bounds, and the operator's sentences |
| `crates/herdr-tg/tests/` | the guards of §9 widened, and the adversarial suite of §10 |
| `docs/` | `ATTACHING.md` (a new section, and the frame tables), `CAPABILITIES.md` (a tenth offer, OPEN 1 narrowed), `INTERFACES.md` (seam ④, and the closed list gains a seventh **as a decision**) |

### Left to Kickoff, entirely

* **The AgentSpec** — the image, the command, the mounts, the secrets, the environment, the host
  paths, the resource limits, the network. All of it. The hub carries the handle and never the thing.
* **Every runner and container lifecycle** — creating, supervising, draining and destroying.
* **Which session of an engine a conversation is bound to**, and enforcing it in both directions.
  `docs/CAPABILITIES.md:366-395` (OPEN 5) is the settled split and this proposal changes nothing in
  it.
* **Whether an intent is a good idea** — quotas, cost, scheduling, blast radius. The hub asks
  *"is this one of the things you said you could do, for a run that is still the run I showed him?"*
  and nothing else.
* **What `accepted` means in practice**, and when `completed` is honest.
* **Minting the spec ids**, and keeping them stable across restarts.

### The line, said once

**The hub carries a handle and a verb. Kickoff owns everything the handle resolves to.** If a change
would require the hub to resolve a handle, look inside a spec, or learn one word of what a spec
contains, it is on the wrong side of this line.

### Where PLAN-v2 §Slice F2 and the code disagree — the code wins

PLAN-v2 was drafted against an earlier tree. Five things in F2 do not survive contact with the code
as it now stands, and one more is a hazard rather than an error.

1. **`domain` spelled `-` for "the conversation itself" is refused by the hub's own rule.** F2 says
   *"absent lane = the conversation itself, spelled `-`"* and that the shape rule is *"generalised
   from `lane_is_addressable`"*. Those two sentences contradict each other:
   `lane_is_addressable` explicitly refuses `-` (`crates/herdr-tg/src/hub.rs:2073`), and the doc
   comment at `hub.rs:2058-2066` gives a hard operational reason — `-` **is** the project's own voice
   on disk in both file trees, so a lane admitted under that name would be handed the project's own
   media and outbox directories. **This proposal omits `domain` instead**, which is the spelling
   `hello.lane` and `Addr.lane` already use for exactly this meaning
   (`crates/hub-proto/src/frame.rs:425-426`, HEAD; `crates/herdr-tg/src/hub.rs:2006-2008`), and which
   the whole wire's omit-when-absent rule already covers (`docs/ATTACHING.md:908-909`).
2. **`Op` must not be `#[serde(other)] Unknown`.** F2 specifies exactly that. This repo has already
   measured what it costs: `crates/herdr-client/src/proto/model.rs:37-42` records that
   `#[serde(other)]` *"compiles and looks right, but it DISCARDS the wire string: verified by
   compiling both"*, and `crates/herdr-client/tests/golden.rs:281-303` is a standing test that
   forbids it. Every closed set this wire branches on today — `RefusedReason`
   (`crates/hub-proto/src/frame.rs:230`), `AckWhy` (`:186`), `Delivered` (`:173`), `AckStatus`
   (`:554`), all HEAD — has **no** catch-all; the two `#[serde(other)]` in the crate are on the
   internally-tagged frame enums (`:547`, `:658`), where they mean something else entirely.
   **Instead**: read `allowed` as a list of plain strings and map known names to `Op`, dropping
   unknown ones — which is precisely the `promises_to_confirm` pattern
   (`crates/hub-proto/src/frame.rs:140-149`, HEAD) and its argument: *"a name this hub does not send
   is a promise about nothing, which is the same as no promise, and both fail towards saying less
   rather than more."*
3. **"There is no `Command` in the binary" is false as written, and F2's test name inherits it.** F2
   plans `nothing_the_bot_reads_can_reach_a_process_and_the_hub_has_no_command`. The hub binary
   contains three `std::process::Command::new("git")` call sites today, all in enrolment:
   `crates/herdr-tg/src/cmd/enroll.rs:313`, `:330` and `:387`. They are terminal-only, reached from
   argv, with fixed argv and no string from the wire — which is the property that actually matters —
   but `docs/INTERFACES.md:278` (*"There is no `Command` in the binary"*) and
   `docs/HUB-AND-KICKOFF.md:112` (*"zero `Command` in the binary"*) are both literally untrue and
   `docs/CAPABILITIES.md:240` and `:336` repeat the claim. **A scoped guard is what can be written
   and what is worth writing** — no `Command` reachable from anything inbound — and the three doc
   sentences should be corrected in the same commit rather than a test being named after a property
   the tree does not have.
4. **`docs/HUB-AND-KICKOFF.md` contradicts itself, and F2's plan to "rewrite §4 and Q2" is right for
   a reason it does not give.** §4 at `:110-121` says *"the hub starts named systemd units that
   already exist"* and gives the `systemctl --user start kickoff@<project>` line. Q2 at `:150-160`
   answers the same question **no**, and adds the sentence this whole proposal is the interface for:
   *"Lifecycle intentions may travel as typed, capability-checked operations against specs the
   launcher has declared in advance — never as an image, a command, a mount, a secret, an environment
   variable or a host path from Telegram."* Q2 is later and is the operator's ruling; §4 is stale.
5. **PLAN-v2's line citations no longer land.** F1 cites *"hub.rs:3670-3684"* for the welcome fill;
   at HEAD that range is `addr_for_topic` (`crates/herdr-tg/src/hub.rs:3668`). Treat every file:line
   in PLAN-v2 as needing re-checking before use.
6. **The hazard, not an error.** F2's `expected_generation` sits one qualifier away from
   `generation`, the one payload name this wire forbids
   (`crates/hub-proto/src/frame.rs:64-66`, HEAD). It does not collide. But the guard that enforces
   the rule checks for the exact name, so nothing would catch a later rename to `generation`. §9
   leaves the choice — widen the guard, or pick a name that cannot be shortened into the reserved
   one — open.

**One more thing the code says and PLAN-v2 does not.** F2 plans the wire and the hub and defers the
Telegram surface entirely (*"No Telegram surface in this slice: `intend` is called by tests only"*).
That is the right order and this proposal keeps it — but it means **the operator cannot reach any of
this until a later slice**, and the honest way to write that down is that the contract ships
unexercised by a person. `docs/HARDENING-HANDOFF.md:526-531` is the precedent for saying so: a
capability proved only against a fake is proved against a fake.

---

## 9. How an intent originates: he SELECTS, he never NAMES

This is the repo's oldest line and the one this proposal is most at risk of crossing, so it is worth
stating as a mechanism rather than as an intention.

**An intent is a `choice` that grew a type.** It originates the same way a tap does today:

1. A controller connects and declares, at `hello`, what it can be asked to do. Nothing the operator
   ever types can add to that list — a control arrives on a connection that has already proved a
   secret (`crates/herdr-tg/src/hub.rs:2702-2818`), and the hub admits it or drops it.
2. When a controller becomes live, the hub posts **one message with buttons** into that conversation's
   topic — one button per (spec, domain, op) the controller declared and the hub knows. The labels
   are the hub's own words. The callback payload is a key into a record the hub wrote.
3. He taps. The tap is resolved against that record, and **only** against it.
4. The hub builds the `intent` from the record — the op is a variant the hub selected, the spec id is
   one the controller declared, the generation is one the hub minted — and delivers it to the claim
   at that address.

Nothing typed can enter this path. The four guards, quoted:

**`HubFrame::Message`, `crates/hub-proto/src/frame.rs:613-616` (HEAD)** — the sentence itself:

> **Opaque.** The hub does not parse it, does not act on it, and does not let it name anything.
> **Inbound content selects; it never names.**

The same sentence is on the relay path at `crates/herdr-tg/src/hub.rs:3685-3686`.

**A line that is not a command is never parsed. `crates/herdr-tg/src/bot.rs:332-340`:**

> A command opens with a slash; nothing else is one. The parser is trusted only past that point […]
> ```rust
> if !text.starts_with('/') {
>     return Typed::Steering;
> }
> ```

`Typed::Steering` is *"Words for whatever is running in the topic. Relayed verbatim; never parsed"*
(`crates/herdr-tg/src/bot.rs:314-315`). There is no branch anywhere that reads a word out of it.

**A button is resolved against a written record, never against its position.
`crates/hub-proto/src/ids.rs:73-75`:**

> Resolved against the record written down beside the message, never against a button's position.
> Position is how a button reading "Reject" once confirmed "Allow always".

Enforced at `crates/herdr-tg/src/hub.rs:2538-2540`: an option that is not one of the ones written down
is `TapRefusal::NotAnOption`, before anything is delivered.

**The command set is a closed list of two, and a test fails the build if it grows.**
`crates/herdr-tg/tests/nothing_inbound_can_add_a_person.rs:238-248`:

> ```rust
> assert_eq!(
>     variants,
>     vec!["Projects,", "Help,"],
>     "the command set grew: {variants:?}. Letting a person speak is a decision made at a \
>      keyboard, never from a message."
> );
> ```

**What this proposal adds to that list of guards, and it must be added in the same commit:** a scan
that no field of any intent-family frame is typed such that it could carry an image, a command, a
mount, a secret, an environment variable or a host path — that is, that the field set is closed and
every member of it is either an opaque id, a bounded integer, a variant of a hub-owned enum, or a
sentence that never reaches the audit. Named in §10.

---

## 10. The adversarial contract tests

Full sentences, as this repo names tests, and each with what it would prove. The seven the checkpoint
names are first.

### The checkpoint's seven

1. **`two_sessions_in_one_directory_are_two_addresses_and_an_intent_for_one_never_reaches_the_other`**
   Two claims at `Addr{p, Some("a")}` and `Addr{p, Some("b")}` from one process — which is the real
   shape, since one opencode adapter holds every lane and they therefore share an `instance`
   (`crates/herdr-tg/src/hub.rs:2557-2561`). An intent for `a` arrives on `a`'s connection and `b`'s
   sees nothing. **Proves** the address and not the directory is the unit of confinement, and that
   the instance check cannot be relied on to separate them.

2. **`an_intent_is_routed_by_the_claims_address_and_never_by_a_field_on_the_frame`**
   Deliver an intent whose `domain` names a different lane than the record it was built from.
   **Proves** the frame cannot retarget itself — a globally resolvable id on the wire buys the sender
   nothing, because routing reads `Addr` from the record and the claim, exactly as `resolve_tap`
   already does (`crates/herdr-tg/src/hub.rs:2547-2556`).

3. **`an_intent_with_a_stale_expected_generation_is_refused_before_delivery`**
   The claim at the domain is at generation *g*. `Some(g-1)` → refused. `Some(g)` → delivered. The
   worker reconnects at *g′ > g*; `Some(g)` → refused. A domain never claimed → its own refusal, and
   **not** a comparison against the zero `highest_for` returns
   (`crates/herdr-tg/src/hub.rs:1839-1841`). `None` on `restart` → refused. `Some(g)` on `start` →
   refused. **Both spellings of the conversation itself are pinned** — omitted `domain`, and the
   `domain` a caller might be tempted to spell `-` — so an expected generation can never be compared
   against a different address's claim. **Proves** the fence points the right way in all six cases.

4. **`an_intent_repeated_with_the_same_key_is_answered_from_the_record_and_never_redelivered`**
   Two taps, one key. The second returns the first's status; no second frame reaches the connection.
   **Proves** duplicate-safety without claiming exactly-once.

5. **`an_intent_key_minted_before_a_rollover_is_not_the_key_minted_after_it`**
   Send an intent, let the address roll to a new generation, draw the buttons again, tap the same
   button. The key differs, and the new intent is delivered rather than answered from the old record.
   **Proves** replay-after-rollover is a *different* operation and is not silently deduplicated into
   the old one — the mirror of test 4, and the one that fails if the key is minted from the option
   alone.

6. **`an_outcome_for_an_intent_this_hub_never_sent_writes_nothing_in_the_topic_and_one_line_in_the_audit`**
   A controller sends `intent_outcome` for an unknown `intent_id`. Nothing reaches Telegram; one
   audit line is written. **Proves** a non-bound event cannot put words on the operator's phone —
   which is the general form of the checkpoint's "non-bound question", and the hub-side half of the
   hole `20260906T194228Z` §3 names.

7. **`an_intent_survives_an_adapter_restart_as_an_unknown_outcome_and_never_as_a_failure`**
   Deliver an intent, get `accepted`, drop the connection. The operator is told the connection ended
   and the outcome is unknown. A redial does **not** resurrect the record as `failed`, and the
   controller answering late — after the redial, from its own ledger — still corrects the line.
   **Proves** the checkpoint's fifth question end to end: a disconnect reports communication state
   only. Queued delivery is the other half: frames the hub held for a bridge that never ponged are
   acked `no` before the connection is refused (`CLAUDE.md`, and the pre-pong hold at
   `crates/herdr-tg/src/hub.rs:2377-2380`), and an intent must be no exception.

### The ones this proposal adds

8. **`the_hub_carries_no_field_that_could_name_an_image_a_command_a_mount_a_secret_an_environment_variable_or_a_host_path`**
   A scan of the intent-family frames: every field is an opaque id, a bounded integer, a variant of a
   hub-owned enum, or a sentence that never reaches the audit. Scoped, with a comment saying why the
   scope is what it is. **Proves** §6.6 is a property of the types and not a promise in a document.

9. **`a_controller_that_did_not_declare_a_capability_is_never_sent_an_intent_for_it`**
   **Proves** the hub cannot invent an op, a spec or a domain.

10. **`two_live_claims_of_one_project_declaring_one_domain_are_refused_rather_than_disambiguated`**
    **Proves** the hub never picks the newer. Silence and a refusal he can read, never a guess.

11. **`a_hello_without_controls_is_byte_for_byte_the_hello_this_protocol_has_always_sent`**
    The crate's standing pattern (`crates/hub-proto/src/frame.rs:748`, `:1103`, `:1260` at HEAD, and
    every optional field before this one). **Proves** additivity as bytes rather than as a round
    trip, which is what catches a `"controls":null`.

12. **`a_control_naming_an_operation_this_hub_does_not_know_is_dropped_and_the_connection_is_kept`**
    **Proves** the forward-compatibility rule of §8.2 — and, with the echo, that a controller can
    tell what was dropped.

13. **`a_controller_that_declares_controls_and_gets_no_echo_learns_the_hub_is_older_and_does_not_act`**
    The `lane`-echo argument (`crates/hub-proto/src/frame.rs:575-586`, HEAD) applied to controls.
    **Proves** a new controller against an old hub fails closed rather than waiting for ever.

14. **`an_outcome_reason_carrying_a_newline_writes_no_second_line_in_the_audit`**
    **Proves** the clamp. The failure is already documented for a filename at
    `crates/herdr-tg/src/hub.rs:1290-1293`.

15. **`an_intent_this_hub_holds_no_capability_for_is_refused_in_words_that_name_no_id_and_no_enum`**
    Every operator-facing sentence, walked. **Proves** the quality bar's *"Operator-facing strings
    carry no jargon"*, which `TapRefusal::say` (`crates/herdr-tg/src/hub.rs:851-874`) already holds
    to.

16. **`nothing_the_bot_reads_can_reach_a_process`**
    The scoped guard §8.3 argues for: no `std::process::Command` reachable from `bot.rs`, `hub.rs`, or
    anything they call, watched RED first on a planted call site. **Proves** the property the tree
    actually has, rather than the one three documents currently claim.

**RED before GREEN, and it is not optional here.** `CLAUDE.md` is explicit: *"A regression test that
never failed proves nothing."* Tests 3, 4, 5 and 8 are the ones most likely to be written green by
accident — 3 because a wrong comparison still refuses something, 4 because a slow test looks
deduplicated, 5 because the key can be minted from the option alone and nobody notices until a
rollover, and 8 because a scan with a typo in the field list passes vacuously.

---

## 11. What this proposal refuses

Beyond the field-level refusals of §6.6, which are the substance:

1. **A generic RPC.** No `op: String`. No pass-through payload. No `extra`. The op set is closed and
   compiled in, and adding a seventh op is a decision the same way
   `docs/INTERFACES.md:259` says a seventh capability is: *"adding a seventh is a decision, not a
   refactor."*
2. **The hub starting anything.** `docs/HUB-AND-KICKOFF.md:150-160` (Q2) settled it. `systemctl` does
   not appear in this design in any form, and §8.4 records that `docs/HUB-AND-KICKOFF.md:110-121`
   still says otherwise and is stale.
3. **A Telegram command for any of this.** The command set stays two, guarded at
   `crates/herdr-tg/tests/nothing_inbound_can_add_a_person.rs:238-248`. An intent is a tap on a
   keyboard the hub drew, or it does not exist.
4. **Learning what any of the words mean.** `docs/CAPABILITIES.md:324-331` (REFUSES 6). The hub does
   not know what an AgentSpec is, what scaling does, or what a domain contains. It knows the
   controller said it could do a thing, and that the operator picked that thing.
5. **A renewal frame, or any second way to hold an address.** §5.
6. **Any claim of exactly-once.** §7.1, and the checkpoint asks for exactly this restraint.
7. **Inferring a workload's state from a connection's.** §5, and `docs/CAPABILITIES.md:240`.
8. **Retrying an intent.** Never automatically, in either direction. The hub retries nothing that
   carries buttons today, for a stated reason (`crates/hub-proto/src/frame.rs:178-180`, HEAD: *"two
   live menus for one question, both tappable forever, is a misfire this system would have built"*),
   and an intent is strictly worse to double-send than a question.
9. **Building any of it now.** This is a proposal. Nothing here is built and no file outside this one
   was changed to write it.

---

## 12. Open questions — the operator's, or joint

These are genuinely undecided. None is a decision this document made quietly.

### The operator's

1. **Does he want lifecycle buttons on his phone at all?** Everything in this proposal is machinery
   for one moment: a keyboard in a topic offering *Start · Scale · Drain · Stop · Restart · Inspect*.
   `docs/CAPABILITIES.md:335-339` (OPEN 1) has listed the actor as undecided since 3 September, and
   the honest position is that the wire can be built and the surface refused, but the wire has no
   other purpose.

2. **Does a seventh capability get added to the closed list?** `docs/INTERFACES.md:257-273` lists six
   and says a seventh is a decision. This is a seventh: *carrying a typed intention from a keyboard to
   a controller.* It is not spawning, it is not supervising, and it is not a shell — but it is new,
   and calling it "just another `choice`" would be the kind of framing this repo distrusts.

3. **Is the offer message posted once per controller, or drawn on demand?** Once per (project,
   domain) when a controller becomes live and edited in place is cheapest — edits are free
   (`docs/CAPABILITIES.md:445`) and a send is not (`docs/CAPABILITIES.md:441-444`: twenty a minute per
   *chat*, and topics buy nothing). But a permanent keyboard in a topic is a permanent way to restart
   production with a mis-tap, and the generation fence bounds the damage without eliminating it.

4. **Should `stop` and `restart` need a second tap?** Nothing in this system asks twice today. A
   confirmation step costs one more free edit and is the only thing standing between a pocket and a
   stopped fleet.

### Joint with Kickoff

5. **Where does the idempotency key come from when the buttons land?** This proposal mints it from
   (offer message, option, generation shown). PLAN-v2 §F2 names this as a thing to *"decide before the
   buttons"* and reaches the same answer. It needs Kickoff's agreement, because Kickoff is the party
   that must not re-execute on a repeat.

6. **How does a controller declare a domain it has not created yet?** `start` is the one op whose
   domain may not exist. Either the controller declares the domains it *could* create — which means
   declaring names before there is anything at them — or `start` carries no domain and the controller
   picks. The second is simpler and gives the operator less to look at; the first is the only one
   where he can be shown *where* a thing will start.

7. **Is `spec_id` stable across a controller restart, and who guarantees it?** The hub echoes it and
   cannot check it. A controller that mints fresh ids on every boot silently invalidates every
   keyboard already on his phone — the generation fence catches it only if the domain's generation
   also moved.

8. **Does an intent belong in `docs/CAPABILITIES.md` as an offer, or in OPEN until the surface
   exists?** An offer is a promise another org builds on
   (`docs/CAPABILITIES.md:187-190`), and a wire with no surface is a promise about a thing nobody can
   use yet.

9. **The `expected_generation` naming hazard.** §8.6. Widen the guard to reject any payload field
   whose name contains the reserved one, or choose a name that cannot be shortened into it. Either is
   fine; leaving both undone is not.

### Answered here, and recorded so nobody re-opens them by accident

* **Does a session id go on the wire?** No. `docs/CAPABILITIES.md:326-331`, and codex-steering's own
  `20260906T194228Z`: *"No session field should enter the hub wire; that boundary remains correct."*
* **Does the hub run `systemctl`?** No. `docs/HUB-AND-KICKOFF.md:150-160` (Q2).
* **Does the hub validate a spec?** No. It cannot, and pretending otherwise would be the first step
  towards it holding one.
