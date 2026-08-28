# herdr-tg

**A Telegram front door for your [herdr](https://herdr.dev) agent herd.** One bot per workspace;
agents' questions arrive as chat messages with buttons; your replies land in the right terminal
pane. Deterministic routing you can trust with a one-letter answer. A single Rust binary with no
runtime to drift.

## Why

Agent supervision is *discrete turns* — a question arrives, you answer it, silence until the next
one. That is structurally a chat message. Terminal mirrors (like [Collie](https://colliepwa.dev),
which stays installed as the drill-down *viewport*) show you a screen; herdr-tg gives the herd a
*voice*: push-native (no VAPID/service-worker machinery), voice dictation, real history,
camera-roll images — all Telegram's own affordances instead of re-implementations inside a PWA.

Routing is deliberately **deterministic in v1**: your reply goes to the pane that asked (sticky,
visible, one tap to switch). No LLM sits in the path between your words and a terminal — a "y"
can never be misread. The routing *agent* arrives in v2, running as a pane itself, on top of the
boring-correct layer.

## Status

Incubated 2026-08-28. **Slice 1 — the `herdr-client` crate and the `herdr-tg` CLI (`status`,
`read`, `doctor`, `watch`) — is built**; its falsifiable proof is `./scripts/proof-slice1.sh`,
and whether it currently runs green is not claimed here. The build spec, with the verified
protocol-20 findings it rests on, is [docs/SLICE-1.md](./docs/SLICE-1.md); the product intent,
architecture diagrams, decision log and slices live in [PLAN.md](./PLAN.md). Built for personal
use by [@vinceferro](https://github.com/vinceferro) against herdr 0.8.x on a tailnet'd Linux box.

## Kickoff adoption (turnkey)

This repo is prepared for the [kickoff](https://github.com/vinceferro/claude-kickoff) coordinator
pattern:

```bash
cd ~/Projects/herdr-tg
kickoff adopt --dir ~/Projects/herdr-tg   # wires .kickoff/, engine parity, gates (additive)
# then, in the first coordinator session in this repo: follow /adopt
kickoff verify --dir ~/Projects/herdr-tg  # one-shot health check (needs no Telegram)
```

Per-instance wiring to carry into `.kickoff/instance.env` (modeled on the bliz instance):

```bash
export MEMORY_DIR="$HOME/obsidian-vault/Memory"          # shared vault corpus — NOT a repo-local one
export TELEGRAM_STATE_DIR="$HOME/.claude/channels/telegram-herdr-tg"  # this project's own dogfood bot
export KICKOFF_CORE_DIR="$HOME/Projects/claude-kickoff"   # pinned core clone — must match .kickoff/instance.env
```

**The dogfood rule:** this project's own Telegram bot is herdr-tg's first user. From slice 3
onward, the instance runs on its own product — bootstrap day is integration-test day.

## License

MIT — see [LICENSE](./LICENSE).
