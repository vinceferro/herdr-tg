# Slice 3 — what the review cost to close, 2026-08-31

The first adversarial review of slice 3 (`docs/SLICE-3-REVIEW.md`) returned **BROKEN**, with 7
blockers and 16 majors. This is the record of closing them: five rounds, 27 commits, and a test suite
that went from **232 to 352** on `main`.

Every round had the same shape — fix, then hand the tree to a fresh sceptic told to break it. Every
round the sceptic found something real. That is the point of the loop, and it is why this file exists:
the count of rounds is the finding.

## What is closed and merged

All 7 original blockers, verified by an independent agent that re-ran the review's own reproductions.

| Area | What was wrong | Closed by |
| --- | --- | --- |
| `permission.rs` | A two-option dialog was invisible, so "no" was typed and Enter confirmed Yes | `d3141dc`, `c94f45d` |
| `bot.rs` / `deliver.rs` | Buttons resolved by position, so "Reject" could confirm "Allow always" | `f00b715` |
| `deliver.rs` | A relative key move raced the operator's own keyboard | `706172c`, `9155ba0` |
| `routing.rs` | Topics and pushes keyed on a bare Telegram id, so a DM swipe-reply hit a forum pane | `e878c69` |
| `summarize.rs` | Pane text reached a hosted model; nothing in the crate could tell | `b54f086`, `d74b073` |
| `no_live_write_call_site.rs` | The write guard skipped any directory named `target` | `a2dae97`, `7b5850f` |
| `voice.rs` | A model's summary stood in for the operator's real screen | `82f1676` |

The write guard was walked past **six** times across the five rounds — by a source directory named
`target`, by `include!` of a non-`.rs` file, by a trailing comment on a test module's brace, by
`#[rustfmt::skip]` indentation, by `#[path]` resolved the way rustc resolves it, and by a Cargo target
path. All six are closed, and the guard now proves it scanned what ships rather than asserting it.

## What is open

**The screen parser is not settled.** Round 5 inverted its default — a screen must now look *safe to
type into* rather than merely *not like a menu* — and a sceptic working from real `tmux capture-pane`
output still got 13 live controls classified as safe, erring toward typing into controls by about six
to one. That work sits on `fix/r5-parser` (367 tests) and is **deliberately not merged**.

The root cause is not the parser. herdr's protocol exposes agent status (`Idle`/`Working`/`Blocked`/
`Done`) and raw screen bytes, and nothing structured about what an agent is asking. The parser's whole
job is reconstructing structure that was already discarded. Five rounds is the evidence that it cannot
be done reliably from a rendered screen.

The service has stayed **stopped** throughout.

## Accepted, not fixed

The operator decided on 2026-08-30 that the write guard stays a source scanner rather than becoming a
compiler-enforced private API. Under that decision, evasions an honest change could stumble into get
fixed; ones needing deliberate contrivance get written down. Two remain, both in
`no_live_write_call_site.rs` and both needing someone actively trying:

- `include !("...")`, with a space before the `!`, is legal Rust and is not recognised as an inclusion.
- A line that begins inside a multi-line string, closes it mid-line, then carries a real statement
  reads to the line-local scanner as an unclosed string.

Two more are open in `summarize.rs`, both contrived: the egress latch is read once per call rather
than atomically, and `plausible()` does not screen control characters out of a returned gist.

## What this cost, honestly

Roughly a third of the work went into the write path — the parser, the key sending, the button
plumbing. If the bridge stops typing into panes, that third does not survive.

The rest does: the egress gates, chat-scoped routing, the write guard, the audit records, the
operator-facing wording rules, and every property test that pins them. So does the lesson that
produced them, which is cheaper to read here than to rediscover: **a fix that has not been attacked
by someone trying to break it is a draft.** Every round of this loop was green before the sceptic
arrived.
