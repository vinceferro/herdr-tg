<!-- CONTRACT, 19 September 2026. Written against the WORKING TREE standing above HEAD 54499fc —
     the app plane of the hub and the trial that holds it are in that tree and not committed
     themselves yet, so the hash alone will not hand you this code. Where this file and the code
     at that hash disagree, the tree is the newer of the two and this file follows the tree; once
     it is committed, the commit carrying this paragraph is the one to read. Every route, body,
     status code
     and sentence below was read in the code that serves it — crates/herdr-tg/src/gateway.rs (the
     door), crates/herdr-tg/src/hub/door.rs (the ring), crates/herdr-tg/src/hub/answers.rs (the
     drop), crates/herdr-tg/src/surface.rs (the surface a hub with no phone line is built with)
     and crates/herdr-tg/src/hub/tests.rs (the trials) — and every behaviour named "yours" was
     read in your own repository: your pinned stub suite, your bridge, your push watcher and
     your board. **The precise citations into your tree — which file, which line, and the names
     inside them — are deliberately not published here; this file is public and that code is
     yours, so the citations travel by the letter our two organisations exchange, which lives
     outside this repo and is never committed to it.** Where this file and the code disagree, the
     code wins and we fix the file; nothing here is checked against a second copy of itself.

     This is the door's half of an agreement whose other half your own tests hold: the shapes
     are transcribed from the stub your own suite pins rather than designed, because a
     client written against the stub working against the real thing is the whole cost advantage
     of adopting it. docs/ATTACHING.md is the contract for an ADAPTER — a process holding the
     claim, speaking hub-proto on a socket. This file is the contract for a READER — a process
     that never touches a socket, holds no claim, and speaks HTTP on loopback. -->

# The PWA's door — `kickoff-door`, and the `/v1` seam it serves

You are an engineer on the surface side. Your PWA already speaks a `/v1` seam — a stream, a
poll and a write door — through your own bridge (the server-side process that proxies the
three routes and holds the token so the browser never does), and that seam's shapes are pinned
by your own tests against a stub. This document describes the real binary that stands where
your stub stood: `kickoff-door`, the one program in this workspace that listens. It is written to
be implemented against by a stranger who has never opened this repo — every route, every
body, every refusal, and the laws underneath them.

Nothing of yours needs to be installed from here. The door serves three routes on loopback;
your bridge already proxies exactly those three.

**The hub behind this door can now run with your app as its only surface.** It began as a
Telegram bot that also wrote the ring; on 17 September the operator revoked its bot token, and a
hub started the other way holds no credential, dials nothing off the box, and has the ring and
the drop as the only places his half of a conversation lives. **Nothing on this seam changes
because of it** — same routes, same envelope, same cursor discipline, same refusals, byte for
byte. Which surface the hub was built with is a fact about the hub; this door reads the same two
files either way and cannot tell which plane wrote them. The one difference you can observe is
an absence, and §2 names it: where there is no phone line, every line he says comes in through
this door, so you will never meet a down `message` line named with a `p…` instead of the `w…`
your own POST was answered with. A client that already matches on that name needs no change to
notice either way. It is held by a trial of its own — the second round trip in §7 step 8's
harness, which stands the real binary in front of a hub with no Telegram surface in the process
at all.

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
| `ts` | Unix seconds, a number, stamped where the hub first handled the frame. (Your stub put a string placeholder here; nothing in your client reads `ts`, and ours is a number — say so now so nobody pins the stub's shape by accident.) |
| `dir` | `"up"` is the agent's half — its `say`, `ask`, `done`, a question stopping being open, and the hub's own follow-ups about your acts (below). `"down"` is the operator's half — his typed words and his taps, stamped **only where the frame was handed to a live session**. A down line is a receipt: it never appears for words that reached nobody. What became of a delivered act *later* is an up-line follow-up, never an edit of the receipt. |
| `conversation` | The id of the conversation this event belongs to: `p-` or `c-` then twelve lower-case hex characters, exactly fourteen characters. Not a path, not a name — the shape is refused at every door here, and the id is the only project-identifying thing the ring ever carries (§5). |
| `lane` | Which conversation *of that project* said it: the lane's own address (a bare name — see the lane law in §4), or the single character `-`, which is the spelling for **the project's own voice**. **Two deltas against your side, and both are live today.** (i) `-` is not the empty key your lane-keying folds into the fleet's own row: it folds only a falsy value into that row, and `"-"` is a perfectly good string, so it comes back out as `-` and earns a lane row of its own called `-`. (ii) It never reaches that keying anyway — both of your readers take the lane off the **frame**, not the envelope: the live board makes its lane key out of the frame's lane, and the push watcher asks which project a lane pins to with a lane it read off the frame too — that second reader your reply never mentioned. Our lane is on the envelope beside `frame` and never inside it, because on this wire a lane is a property of the connection that spoke and not of the thing it said. So the lane they both read is undefined on every line we serve: the board files every one of our events under the project's own voice, and the push watcher's own law — a lane nothing pins to a project is skipped, never mislabelled — skips **every ask we send**, which means no ask of ours produces a push today. Read the lane off the envelope instead, map `-` to your empty key yourself, and both are mended. |
| `frame` | The event itself, in the wire's own vocabulary. Up frames: `{"t":"say","text":…}`, `{"t":"ask","ask_id":…,"text":…,"options":[{"option_id":…,"label":…}]}` (options absent means free-text), `{"t":"done","text":…}`, `{"t":"ask_resolved","ask_id":…,"how":…}` — `how` is `"answered"`, `"withdrawn"` or `"timeout"` when a bridge said it, or a plain sentence when the hub itself put a dead session's question away — and when the question had been answered, that sentence names the surface the answer came from: `answered from your phone — …` or `answered from the app — …`, never the wrong one. The hub's own record of a tap says where the tap was made; a retirement that had to guess has credited the phone for the app's answer, on the phone and in this history at once. Down frames: `{"t":"message","text":…,"msg_id":…}` — **every** message line he says carries a
`msg_id`, see the names below — and `{"t":"choice","ask_id":…,"option_id":…}`. Nothing else is recorded — the `hello` that carries the token never reaches this file, nor any `ack`/`bye`/`beat` bookkeeping except the follow-up lines below. |

**The egress law, which is the whole design.** This file is built to leave the machine. It
carries no chat id, no topic id, no user id, no Telegram message id, no filesystem path and
no token. Every string in every frame is scrubbed and clipped in one pass: absolute and
home-relative paths become `[a path]`, and everything is clipped at 3500 characters — the
same ceiling your own composer enforces (`HUB_MAX_TEXT`), so a line is bounded by the same
number you already bound the writer with. `http://` and `https://` URLs are spared whole —
including something glued to their front (`src=https://…`) — because the ring is your only
history and mangling every link an agent pasted would quietly gut it. Every other scheme
scrubs where the path inside it would. The only identifier a down `message` line ever carries is
its own name — the receipt nonce when the door sent the line, the hub's own `p…` mint when the
phone did — and names of that kind name nothing on this box.

### What the ring may carry, and what to do with a line you cannot read

The write door has a forward-compatibility law and §4 states it: a field you send that we do
not know is ignored, a field we know written wrongly is refused. The read direction needs one
too, and it points the other way, because here **we** are the side that may add.

**Skip what you cannot read, and keep your place.** A `frame.t` outside the list below, an
unknown field inside a frame you do know, a `dir` you have never seen — ignore that line,
advance your cursor past it, and carry on. It is not a gap and it is not corruption; it is
this hub having learned a word after your client was written. What you must never do is stop
the stream, resync, or treat it as a hole: the `seq` is contiguous by construction, so a line
you skipped is a line you were served, and a client that resyncs on a line it did not
understand will resync on that same line for ever.

The closed list, as the code serves it today:

| `dir` | `frame.t` | what it is, and what it carries |
| --- | --- | --- |
| `up` | `say` | the agent talking. `text`. |
| `up` | `ask` | a question opening. `ask_id`, `text`, and `options` when there are buttons — `options` absent means it wants free text. |
| `up` | `done` | the agent's turn ending. `text`. |
| `up` | `ask_resolved` | a question stopping being open. `ask_id`, `how`. |
| `up` | `ack` | the follow-up half below — what a bridge said *later* about an act of his. `of`, `reason`, sometimes `status`, and the join its kind carries. |
| `down` | `message` | his typed words, at the moment they were handed to a live session. `text`, `msg_id`, and `in_reply_to_ask` when he typed under a question. |
| `down` | `choice` | his tap, at the same moment. `ask_id`, `option_id`. |

Seven kinds, and nothing else reaches this file — not the `hello` that carries the token, not
a `bye`, not a heartbeat, none of the wire's own bookkeeping. An eighth would arrive exactly
the way the law above describes: appended, numbered in sequence, and harmlessly unread by a
client that has never heard of it.

### The follow-up lines — `t:"ack"`

What a bridge said *later* about an act of his is appended beside the receipt it answers,
never folded into it — the same tense discipline your phone surface keeps when it edits a
line forward in time and never backwards:

```json
{"t":"ack","of":"choice","ask_id":"a1","option_id":"y","status":"refused","reason":"the session is busy"}
```

* `of` is which kind of act it is about — `"message"` or `"choice"`. A choice carries the
  `ask_id` and `option_id` its down line already carries, so you join it to the same question.
* **A follow-up about his words names the line it is about**, in `msg_id` — the same name that
  line's own down `message` carried. So a refusal is drawn under the sentence it refuses and
  not under the last sentence you happened to draw. Your composer keeps one send in flight and
  got away without this; that is a property of your composer, not of this history, and a second
  line in flight is all it would take. The name is absent in exactly one case: words that
  reached the ring under a name nobody outside the hub has ever seen, which is a file written
  into the drop with no `ref` on it — never a write of yours, since the door mints a nonce for
  every one. Absent means *unplaceable*, and an unplaceable refusal saying so is honest where an
  invented name would join to nothing while looking like it joined to something; show it against
  the conversation rather than against a row. (Words he typed on his **phone** produce no `ack`
  line here at all. The hub answers those where he typed them, with a mark on his own message,
  and this file is not a record of the phone's plumbing.)
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

### The names every line he says carries

Every down `message` line carries a `msg_id`, with no exceptions:

* **yours, when you sent the line** — a `w…` string, **the door's own nonce for your POST**:
  minted by the door at write time, returned to you in the POST's ok-shape as `msg_id`,
  carried on the answer file as `ref`, and echoed here by the hub. One nonce, three places,
  and the match your board already makes in its down-`message` arm — the row still waiting on a
  send, whose held name equals the line's `msg_id` — turns the line you sent into its own
  receipt instead of a second bubble.
* **the hub's, when the phone sent the line** — a `p…` string the ring mints for any message
  line that arrives with no nonce. Your matcher is the reason: it takes the first row whose held
  name equals the line's, and against a nameless line that is any row whose name is not yet set
  — an optimistic row mid-POST — so the second phone-typed line your client ever read would have
  been marked as a line it never sent. The `p…` names the line's own `seq` and nothing on this
  box. **Where there is no phone line there is no such line to meet**: every line he says
  arrives through this door and wears the `w…` it was answered with. That absence is a property
  of the box, not of this seam — your matcher handles both and needs to know nothing about
  which one it is reading.

Either way the name joins a line to a row and nothing else; a Telegram message id never rides
this file. Down `choice` lines carry no name of this kind — your client joins a tap to its
question by `ask_id`, and the line already names that. (A choice's **ok-shape** still carries
a `msg_id` like every ok-shape — the same opaque string a message's carries, minted the same
way. What a choice's does *not* do is go anywhere else: a message's name is written onto the
answer file as `ref` and ridden from there onto the ring, and a choice's is written onto no
file at all, so the hub never sees it and nothing ever echoes it back. Nothing of your
client's reads it, and there is nothing it could join to if it did. §3's `504` is where that
difference stops being a curiosity.)

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
  megabyte each — roughly six hundred events between them at the 3500-character clip
  ceiling, which at the hub's own send ceiling (eighteen messages a minute for the whole
  chat) is on the order of half an hour of flat-out talking, and far longer at any human
  cadence. That is arithmetic from the constants, not a measurement — no test here pins how
  long your fleet takes to spend a megabyte. Ask from a cursor older than what the two files
  hold and there is a gap nothing here
  can fill; the door serves what it holds without renumbering, your client's coherence check
  trips, and it resyncs from the hub's own cursor echo — which is the designed mend, and the
  reason the poll's echo is always the ring's true head (below). Your own resync path — the
  one your coherence check trips into — already does exactly this.

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
  forced resync fetch re-baselines on, and what your push watcher re-anchors on.
* No cursor means zero: the whole history the two files hold.
* A malformed cursor — not digits, or more than twelve of them — is a `400`:
  `{"ok":false,"why":"that asked to carry on from a place in the events this door could not
  read"}`, the same sentence on the stream and on the poll. The same rule your bridge applies
  to its own cursor, at both hops. The sentence names neither "cursor" nor "sequence number"
  — those are this door's words for the ring's insides — and it does not read your text back
  at you, because `why` is rendered straight onto a person's screen and a person told what he
  already typed learns nothing. The offending text goes to the door's journal at **warn** — the
  level a door started with nothing set in its environment writes at, so it is on the stderr of
  an ordinary run — and that line is the only copy of it anywhere: both of your read callers
  throw on a failed fetch without ever reading its body — the error they raise names the
  status and nothing out of the body — and an `EventSource` never exposes one at all. That
  journal line is where whoever is wiring a client should look for it.
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

The space after the colon is yours — your serialiser puts it there, your test compares
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
  {"ok":true,"t":"message","msg_id":"w18f…-9c1d42aa07e1","lane":"fix-17","conversation":"p-0123456789ab"}
  ```
  `msg_id` is the receipt nonce of §2 — hold it, and the ring's down line with the same `msg_id`
  is your echo. `lane` is the lane the act went to (the one the body named, or the question's),
  and JSON `null` when it went as the project's own voice. (Your stub spelled a string with the
  `lane/` prefix still on it; nothing in your client reads it.) `conversation` is where the act
  actually landed — always a real id, never null, because the door writes nothing at all until
  the ladder in §4 has named one. It is here so that a client which named no conversation can
  find out what it was routed to instead of guessing; and until a client can find that out,
  neither guessing rung of that ladder could ever be withdrawn without breaking it. Both fields
  are safe to keep and safe to show — an id names nothing on this box (§5).
* refused → `400`, `{"ok":false,"why":"…"}` with the **hub's own sentence** — plain words,
  the same ones a person reads: *"Nothing is connected for that conversation right now, so
  nothing was sent. It will not be delivered later."* Your client already renders `why`.
* no result inside **2.5 seconds** → `504`, and the words claim exactly this much and no
  more. A message's:
  ```json
  {"ok":false,"why":"the hub has not said what became of it yet — it may still; sending it again may say it twice","msg_id":"w18f…-9c1d42aa07e1","conversation":"p-0123456789ab"}
  ```
  A tap's is the same shape **with no `msg_id` on it**:
  ```json
  {"ok":false,"why":"the hub has not said what became of it yet — it may still; sending it again may say it twice","conversation":"p-0123456789ab"}
  ```
  Not a refusal, never an ok: the hub may still take it, and a client that re-sent on a
  timeout would be a client turning "slow" into "twice". **The conversation rides every one of
  these**, and it is why the shape is worth handling rather than merely logging: a write the
  hub honours late lands somewhere, and this is the only place a client that named no
  conversation is ever told where. **The name rides a message's and only a message's**, and it
  is the join: a message the hub honours late puts a line on the ring carrying that same name,
  no other program on this box can mint it, and a client that threw it away draws a second
  bubble beside its own failed one — so he reads his own sentence twice.
* **A timed-out tap is not named, and must not be waited on by name.** A tap's name is minted
  and then goes nowhere — written onto no answer file, so the hub never sees it (§2) — and the
  echo it would have to match against has no name either: the ring's down `choice` line carries
  none at all, and the hub mints its own for whatever the tap becomes. So a name here would be
  a string to hold for a match that cannot ever happen, held for ever. There is nothing missing:
  you already join a tap to its question by `ask_id`, and the tap knows its own.
* `ok:false` and the sentence are the same on both and will stay the same; go on reading this
  shape as a failure, and keep whichever fields it carries as a join, not a verdict. Your stub
  never had to spell a hub that was slow rather than wrong; this shape is ours and documented
  as ours — and §7.7 is what it costs your client as it stands today.
* **The `400` refusal carries none of those fields**, on purpose. A refusal means no ring line
  will ever appear under any name, so there is nothing for a client to join and nothing worth
  holding.
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
nothing would be the door quietly rewriting what you wrote. **The refusal says which part of
the body was wrong in the words a person would use for it** — *"that named the conversation it
is for as something other than words"*, *"that named the question it answers as something other
than words"* — and never in the field's own spelling. `why` is rendered straight onto his
screen by your client, and a field name is our vocabulary, not his; `in_reply_to_ask` was a
word on the operator's screen until this was fixed. So do not pattern-match a refusal to work
out which field it means: these sentences are written for a person and get reworded whenever a
plainer one is found. The `400`, and `ok:false` with a `why` in it, are the stable part.

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
The `lane` names which conversation of the project an act is for. Note the spelling law:
a lane is a **bare name** — `fix-17`, not `lane/fix-17` — because `/` would make a lane a
path shape this hub refuses to address. Your client already strips that prefix on read, in
the one helper that makes its lane key; strip it on write too, if you ever send one.

**When the door fills a missing lane in for you, exactly.** A command that answers a question —
a `choice` naming an `ask_id`, a message naming an `in_reply_to_ask` — and sent with no `lane`
takes the lane of the question it answers, on two of the ladder's three rungs:

* **Rung 1, the body names its conversation.** The question has to be one **that same
  conversation** is asking, and the only one open there under that name. A question open next
  door under the same name is no evidence about this conversation and is never borrowed; two
  open under it here is an ambiguity the door will not resolve. In either case nothing is
  filled in and the act goes exactly as you addressed it — from there it is the hub's business
  whether it can be taken at all, and where it cannot, the hub says why in its own sentence.
  This rung matters more than it sounds: the moment your composer starts sending
  `conversation` (§7.4) every write moves onto it, and without the fill that one new field
  would have taken the lane away from every tap that used to get it for free — a tap that
  landed yesterday refused tomorrow, for saying *more* about where it belonged.
* **Rung 3, nothing names the conversation and the question does.** The lane arrives with the
  conversation, off the same remembered `ask` line. Here the name must be open in exactly one
  conversation anywhere this door remembers — the same test that rung already applies before it
  will route at all.
* **Rung 2, the door was started for a conversation, fills nothing.** A door started with
  `--conversation` was told where writes go and told nothing whatever about lanes, so it
  addresses that conversation's own voice unless you name a lane yourself. If you run a door
  that way and your panes are per-lane, send the lane.

A `lane` you send always wins over the one a question would have supplied. The question says
where the answer belongs; a body that named a conversation of the project meant that one.

**One asymmetry to know about, because it is three spellings of a single idea.** "No lane" is
written three different ways across this seam, and each is right where it stands:

| where | "no lane" is spelled | why there |
| --- | --- | --- |
| the ring envelope, which you read | the string `-` | every line carries the field, and a one-character sentinel keeps the shape of the JSON identical for a lane and for the project's own voice. |
| the POST's ok-shape, which you read | JSON `null` | it is a receipt about one act, and the absence of a lane is the absence of a value. |
| the write body, which you send | **refused** | `-` is not an addressable lane name here, and neither is `.`, `..`, anything holding a `/` or a `\`, anything over 64 bytes, or anything with a control character in it. To address the project's own voice, **leave the field out**; sending `"-"` is a `400`. |

So a client that reads a lane off the ring and echoes it back on a write has to map `-` to
"omit", not pass it through. Unifying the ring on `null` would remove the asymmetry and is
worth doing one day. It is named here as an optional follow-up and promised for no date,
because it changes bytes your client already reads.

**`in_reply_to_ask`** — reply context. When he swiped to reply on a question, the hub uses
this to route the words to the session that asked, and the question stays open for the tap.
On the ring, the down `message` line echoes it, so a reply and the question it was typed under
stay together in the one history. Present and not a string is a refusal at both doors, in the
plain sentence above — never silently dropped.

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

**A port the door cannot read stops the door.** `$KICKOFF_DOOR_PORT` holding something that is
not a port number no longer falls back to 8791 in silence: the door refuses to start, says one
line naming the setting, and exits without binding anything. Silence was the worst shape this
program had — nobody is told, the door comes up on an address nobody chose, and whatever was
pointed at it talks to nobody for as long as it takes somebody to notice. **Set to nothing at
all counts as set**, and gets a line of its own, because `KICKOFF_DOOR_PORT=` is exactly what a
unit file or a template renders when the substitution it was written with never happened — and
nobody clears a line to ask for a default they would get by deleting it. **Zero is the same
mistake wearing a number**: it parses like any other value, and a door opened on it comes up on
whatever port happens to be free — the address nobody chose, reached by the one value that gets
past a parser rather than failing it. It is refused as well, in a line that says to name the
port you mean or to leave the variable unset. Whitespace around a real port is forgiven. The
`--port` flag wins without the variable being looked at, because a setting nothing is about to
open on cannot send anybody anywhere — which is also why the flag may say zero where the
variable may not: whoever typed the flag is standing at the line that prints where the door came
up, and a variable is rendered into a unit file by something that will never read that line.

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

1. **Run the door beside the hub — and know that nothing here runs it for you.**
   `kickoff-door` finds the state home the same way every part of this product does;
   `--state <path>` names it outright. It prints one line on stdout —
   `kickoff-door: listening on http://127.0.0.1:<port>` — and nothing after. Now the honest
   part, because you will meet it on day one: **there is no unit file for this binary and no
   installer puts it anywhere.** The hub ships a systemd unit and a watchdog; the door ships
   neither, the hub's unit does not start it and does not know it exists, and the only thing in
   this repo that builds and runs it is the hermetic trial in step 8. So today it is started by
   hand, or by whatever you write to start it — `cargo build -p kickoff-channel --bin
   kickoff-door`, then run what that produces. Nothing about the seam depends on how it was
   started: the hub and the door share two files and no process, either can be restarted
   without the other noticing, and a door that is not running is a bridge that cannot reach it
   rather than a hub that has lost anything. Writing the unit is adoption work somebody has to
   do, and this document would rather say so than let you find it out from a dead port.
2. **Mint the token at the terminal** (`herdr-tg door-token`), and point your bridge's
   `HUB_TOKEN_FILE` at it. Your bridge's own closed-door 503 covers its file being unset,
   unreadable or empty; the door's named 503 covers the same family on our side, and both
   speak the refusal shape your client renders.
3. **Point `HUB_URL` at the door** — `http://127.0.0.1:8791` unless the port was moved. Your
   default (8777) is the old hub's; the variable must be set.
4. **Add `conversation` AND `lane` to your writes.** Your composer and ask-buttons send
   `{t,text}` and `{t,ask_id,option_id}` — no conversation and no lane on either. Send both,
   from the pane he is actually looking at: the conversation off the envelope's own field, and
   the lane off the detail pane he has open, bare, with your `lane/` prefix already stripped.
   **The §2 envelope fix is a prerequisite for both halves, so do it first, in the same change.**
   Neither source exists on your side yet. Nothing in your envelope handler reads the
   conversation off a line at all, so nothing you hold today knows which conversation a pane is
   showing; and the pane's lane is the empty string in every pane there is, because your lane
   slot is looked up by a key made from the *frame's* lane — the field §2 establishes is
   undefined on every one of our events — so the slot's key is `""`, the lane your pane markup
   carries in its attribute is `""`, and the lane your detail pane believes it is open on is
   `""`. Read the conversation and the lane off the envelope, map `-` to your empty key, carry
   both on the lane slot, and you then have something to send. Do it the other way round — reach
   for the conversation first, since it is the field this document keeps asking for, and take
   whatever the pane holds for a lane — and you ship exactly the mis-address the next paragraph
   warns about. An empty string is not a lane on this wire: send it and every write is a `400`
   (§4's lane law is non-empty), drop it as falsy and every write names a conversation and no
   lane, which is the project's own voice. There is no third thing an empty pane can do.
   **Both or neither**, and this is the trap: the conversation on its own is worse than nothing
   for the composer. A body naming a conversation and no lane addresses the project's *own
   voice*, which is a different live session from every lane of it — so every line he typed
   while looking at a lane would go to the voice, and a conversation whose only connected
   session is that lane refuses it outright. The door covers your ask-buttons for you (a tap
   naming its conversation takes the lane of the question it answers, so long as that is the
   only question open under its name in that conversation — §4), but nothing can cover a fresh
   line of words: a line he has just typed answers no question, so
   there is nothing for the door to read a lane off. Until you send them the ladder (§4)
   carries you — but these two fields are the addressing facts that survive our deployment
   choices, and a fleet of many conversations is the thing this hub is.
5. **Map conversations to projects for push attribution.** Every envelope stamps
   `conversation`; we send nothing that names a project — no path, no repo name, ever. Your
   push watcher's law — a push is attributed only to what pins it, never guessed at, and a lane
   nothing pins is skipped rather than credited to the wrong project — stands; the map from our
   ids to your project display is yours to keep, the way your lane→project pinning already is.
   The ids are stable for as long as the enrolment lives —
minted at enrolment and never rewritten while the row stands. What a re-enrolment does to
them is not pinned by any test here, so treat re-enrolling as the moment an old id may stop
resolving and re-read `projects --json` rather than caching forever.
6. **Key your ask maps on the conversation as well as the id.** Three of your structures are
   keyed on `ask_id` alone — the board's map of the questions it has open, the tap handler that
   reads that map back, and the push watcher's record of which asks it has already pushed. That
   is safe against a hub with one conversation in it and unsafe against this one: an ask name is
   minted **per bridge**, off a counter that starts over with every session, so the first
   question of every conversation on this box is called `a1`. Two rooms asking at once give you
   one `a1` overwriting the other's row, a tap answering whichever the map happens to be
   holding, and a push for the first with silence for the second. The mend is the pair
   `(conversation, ask_id)` as the key, in all three. Our side fails closed at the matching seam
   — a tap whose name is open in two conversations is refused with directions rather than
   guessed between (§4 rung 3) — but be clear about what that costs him: a refusal he cannot act
   on, because the one thing that would resolve it is a field his app never sent. Naming the
   conversation on the write (step 4) and keying on the pair here are the same fix from its two
   ends.
7. **Hold your own stub suite's expectations against the door** — the shapes that suite
   pins are the shapes this door serves, so its assertions are the checklist: the
   byte-identical 401, the 404 body, the poll envelope with strict next-seq and a truthful
   echo, the stream's opening and per-event framing, the ok-shape's `ok`/`t`/`msg_id`/`lane`,
   the refusal shape with `why`. (The suite itself runs a stub that emits its own two events
   and inspects what reached it; the door is not that stub, so this is a comparison of pinned
   shapes, not a re-run of the file.) Named deltas, all of them already written down above:
   `ts` and `at` are numbers here; `lane` in the ok-shape is the bare lane or `null`; the
   ok-shape carries a `conversation` your stub never spelled; and the 2.5-second result window
   means the 200 you get is the hub's verdict, not the instant optimism your stub practiced.
   One more, and it is the delta most likely to bite a client written against the stub: **a
   message's 504 names the message.** It carries `msg_id` and `conversation` beside `ok:false`
   and its sentence; a tap's carries `conversation` and no name at all, for the reason in §3. A
   client that looks only at `ok` throws them away — then draws a second bubble when the hub
   honours that write late and its ring line turns up under a name the client no longer holds.
   Take the name off a message's 504, keep it, and match the echo against it exactly as you do
   for a 200. Your suite's write assertions are predicates rather than whole-body comparisons,
   so none of these extra fields breaks it; only the 401 is compared byte for byte, and that
   body is untouched.

   **What a 504 does to your client as it stands, which is more than a missed join.** Your
   composer's send has one unhappy arm and reaches it on either half of a single test — the
   response not being ok, or the body not carrying `ok:true` — and a 504 is both halves of that
   at once, so it takes the refusal arm. The row goes to its failed state and draws your failed
   tick, which tells him the line did not go and that we are the ones who refused it, and your
   error line puts your refusal prefix in front of our sentence — *"the hub has not said what
   became of it yet — it may still; sending it again may say it twice."* One line tells him we
   refused it and the next tells him it may yet go; the door went to some trouble to say neither
   "refused" nor "ok", and that arm makes it say both. Your tap path goes
   further: on that same branch it unlocks the row, so the buttons come back and he is invited
   to make the second tap the very sentence beside them warns him about. **The mend is one
   branch, not a rewrite**: test for the `504` status before that arm and give it a third state
   — leave the row sending, leave the buttons locked, say something like *"not answered yet"* —
   because `ok` has two values and this seam has three outcomes. That mismatch is ours to have
   documented and yours to spend ten minutes on; everything else in step 7 is shapes, and this
   one is his screen.
8. **Or run the smoke harness first:** `bash scripts/pwa-door-trial.sh` — this repo's
   hermetic trial. A real hub over a real socket and the **real gateway binary** on a
   loopback port, driven by a client scripted exactly like yours (anchor poll, SSE
   subscribe, write POSTs). **Two round trips run, one per plane**, and both must pass.

   With Telegram counted instead of called, it proves the whole loop — an ask envelope
   arriving stamped with its conversation and lane, a POSTed choice reaching the lane's
   session and nobody else's, the POST answered from the hub's own result, a refusing
   bridge's word landing on the ring as a follow-up line, the nonce's three places agreeing,
   the byte-identical 401, exact replay from an old cursor, and nothing on the raw wire
   naming the machine.

   With **no Telegram surface in the process at all** — the plane the box runs on now — it
   proves the same seam against the hub you will actually be served by: the question on the
   ring stamped with its conversation and its lane, `GET /v1/events` serving it, a POSTed
   choice reaching the session that asked and nobody else, the POST answered from the hub's
   verdict rather than optimistically (the sentence you are refused with is the hub's own,
   matched against what it wrote in the drop), the ok-shape's `msg_id` echoed on the ring's
   down line, the question ceasing to be open with the hub's own retirement beside it, and
   the same wire law. Nothing is installed, nothing is sent, nothing is spent; it refuses to
   run anywhere near a real state home.

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
* **The door still tolerates a hub that writes a receipt in place.** The hub in this repo no
  longer does one: a result's bytes go down under a staging name inside the drop and are
  renamed onto `<name>.result`, so what the door finds at that name is either the whole receipt
  or nothing at all. The tolerance stays, and it is not dead code. The two files are the entire
  interface between the hub and the door (§1), which makes the writer at the far end of them a
  **seam** — an older hub still installed on the box, a hub somebody else builds, a later
  reader implementing against §5's laws rather than against this binary — and any of those may
  still open-truncate-then-write while this door polls every 50 ms. So the door goes on
  treating a `.result` that is not yet a whole verdict as a result that has not arrived: it
  waits, and the 2.5-second window closes with the 504 and nothing else. Answering torn bytes
  with anything terminal would be a receipt that lies, because behind them the answer was
  accepted and delivered. The door does not know which kind of hub it is talking to and does
  not need to — which is the reason this is recorded rather than deleted.
* **What the staging hop cost, said plainly.** Putting the receipt down under a second name
  before moving it into place bought the paragraph above, and it brought two shapes with it.
  Both are **contrived** under the drop's standing ruling — they need a writer that is already
  this uid, inside a directory that is `0700` — and both are recorded here rather than fixed,
  because at that point the box is already his and the ring is not the secret worth defending.
  * **A chmod that follows a link.** `rename` follows no link, which is exactly why the final
    name is safe now. The staging name is not renamed onto, it is *opened* and then *chmodded* —
    and both of those follow one. So a link planted at the staging name before the hub reaches
    it takes the receipt's bytes and has its target narrowed to `0600`, a file that need not be
    in the drop at all. It has to be planted blind: the staging name carries the hub's pid, and
    the only name anybody is ever told is the finished one. The drop's own notes named the open
    alone when this landed and now name the chmod beside it, because the chmod is the half that
    reaches out of the directory.
  * **A long name in the drop now gets no receipt.** The staging name is the receipt's name plus
    about twenty-one bytes, and nothing here bounds how long a name in the drop may be — so a
    name that had no headroom left has twenty-one bytes less than none. The act still happens:
    the file is read, judged and consumed exactly as always. Only the receipt fails to be
    written, so your POST waits the full 2.5 seconds and is answered `504` for a write that went
    through — the one shape §3 calls "the hub has not said what became of it yet", meaning it
    here for good. Nothing you send can reach this: the door mints every name it writes, and
    they are short. It needs a program writing into the drop directly (§4's `ref` note) under a
    name near the filesystem's own limit.
* **One door per state home.** An operational recommendation, nothing more — the code neither
  enforces it nor coordinates two doors that run: each door's routing memory (§4 rung 3) is
  its own, learned from the lines it has served, so two doors on one state home means a tap
  can be refused "did not say which conversation" by the door that never served the ask its
  sibling did. Run one; if you ever must run two, **name `conversation` AND `lane` on every
  write**, which is what §7.4 asks of you anyway.

  The conversation alone is not enough, and it is worth being exact about why, because it used
  to be. Rung 1 no longer returns on the conversation alone: where the body names a conversation
  and no lane, it reads the same per-door memory rung 3 reads, to fill the lane of the question
  being answered (§4). So the memory is still on the path, and two doors still disagree about
  what is in it. Doors A and B, one state home: his stream is open on A, so only A ever served
  the `ask` and only A remembers it. The tap goes to B with exactly the body this document
  prescribes — conversation named, no lane. B has never heard of that question, fills nothing,
  and the act goes to the conversation's own voice; if the session that asked is a lane, the hub
  refuses it and he is told nothing went. Naming the lane is what takes the memory out of the
  path, because a lane on the body short-circuits the fill before it is reached.
* **Your prefix calls a timeout a refusal, and that one is not punctuation.** Your client
  composes every unhappy answer as a prefix of its own plus our `why` — one prefix before ours
  for a line of words, another before ours for a tap, and each of them says in its own words
  that we refused it. On a genuine refusal that is two sentences where you meant one and nothing
  worse: ours are whole sentences, capital letter and full stop, because the very same strings
  are read on his phone where there is no prefix at all, so he gets your prefix and then *"That
  message had no words in it, so it was not sent."* Clumsy, and true.

  On a **504** the same prefix says something that is not true. The word is wrong, not the
  spacing: the door said neither "refused" nor "ok" on purpose, your prefix supplies the
  refusal, and our sentence immediately contradicts it — *"the hub has not said what became of
  it yet — it may still; sending it again may say it twice."* The tick your client draws under
  the row says the same thing beside it: that the line did not go, and that we are the ones who
  refused it. It may well have gone. Fixing this is the same one branch step 7 asks for — a
  third state for a third outcome — and it is the reason that branch is worth ten minutes rather
  than a backlog entry.

  (Your two prefixes and that tick are your own strings, so this document does not quote them;
  each is one search of your own tree, and the letter that carries the citations says which.)

  The punctuation half is recorded rather than fixed, because that fix is a wording decision on
  one side or the other and neither side should take it alone — the cheapest version is your
  prefix ending in a line break or a dash rather than a colon and a space. The door's **own**
  refusals all open in lower case and read on from your colon, so those compose correctly
  already; it is only the hub's own sentences, passed through as `why`, that begin again.
* **Nothing of yours has talked to this binary yet.** The hermetic trial proves the seam
  with a scripted client, not your browser, your EventSource's reconnect behaviour, or your
  bridge's forwarding — your own suite holds your half against a stub standing where
  this binary stands, and the two halves have not met. That meeting is what adoption is.
