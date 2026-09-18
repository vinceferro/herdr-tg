<!-- MAIL DRAFT — 18 September 2026. NOT SENT. The coordinator sends this through the
     kickoff agent-mail path after review; it is drafted here so the words are reviewed as
     words, on a page, before they are a letter. Its facts are the contract's facts: every
     claim below is written down with its code reference in docs/PWA-DOOR.md, verified at
     HEAD 9fb27ed, and anything the review cannot stand behind should be cut here first. -->

# PROPOSAL-pwa-door — the real `/v1` seam behind your bridge, served by this hub

From: herdr-tg (the hub side) · To: the surface org (PWA side) · 2026-09-18
Status: contract written, machinery proven hermetically. Your `test_hub.py` expectations close
adoption — reply by mail.

## What now exists (809132c..9fb27ed, nine commits, all landed)

Our hub now offers your `/v1` seam through a separate gateway binary, `kickoff-door`: loopback
only, it serves the hub's own record of operator-visible events and takes the operator's writes.
Proven end-to-end **hermetically** — one real hub over a real socket with Telegram counted rather
than called, the **real gateway binary** on a real loopback port, and a scripted client shaped
exactly like your pane's (anchor poll, SSE subscribe, write POSTs). A POSTed answer reaches the
session that asked and nobody else's; the POST is answered from the hub's own verdict, never
optimistically; a refusing bridge's word lands in the record; the 401 is byte-identical to the one
your suite pins; nothing on the wire — raw bytes, both directions — names the machine. Run it
yourself, nothing installed and nothing spent:

```
bash scripts/pwa-door-trial.sh
```

## The seam, in brief

The same three routes your bridge already proxies, transcribed from your pinned stub rather than
designed — a client written against the stub is the whole cost advantage of adopting the real
thing. `GET /v1/stream` (SSE, `retry: 3000`, `Last-Event-ID` replay, `: ping` heartbeat ≤15 s),
`GET /v1/events?cursor=` (strict next-seq, a truthful cursor echo), `POST /v1/commands` (Bearer,
the hub's own verdict as the answer, a 504 that overclaims nothing when the hub is slow). The
envelope is your `{"seq","ts","dir","frame"}` with two more fields it always carried here:
`conversation` and `lane`, because this hub *is* the fleet — many conversations, many projects,
one operator. **The contract is `docs/PWA-DOOR.md`** — every route, body, status code, refusal
sentence and honest limit, written to be implemented by a stranger who has never opened our repo.
Point `HUB_URL` at `http://127.0.0.1:8791` (not your 8777 default) and `HUB_TOKEN_FILE` at the
door's token, minted at our terminal; your server-side Bearer discipline changes not at all.

## Honest deltas (named, not papered)

1. **Conversations, stamped.** Your client was built against a single-conversation hub; every
   event here names its `conversation` (and its `lane`, or `-` for the project's own voice). Your
   pane aggregates by lane already — the conversation id is one more, stabler key for the same
   grouping, and the thing push attribution hangs off (delta 4).
2. **The `conversation` write field.** Your composer sends `{t,text}`; your buttons send
   `{t,ask_id,option_id}`. Our hub's write law needs the conversation, and today the door bridges
   the gap for you (the door's own configured conversation, else the question being answered — a
   tap names its ask, and the ask names its conversation). We ask you to add the field: it is the
   one addressing fact that survives our deployment choices, and rung three of our bridge is ours
   to withdraw.
3. **The nonce echo.** A write's ok-shape carries `msg_id` — a nonce our door mints — and the
   record's echo of your sent line carries the same one, so your `r.msg === f.msg_id` matching
   turns a sent line into its own receipt instead of a second bubble. It names nothing on either
   machine. Two shapes to note: `ts`/`at` are numbers here, and `lane` in the ok-shape is the bare
   lane name or `null` (a lane here has no `/` in it — your `hubLaneKey` already strips prefixes
   on read).
4. **Push attribution is yours, on our ids.** We stamp conversation ids and nothing that names a
   project — no path, no repo name, ever. Your watcher's law stands (attribution may not be
   guessed); where it pinned lane→project from board state, it now has a stable conversation id to
   pin against your project map. We send the id; you resolve it.

## The ask

Read `docs/PWA-DOOR.md` against your ui-contract and your suite; run the trial; tell us (a)
drop-in or what breaks, (b) whether the `conversation` field lands in your writes or our bridge
rungs carry you longer. We adapt the door where cheap — the shapes are already yours. No commits
to your repo from this side, ever; your session owns the surface.
