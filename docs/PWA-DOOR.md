<!-- CONTRACT, 18 September 2026. Written against HEAD 9fb27ed. Every route, body, status code
     and sentence below was read in the code that serves it — crates/herdr-tg/src/gateway.rs (the
     door), crates/herdr-tg/src/hub/door.rs (the ring), crates/herdr-tg/src/hub/answers.rs (the
     drop), crates/herdr-tg/src/hub/tests.rs (the trial) — and every behaviour named "yours" was
     read in your repo: bridge/test_hub.py, bridge/serve.py, bridge/pushwatch.py and
     app/index.src.html. Where this file and the code disagree, the code wins and we fix the
     file; nothing here is checked against a second copy of itself.

     This is the door's half of an agreement whose other half your own tests hold: the shapes
     are transcribed from your pinned stub (bridge/test_hub.py) rather than designed, because a
     client written against the stub working against the real thing is the whole cost advantage
     of adopting it. docs/ATTACHING.md is the contract for an ADAPTER — a process holding the
     claim, speaking hub-proto on a socket. This file is the contract for a READER — a process
     that never touches a socket, holds no claim, and speaks HTTP on loopback. -->

# The PWA's door — `kickoff-door`, and the `/v1` seam it serves

You are an engineer on the surface side. Your PWA already speaks a `/v1` seam — a stream, a
poll and a write door — through your own bridge (`bridge/serve.py`, which proxies the three
routes and holds the token so the browser never does), and that seam's shapes are pinned by
your own tests against a stub. This document describes the real binary that stands where your
stub stood: `kickoff-door`, the one program in this workspace that listens. It is written to
be implemented against by a stranger who has never opened this repo — every route, every
body, every refusal, and the laws underneath them.

Nothing of yours needs to be installed from here. The door serves three routes on loopback;
your bridge already proxies exactly those three.

---

## 1. The shape, in one paragraph

The hub appends one line per operator-visible event to a ring file (`hub.ring.ndjson`, with
one rotated old file beside it) and sweeps an answers drop (`<state>/answers/`) about once a
second; `kickoff-door` is a separate binary that reads the ring and serves it as `/v1` over
HTTP on 127.0.0.1 only, takes your writes as `POST /v1/commands`, and turns each into one
file in the drop — the hub consumes it, writes a `.result` beside it, and the door answers
your POST with what that result says. **That is the whole mechanism.** No hub-proto frame
crosses this seam, nothing holds a claim through it, and the two files are the entire
interface between the hub and the door — a later reader could consume them directly, which is
why they carry their own laws (§2, §5) rather than borrowing the socket's.

---

## 2. The envelope

Every event — on the stream and in the poll, identically — is one ring line: a single JSON
object, served **verbatim**. The door never renumbers, reorders, filters or rewrites what it
serves; your cursor discipline (§3) is built on that.

```json
{"seq":41,"ts":1763390000,"dir":"up","conversation":"p-0123456789ab","lane":"fix-17","frame":{…}}
```

| field | what it is |
| --- | --- |
| `seq` | The cursor. A plain increasing integer, never reset — not across a rotation of the ring's files, not across a restart of the hub. Your position is "the last `seq` I saw", and everything after it is what you are owed. |
| `ts` | Unix seconds, a number, stamped where the hub first handled the frame. (Your stub's `"ts":"t"` placeholder was a string; nothing in your client reads `ts`, and ours is a number — say so now so nobody pins the stub's shape by accident.) |
| `dir` | `"up"` is the agent's half — its `say`, `ask`, `done`, a question stopping being open, and the hub's own follow-ups about your acts (below). `"down"` is the operator's half — his typed words and his taps, stamped **only where the frame was handed to a live session**. A down line is a receipt: it never appears for words that reached nobody. What became of a delivered act *later* is an up-line follow-up, never an edit of the receipt. |
| `conversation` | The id of the conversation this event belongs to: `p-` or `c-` then twelve lower-case hex characters, exactly fourteen characters. Not a path, not a name — the shape is refused at every door here, and the id is the only project-identifying thing the ring ever carries (§5). |
| `lane` | Which conversation *of that project* said it: the lane's own address (a bare name — see the lane law in §4), or the single character `-`, which is the spelling for **the project's own voice**. Your `hubLaneKey` already strips a `lane/` prefix and treats the empty key as the fleet's own row; `-` plays that role here. |
| `frame` | The event itself, in the wire's own vocabulary. Up frames: `{"t":"say","text":…}`, `{"t":"ask","ask_id":…,"text":…,"options":[{"option_id":…,"label":…}]}` (options absent means free-text), `{"t":"done","text":…}`, `{"t":"ask_resolved","ask_id":…,"how":…}` — `how` is `"answered"`, `"withdrawn"` or `"timeout"` when a bridge said it, or a plain sentence when the hub itself put a dead session's question away. Down frames: `{"t":"message","text":…}` and `{"t":"choice","ask_id":…,"option_id":…}`. Nothing else is recorded — the `hello` that carries the token never reaches this file, nor any `ack`/`bye`/`beat` bookkeeping except the follow-up lines below. |

**The egress law, which is the whole design.** This file is built to leave the machine. It
carries no chat id, no topic id, no user id, no Telegram message id, no filesystem path and
no token. Every string in every frame is scrubbed and clipped in one pass: absolute and
home-relative paths become `[a path]`, and everything is clipped at 3500 characters — the
same ceiling your own composer enforces (`HUB_MAX_TEXT`), so a line is bounded by the same
number you already bound the writer with. `http://` and `https://` URLs are spared whole —
including something glued to their front (`src=https://…`) — because the ring is your only
history and mangling every link an agent pasted would quietly gut it. Every other scheme
scrubs where the path inside it would. The one identifier a down `message` line may carry is
the receipt nonce, below, which names nothing on this box.

### The follow-up lines — `t:"ack"`

What a bridge said *later* about an act of his is appended beside the receipt it answers,
never folded into it — the same tense discipline your phone surface keeps when it edits a
line forward in time and never backwards:

```json
{"t":"ack","of":"choice","ask_id":"a1","option_id":"y","status":"refused","reason":"the session is busy"}
```

* `of` is which kind of act it is about — `"message"` or `"choice"`. A choice carries the
  `ask_id` and `option_id` its down line already carries, so you join it to the same question.
* **`status` absent means nobody said.** A confirm window that ran out is not an answer:
  the line carries its sentence in `reason` and no `status` at all, and the absent status is
  exactly "the session has not confirmed it took your answer". Recording that as a refusal
  would put a refusal in his history no bridge ever sent.
* `status:"refused"` carries the bridge's own sentence in `reason`.
* `status:"accepted"` appears in exactly one place: **the correction of a timeout.** When a
  bridge that had gone silent confirms his answer after all, late, the accepted line is
  appended after the never-confirmed one — the order of the lines is the explanation. An
  ordinary acceptance gets no line, because the receipt line already claimed nothing was
  confirmed and nothing needs correcting.

### The receipt nonce

A down `message` line may carry one extra field: `msg_id`, a `w…` string. That is **the
door's own nonce for your POST** — minted by the door at write time, returned to you in the
POST's ok-shape as `msg_id`, carried on the answer file as `ref`, and echoed here by the hub.
One nonce, three places, and your client's `r.msg === f.msg_id` matching (index.src.html's
down-`message` arm) turns the line you sent into its own receipt instead of a second bubble.
It names nothing on this box; a Telegram message id still never rides this file. Down
`choice` lines carry no nonce — your client joins a tap to its question by `ask_id`, and the
line already names that.

---

## 3. The seam

Three routes, nothing else. A route spoken to the wrong way round is a named `405`
(`{"ok":false,"why":"that route takes another method"}`); an unknown path is a `404` with
your stub's body, `{"error": "not found"}`. Every non-stream response carries
`Connection: close` and `Content-Type: application/json`.

### `GET /v1/stream` — the SSE

```
HTTP/1.1 200 OK
Content-Type: text/event-stream; charset=utf-8

retry: 3000
: connected

id: 41
data: {"seq":41,…}

: ping
```

* The opening is exactly `retry: 3000\n: connected\n\n` — your stub's chunking, kept because
  your test reads it.
* One `id: <seq>` + `data: <the whole ring line>` block per event, in ring order, the line
  verbatim.
* When nothing new has arrived for fifteen seconds (the default; `--ping-every-ms` moves it
  for a trial), one `: ping` comment line — proof the wire is alive, nothing a client
  should parse.
* **Replay:** open the stream with `?cursor=<seq>` and you are served everything after it
  first, then the tail. A reconnecting `EventSource` should let its `Last-Event-ID` header
  speak instead: **the header wins over the query**, on purpose — the header is the position
  the browser actually reached and the query is where the URL was cut, possibly minutes of
  events ago. A malformed cursor (either spelling) is a `400` in the refusal shape below.
* **Past the rotation window:** the ring keeps the active file and one old file, about a
  megabyte each — a few hundred events at the clip ceiling, comfortably more than a minute
  away. Ask from a cursor older than what the two files hold and there is a gap nothing here
  can fill; the door serves what it holds without renumbering, your client's coherence check
  trips, and it resyncs from the hub's own cursor echo — which is the designed mend, and the
  reason the poll's echo is always the ring's true head (below). Your `hubResync` already
  does exactly this.

### `GET /v1/events?cursor=<n>` — the poll

```json
{"ok":true,"at":1763390002,"cursor":41,"events":[ …the ring lines, verbatim… ]}
```

* Served events are **strictly the next `seq` past the cursor you asked with** — `cursor+1,
  cursor+2, …`, no gaps, no repeats — because the ring's own contiguity provides it and the
  door never renumbers. `at` is unix seconds (your stub's was a string placeholder; see §2).
* **The echo is the truth, not a mirror.** `cursor` is the last seq the ring holds — the
  head — even when nothing past your cursor was served, so a client holding a fiction
  (a cursor past the head, or a ring that restarted) learns it. That echo is what your
  `hubFetch(true)` resync re-baselines on, and what your push watcher re-anchors on.
* No cursor means zero: the whole history the two files hold.
* A malformed cursor — not digits, or more than twelve of them — is a `400`:
  `{"ok":false,"why":"bad cursor: \"banana\" is not a sequence number"}`. The same rule your
  bridge applies to its own cursor, at both hops.
* One poll drains to quiescence (it keeps reading until a pass adds nothing), so a rotation
  landing mid-answer cannot split it — and see §8 for the recorded bound on that drain.

### `POST /v1/commands` — the write door

**The gate.** `Authorization: Bearer <token>`, compared in constant time against the token
file, read **per request** so rotation is live (§5). No token minted at all — or an empty
file — is a named `503`: `{"ok":false,"why":"the hub write door is closed — the operator
has not minted a token for it"}`, the refusal shape your client already renders and your
bridge passes through as it passes everything of the hub's through. A missing header and a
wrong token are one byte-identical `401`:

```
{"error": "unauthorized"}
```

The space after the colon is yours — Python's `json.dumps` puts it there, your test compares
raw bytes, and your bridge passes it through untouched. We transcribed the bytes.

**The body** is one JSON object, §4. Then the door routes it, writes one file into the drop
(staged beside it and renamed in, never written in place — a half-written file would be read
by the hub's sweep as a malformed answer and consumed, which is the one way this door could
eat your words), and waits.

**The answer is the hub's, never optimism.** The hub sweeps the drop about once a second,
judges the answer against its own ledger, options and liveness, and writes a `.result` file
beside it. What your POST is answered with is that and nothing else:

* accepted → `200`:
  ```json
  {"ok":true,"t":"message","msg_id":"w18f…-9c1d42aa07e1","lane":"fix-17"}
  ```
  `msg_id` is the receipt nonce of §2 — hold it, and the ring's down line with the same
  `msg_id` is your echo. `lane` is the lane the act went to (the one the body named, or the
  question's), and JSON `null` when it went as the project's own voice. (Your stub spelled a
  string `"lane/main"`; nothing in your client reads it.)
* refused → `400`, `{"ok":false,"why":"…"}` with the **hub's own sentence** — plain words,
  the same ones a person reads: *"Nothing is connected for that conversation right now, so
  nothing was sent. It will not be delivered later."* Your client already renders `why`.
* no result inside **2.5 seconds** → `504`, and the words claim exactly this much and no
  more: `{"ok":false,"why":"the hub has not said what became of it yet — it may still;
  sending it again may say it twice"}`. Not a refusal, never an ok: the hub may still take
  it, and a client that re-sent on a timeout would be a client turning "slow" into "twice".
  Your stub never had to spell a hub that was slow rather than wrong; this shape is ours and
  documented as ours.
* the door could not reach the drop at all → `503` in the refusal shape.

A body that is not one readable command — not JSON, not an object, no `t`, or a `t` that is
neither `message` nor `choice` — is a `400` before anything is written.

---

## 4. The write fields

| field | on | required | law |
| --- | --- | --- | --- |
| `t` | both | yes | `"message"` or `"choice"`. Anything else: `400`, *"that did not say whether it was words or an answer"*. |
| `conversation` | both | **yes, by the hub's file law** | The conversation id (`p-`/`c-` + twelve hex). See below — the door can bridge this for you today, but we are asking you to send it. |
| `text` | message | yes | Non-empty after trimming. The door passes it through; the hub refuses empty words with its own sentence. |
| `ask_id` | choice | yes | Non-empty, at most 62 bytes, no `\|`, no control character — the same id law the buttons ride. |
| `option_id` | choice | yes | Same law as `ask_id`. |
| `lane` | both | no | A lane of that project: non-empty, ≤64 bytes, not `.`/`..`/`-`, **no `/` and no `\`**, no control character. See below. |
| `in_reply_to_ask` | message | no | Present and not a string: refused at the door, never stripped. Routes the words to the session that asked (§ below). |
| `ref` | — | — | **Not a field the door reads from you.** The door mints its own nonce for every message and carries it on the file; it is documented here because the ring echoes it (§2). A reader writing files into the drop directly may mint its own; the file law takes it optionally. |

**A known field written wrongly is refused, never stripped.** Unknown fields are ignored —
a newer client cannot break an older door — but a known field present in a shape the law
refuses (`conversation` as a number, `lane` as an array, `in_reply_to_ask` as `42`) is the
body *saying something the hub cannot believe*, and sending it on as though it had said
nothing would be the door quietly rewriting what you wrote. The refusal names the field.

**`conversation`, and the ask.** The hub's answer-file law requires the conversation: a file
without it is refused unread. Your client does not send one today — it was built against a
single-conversation hub — and the door bridges that, by a ladder with no guess on it:

1. the `conversation` the body names;
2. else the conversation this door was *started for* (`kickoff-door --conversation <id>` —
   how a dispatcher would start one for a single room);
3. else, for a command that answers a question — a `choice` naming an `ask_id`, a message
   naming an `in_reply_to_ask` — **the question itself**: the door remembers every `ask` line
   it has served and whose it was, and the ask is the one fact on the wire that says whose
   turn the answer belongs in. The same ask name open in two conversations is refused with
   directions (*"that did not say which conversation it is for…"*), never guessed between;
4. there is no fourth rung. A command that names nothing, defaults to nothing and answers
   nothing is refused, and nothing is written.

We are asking you to add the field (§7) because rungs 2 and 3 are ours to withdraw: a door
started for one conversation is a deployment choice, and the routing memory is bounded and
in-process — the field is the one addressing fact that survives all of it.

**`lane` — what it disambiguates.** A project's own voice and each of its lanes are separate
live sessions, and two of them can hold an open question under the same ask name at once.
The `lane` names which conversation of the project an act is for. On a choice routed by its
question, the ask's own lane fills the name in when you sent none. Note the spelling law:
a lane is a **bare name** — `fix-17`, not `lane/fix-17` — because `/` would make a lane a
path shape this hub refuses to address. Your client already strips the prefix on read
(`hubLaneKey`); strip it on write too, if you ever send one.

**`in_reply_to_ask`** — reply context. When he swiped to reply on a question, the hub uses
this to route the words to the session that asked, and the question stays open for the tap.
On the ring, the down `message` line echoes it. Malformed is a refusal at both doors, as
above.

---

## 5. Auth and trust

**The token.** One credential opens the write door: 64 hex characters in
`<state>/door/token`, mode `0600`, minted only at a terminal by `herdr-tg door-token` (the
canonical binary is `kickoff-channel`; `herdr-tg` is its installed alias — same verb). The
gateway **never mints one itself** — a gateway that could mint its own credential would be a
credential nobody decided to issue — and it reads the file on every request, so rotating it
is live without a restart.

Rotation refuses to overwrite silently, and is *anchored*: `herdr-tg door-token --rotate
<first characters of the old one>` refuses if the token on disk does not start with what you
named — rotating over a token that is not the one you believed you had (a second door, a
restored backup) is a decision the verb declines to make blind. The token is never printed;
it lives in the 0600 file and you read it where it lives.

**Who holds it: your bridge, server-side** — exactly the contract your `HUB_TOKEN_FILE`
already keeps. Your bridge adds the Bearer itself so the browser never sees the token; point
`HUB_TOKEN_FILE` at the door's token file and nothing about your side's credential
discipline changes. The read routes take no token at all — your bridge's session gate is the
front door for both halves, as it is today.

**Loopback only, and no flag for otherwise.** The bind call names `127.0.0.1` and nothing
else; there is no `--address` and there will not be one — a door on `0.0.0.0` is a different
product, and the egress law the ring already holds to is the reason why. Default port
**8791** (`--port` or `$KICKOFF_DOOR_PORT`); your bridge's `HUB_URL` default is 8777, so the
variable must actually be set. Your tailnet serve front door stays the only route in from
anywhere that is not this box.

**What the token can never do — structurally, not by policy:**

* **Start a process.** The door spawns nothing; it contains no process-starting code at all,
  and the workspace guard `tests/nothing_inbound_can_start_a_process.rs` holds the property
  this repo is named for: a fixed program name at every allowed site, a program built from a
  value refused even there. There is no allowed site in the door.
* **Name a path.** Nothing in a request reaches a path: the door's only file reads are the
  token, the ring pair and the drop directory, and the one name it writes into the drop is
  minted by the door, never built from your bytes. What leaves is guarded too — the door adds
  nothing to the ring's already-scrubbed lines, no header carries an identifier, no error
  body names a file, and two tests hold the bytes themselves:
  `nothing_the_door_says_over_http_names_this_machine` (unit, in `gateway.rs`) and the
  hermetic trial (§7), which asserts on the **raw bytes both directions** that the state
  home, `/tmp/`, a chat id, a bridge secret and the door's own token never cross the wire.
* **Read a secret.** The drop's own laws hold the inbound half: ids are shape-refused (a
  control character in an id would forge a line in the ring), a symlink in the drop is
  refused and unlinked, never opened, and every file is bounded (a body past ~32 KiB is
  refused unread). The token opens a write door into conversations, nothing wider.

**One token, one operator.** This hub serves one operator; the token is his steering wheel,
not a person-gate. Your bridge's session gate is what decides which human is at the other
end — keep it, as your tests already insist.

---

## 6. What is deliberately not offered

* **Files and media.** The write door carries words only — no attachments, no media, no file
  handles. A message with no words is refused; there is nothing else a message could be here.
* **Remote binds.** Loopback, forever (§5). If your bridge and the hub are ever on different
  machines, the seam between them is yours to build and yours to defend — nothing in this
  binary will help or hinder it.
* **Spawn and lifecycle.** The door cannot start, stop or restart anything; the launcher
  question (who starts the engine a wall runs) is still open in `docs/CAPABILITIES.md` and is
  not this seam's to answer. A write can steer a *live* session; a dead one is the hub's
  refusal to read, not a request to respawn.
* **Multi-operator.** One token, one operator (§5). Your surface's people model is yours;
  this seam has no concept of a second person.
* **Any Telegram path.** By ruling of 17 September 2026, Telegram is a connector that
  arrives later, as kickoff work, driven by the kickoff program — not grown here. This door
  is the work surface's seam; the phone surface gets no new investment from this repo and
  nothing through this door reaches it.

---

## 7. Adoption, from your side

1. **Run the door beside the hub.** `kickoff-door` finds the state home the same way every
   part of this product does; `--state <path>` names it outright. It prints one line on
   stdout — `kickoff-door: listening on http://127.0.0.1:<port>` — and nothing after.
2. **Mint the token at the terminal** (`herdr-tg door-token`), and point your bridge's
   `HUB_TOKEN_FILE` at it. Your bridge's own closed-door 503 covers its file being unset,
   unreadable or empty; the door's named 503 covers the same family on our side, and both
   speak the refusal shape your client renders.
3. **Point `HUB_URL` at the door** — `http://127.0.0.1:8791` unless the port was moved. Your
   default (8777) is the old hub's; the variable must be set.
4. **Add `conversation` to your writes.** Your composer and ask-buttons send
   `{t,text}` and `{t,ask_id,option_id}`; send the conversation the pane is looking at, from
   the envelope's own `conversation` field. Until then the door bridges by its ladder (§4) —
   but the field is the addressing fact that survives our deployment choices, and a fleet of
   many conversations is the thing this hub is.
5. **Map conversations to projects for push attribution.** Every envelope stamps
   `conversation`; we send nothing that names a project — no path, no repo name, ever. Your
   push watcher's law ("attribution may not be guessed", a lane nothing pins is skipped)
   stands; the map from our ids to your project display is yours to keep, the way your
   lane→project pinning already is. The ids are stable for a conversation's life.
6. **Hold your own `test_hub.py` expectations against the door** — the shapes that suite
   pins are the shapes this door serves, so its assertions are the checklist: the
   byte-identical 401, the 404 body, the poll envelope with strict next-seq and a truthful
   echo, the stream's opening and per-event framing, the ok-shape's `ok`/`t`/`msg_id`/`lane`,
   the refusal shape with `why`. (The suite itself runs a stub that emits its own two events
   and inspects what reached it; the door is not that stub, so this is a comparison of pinned
   shapes, not a re-run of the file.) Named deltas, all of them already written down above:
   `ts` and `at` are numbers here; `lane` in the ok-shape is the bare lane or `null`; and the
   2.5-second result window means the 200 you get is the hub's verdict, not the instant
   optimism your stub practiced.
7. **Or run the smoke harness first:** `bash scripts/pwa-door-trial.sh` — this repo's
   hermetic trial. One real hub over a real socket with Telegram counted instead of called,
   and the **real gateway binary** on a loopback port, driven by a client scripted exactly
   like yours (anchor poll, SSE subscribe, write POSTs). It proves the whole loop — an ask
   envelope arriving stamped with its conversation and lane, a POSTed choice reaching the
   lane's session and nobody else's, the POST answered from the hub's own result, a refusing
   bridge's word landing on the ring as a follow-up line, the nonce's three places agreeing,
   the byte-identical 401, exact replay from an old cursor, and nothing on the raw wire
   naming the machine. Nothing is installed, nothing is sent, nothing is spent; it refuses
   to run anywhere near a real state home.

---

## 8. Honest limits

Shapes recorded rather than fixed, each with its reason:

* **The rotation race.** More than two ring rotations landing inside one 250 ms poll gap
  lose the middle old file entirely, and the served sequence gains a gap nothing here can
  fill. Contrived by ruling: a rotation is a megabyte of operator-visible events, and human
  cadence cannot spend three megabytes inside 250 ms. The client that meets the gap resyncs
  from the hub's own cursor echo by design — which is the honest mend for a gap nobody
  caused.
* **No read timeouts on the door's own sockets.** A request head, a body, and every write
  on a stream wait for the peer for ever. Contrived by ruling: the one client is your
  bridge, and it already bounds every upstream call (10 s on the poll, 30 s on the stream),
  so a wedged door task is bounded by your deployment's own timeouts. The day the door
  faces a peer that is not that bridge is the day this becomes a decision rather than a
  settlement.
* **The unbounded poll drain.** One `/v1/events` drains to quiescence, and a writer of the
  same uid appending steadily can keep one going for as long as it likes. The same trust
  settlement the drop itself records: the writer is already this user, inside the state
  home, and a bound here would punish a healthy hub's burst to spite a misbehaving writer
  the drop's own laws have already accepted.
* **The hub's result writes are not atomic yet.** The hub writes a `.result` open-truncate-
  then-write, and the door polls every 50 ms — so the door can read half a receipt. It waits
  torn reads out rather than answering them (a half JSON object is a receipt nobody has
  finished, and behind the torn bytes the answer was accepted); the mend on the hub side —
  stage and rename, exactly as the door already stages its own answer files — is a named
  follow-up, not done.
* **One door per state home.** Nothing enforces it and nothing coordinates two: each door's
  routing memory (§4 rung 3) is its own, learned from the lines it has served. Two doors on
  one state home means a tap can be refused "did not say which conversation" by the door
  that never served the ask its sibling did. Run one.
* **Nothing of yours has talked to this binary yet.** The hermetic trial proves the seam
  with a scripted client, not your browser, your EventSource's reconnect behaviour, or your
  bridge's forwarding — your `test_hub.py` holds your half against a stub standing where
  this binary stands, and the two halves have not met. That meeting is what adoption is.
