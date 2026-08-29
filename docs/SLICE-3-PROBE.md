# The protocol-20 send path — settled by probe, 2026-08-29

> Run: `scripts/verify-send-p20.sh` against a throwaway `herdr --session probe` (socket
> `~/.config/herdr/sessions/probe/herdr.sock`), 119 RPCs, none against the live herd.
> `docs/SLICE-1.md` deferred all five of these deliberately; this document replaces the deferral.
> Where it and `HERDR_API.md` (protocol 16) disagree, this document wins.

## P1 — the success tag is `ok`, and it means less than it looks like

All three write methods return the bare `{"type":"ok"}` envelope. The spec **inferred** this from
`ok` being the only void tag among 58 `ResponseResult` tags; it is now observed on the wire for
`pane.send_text`, `pane.send_keys` and `pane.send_input`.

What it does **not** mean: that the agent received, rendered, parsed, or acted on the bytes. It means
herdr took them. **Slice 3's Telegram confirmation must say "sent", never "delivered."** A focused
TUI dialog can swallow both the text and the submit key with both RPCs reporting `ok`.

## P2 — the key grammar, and a trap inside it

Sent one key at a time into a shell pane, each preceded by a fresh `#` so anything that lands is a
comment. `ctrl+d` and `ctrl+z` were excluded in every spelling — EOF would have closed the pane.

**Accepted:** `Enter` · `enter` · `ENTER` · `Return` · `Tab` · `Escape` · `Esc` · `Space` ·
`Backspace` · `BackSpace` · `BS` · `Up` · `Down` · `Left` · `Right` · `F1` · `F12` · `ctrl+c` ·
`ctrl+u` · `ctrl+l` · `shift+tab` · `alt+f` · `alt+Up` · `ctrl+shift+p` · `ctrl+alt+shift+p` ·
single characters (`a`, `C`).

**Rejected** (`invalid_key`): `CR` · `BSpace` · `BTab` · `Home` · `End` · `PageUp` · `PageDown` ·
`Insert` · `Delete` · `ctrl-c` · `Ctrl-C` · `C-u` · `M-b`.

Two things follow, and the second is the dangerous one.

1. **Named navigation keys do not exist.** `Home`, `End`, `PageUp`, `PageDown`, `Insert` and
   `Delete` are all refused. Anything that wants them must send the escape sequence as *text*.
2. **The tmux `-` chord form is not supported — except that `c-c` is special-cased**, and that
   exception is a trap. Probed directly:

   | key | result | | key | result |
   |---|---|---|---|---|
   | `C-c` | **ok** | | `C-x` | `invalid_key` |
   | `c-c` | **ok** | | `C-a` | `invalid_key` |
   | `C` | ok (literal char) | | `C-C` | `invalid_key` |
   | | | | `C-u` | `invalid_key` |

   So a config written in tmux style **works for interrupt and fails for everything else**. That is
   the worst possible shape: the author tests `C-c`, sees it work, concludes the grammar is
   tmux-like, and every other binding they write is silently dead.

   **Decision for slice 3:** the `Key` newtype accepts the `+` chord form only, and **rejects `c-c`
   even though herdr would take it.** Relying on a special case invites the whole grammar.

Herdr accepting a key name is still not the PTY receiving what you meant — the pane dump showed real
`^C` characters, which is the confirmation that matters.

**Still open, and not a herdr fact:** the per-harness **submit key** (does `claude` submit on `Enter`
like `opencode`?). It needs an agent pane in a probe session, and it is a property of the harness,
not of herdr.

## P3 — `send_input` does not execute lines

Two lines sent through `pane.send_input` in one call left the shell's prompt count unchanged (4 → 4):
both sat in the input buffer, unexecuted.

This is the finding that makes the product safe to use from a phone. `pane.send_text` writes **raw
bytes**, so a `\n` inside a multi-line Telegram reply is a real Enter that *executes a line in the
operator's shell*. `send_input` does not do that.

**Decision for slice 3:** multi-line replies go through `pane.send_input`, and the submit key is a
separate, deliberate step. Never `send_text` for operator-authored text.

## P4 — a filtered subscription fires on later transitions too

Subscribing with `filter=blocked` on an idle pane yielded 0 frames in 2 s, as expected. Driving the
pane `idle → blocked` (via `pane.report_agent`, so no real agent and nothing typed) produced the
frame immediately:

```json
{"data":{"agent":"probe","agent_status":"blocked","pane_id":"w1:p1","workspace_id":"w1"},
 "event":"pane.agent_status_changed"}
```

So a filtered subscription is **not** catch-up-at-subscribe only. Combined with the known
replay-at-subscribe behaviour, **one filtered subscription per pane gives both the recovery path and
the live edges from the same stream** — slice 3 does not need a second mechanism for "what did I miss
while the laptop was asleep".

## P5 — `state_change_seq` is stable while blocked

Sampled every second for 10 s with no writes: `2 2 2 2 2 2 2 2 2 2`. One distinct value.

It is therefore a **sound dedupe key** for the notification state machine — `(pane_id,
state_change_seq)` identifies an ask, and re-reading it will not manufacture a second push for the
same one. Caveat carried forward: it costs an extra `agent.get` per event, and that read races the
event.

## What this changes in the plan

| Slice 3 item | Settled as |
|---|---|
| the reply RPC | `pane.send_input` for operator text; **never** `send_text` |
| the confirmation wording | "sent to `<pane>`" — never "delivered" |
| the `Key` newtype | `+` chord form only; reject `-` forms including the working `c-c` |
| navigation keys | not available by name; send as text if ever needed |
| event subscription | one filtered `pane.agent_status_changed` per pane, covers recovery and live |
| push dedupe key | `(pane_id, state_change_seq)` |
| still unsettled | the per-harness submit key — needs an agent pane, not a shell |
