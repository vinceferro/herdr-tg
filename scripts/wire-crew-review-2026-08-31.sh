#!/usr/bin/env bash
# wire-crew-review-2026-08-31.sh — the GATED half of this repo's first crew-review.
#
# Editing the crew's own config is a behaviour change every future build inherits, so it is not
# applied autonomously. Running this script IS the approval. It is idempotent (every edit checks for
# its own marker first) and reversible (`--revert` restores the byte-exact backups it takes).
#
#   bash scripts/wire-crew-review-2026-08-31.sh            # apply
#   bash scripts/wire-crew-review-2026-08-31.sh --revert    # undo, byte-for-byte
#
# WHAT IT CHANGES AND WHY — ten edits, all additive text, no code touched.
#
# The finding that produced most of this: three coordinator failures in one session shared ONE shape,
# and it is the shape this repo already wrote down for its code. Three `mc` writes whose exit codes
# were never read, reported as "tracker updated" when all three had failed. Five charters and two
# plugins an engine hop delivered, dismissed as "not mine" from `git status` without one being
# opened. An exit code measured through a pipe to `tail`, which returns tail's status. Every one is
# a lookup that came up empty and was carried on quietly — the coordinator doing to its own tools
# exactly what the D3 guard did to its scan six times.
#
# So it is one rule, not three, and it goes where the operator's own overrides live.

set -euo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"

STAMP="2026-08-31"
BACKUP=".kickoff/backups/crew-review-$STAMP"
FILES=(
  ".kickoff/KICKOFF.local.md"
  "CLAUDE.md"
  ".claude/agents/egress.md"
  ".claude/agents/write-safety.md"
  ".opencode/agent/coordinator.md"
  ".opencode/agent/reviewer.md"
  ".opencode/agent/deployer.md"
  ".opencode/agent/builder.md"
)

if [ "${1:-}" = "--revert" ]; then
  [ -d "$BACKUP" ] || { echo "nothing to revert: no backup at $BACKUP" >&2; exit 1; }
  for f in "${FILES[@]}"; do
    b="$BACKUP/$(echo "$f" | tr '/' '_')"
    [ -f "$b" ] || continue
    cp "$b" "$f"
    echo "  reverted $f"
  done
  echo "reverted. The backup is kept at $BACKUP — delete it by hand when you are sure."
  exit 0
fi

mkdir -p "$BACKUP"
for f in "${FILES[@]}"; do
  [ -f "$f" ] || { echo "missing: $f — refusing to run against an unexpected tree" >&2; exit 1; }
  b="$BACKUP/$(echo "$f" | tr '/' '_')"
  [ -f "$b" ] || cp "$f" "$b"
done

python3 - <<'PYEOF'
import sys, pathlib

applied, skipped, failed = [], [], []

def edit(path, marker, anchor, replacement, label):
    """Idempotent anchored replace. A missing anchor is a FAILURE, never a silent skip —
    that is the rule this whole script exists to write down."""
    p = pathlib.Path(path)
    s = p.read_text()
    if marker in s:
        skipped.append(label); return
    if anchor not in s:
        failed.append(f"{label}: anchor not found in {path}"); return
    p.write_text(s.replace(anchor, replacement, 1))
    applied.append(label)

# ── 1. The shape behind all three coordinator failures ────────────────────────────────────────
edit(".kickoff/KICKOFF.local.md",
     "Fail closed on your own tools",
     """- **Report in chat, not to the tracker.** `.kickoff/bin/mc` is dead here — the pinned core dropped
  mission-control from the public line. Never claim a tracker write succeeded; check the exit code
  if you call it at all.""",
     """- **Report in chat, not to the tracker.** `.kickoff/bin/mc` is dead here — the pinned core dropped
  mission-control from the public line.
- **Fail closed on your own tools, not just in the code.** CLAUDE.md's "fail closed" and
  write-safety's "a lookup that comes up empty is a FAILURE, not a silent continue" bind you too.
  Three instances in one session, one shape: three tracker writes whose exit codes were never read,
  reported as "tracker updated" when all three had failed; five charters and two plugins an engine
  hop delivered, dismissed as "not mine" from `git status` without one being opened; an exit code
  read through a pipe to `tail`, which returns tail's status and not the command's. So: read the
  status of what you ran before you report it, and read it unpiped; open what a pull or a hop
  delivered before you decide whose it is; never report an action succeeded on the strength of
  having run it.
- **A crew file that arrives is yours to read.** An engine hop, a `kickoff pull` or a plugin install
  drops charters, hooks and skills into this repo. They arrive untracked and they are not yours —
  which is exactly why they need reading, not dismissing. One of them currently orders a tracker
  update this repo cannot perform.""",
     "KICKOFF.local: fail closed on your own tools")

# ── 2. The review mandate was scoped to the write path; two of seven blockers were outside it ──
edit(".kickoff/KICKOFF.local.md",
     "on what leaves this machine",  # NB: must not span the wrapped line break it inserts
     "- **Adversarial review is not optional on this repo's write path.**",
     "- **Adversarial review is not optional on the write path, on what leaves this machine, or on\n  what the operator is told.**",
     "KICKOFF.local: widen the review mandate")

# ── 3. Every charter's CANON block points at a CLAUDE.md section that does not exist ──────────
edit("CLAUDE.md",
     "## The quality bar",
     "## Conventions this repo actually holds to",
     "## The quality bar\n\nThe conventions this repo actually holds to. Every specialist charter's CANON block points here.",
     "CLAUDE.md: name the section the charters cite")

# ── 4. The corpus lesson is the one product lesson no charter carries ─────────────────────────
edit("CLAUDE.md",
     "build-a-two-sided-corpus",
     "asking. `docs/HUB-DESIGN.md` proposes deleting both files. Do not staff it; do not iterate on it",
     """asking. If the operator does reopen it, the standing lesson is
`.kickoff/memory/build-a-two-sided-corpus-before-tuning-a-classifier.md`: build the corpus from both
sides, and from real captures, before touching a rule — four rounds each traded a false negative for
a false positive because one real screen was pitted against seventeen imagined ones.
`docs/HUB-DESIGN.md` proposes deleting both files. Do not staff it; do not iterate on it""",
     "CLAUDE.md: carry the corpus lesson")

# ── 5. egress owns scanners with the same empty-lookup hole the D3 guard had ──────────────────
edit(".claude/agents/egress.md",
     "A lookup that comes up empty is a FAILURE",
     "## How you work",
     """## How you work
- **A lookup that comes up empty is a FAILURE, not a silent continue.** This is not write-safety's
  private rule, it is the repo's — the D3 guard was walked past six times and every one was that
  shape. Your scanners inherit it: a de-identification check that finds no files to scan, or a
  denylist it cannot read, must be loud rather than green.""",
     "egress: inherit the empty-lookup rule")

# ── 6. write-safety was the one specialist not told its own files are proposed for deletion ───
edit(".claude/agents/write-safety.md",
     "HUB-DESIGN.md",
     "## What you own",
     """## What you own
- `docs/HUB-DESIGN.md` proposes deleting `deliver.rs` outright and re-aiming your guard at a new
  subject — HTTP clients and sockets rather than `send_text` call sites. It is a proposal, not a
  decision. Keep both correct until the operator says otherwise, and say so if a change you are
  asked for only makes sense under that proposal.""",
     "write-safety: name the proposed deletion")

# ── 7-10. The opencode set, delivered by the hop and never reconciled with this repo ──────────
# These are appends, not anchored replaces: the four charters have no stable interior anchor, and
# an override block belongs at the end anyway — it must be read after everything it overrides.

def append(path, marker, text, label):
    p = pathlib.Path(path)
    s = p.read_text()
    if marker in s:
        skipped.append(label); return
    p.write_text(s.rstrip("\n") + "\n" + text)
    applied.append(label)

append(".opencode/agent/coordinator.md", "This repo overrides the above", """
## This repo overrides the above

**Inert today** — herdr-tg runs `WORKER_ENGINE=claude`. Under either engine
`.kickoff/KICKOFF.local.md` wins over this file.

- **There is no tracker here.** `.kickoff/bin/mc` is dead: the pin hopped to `core-v1.0.0-alpha`,
  whose public line drops mission-control, and the shim's "engine not present" message misdiagnoses
  it. Ignore every instruction above to read or update `TRACKER.md` or mission-state. Report in chat.
- **The crew is in `.claude/agents/`** — wire-protocol, write-safety, egress, operator-channel — not
  the planner/builder/reviewer shape above. See CLAUDE.md for the domain table.
- **The service is STOPPED by decision.** Never start it.
""", "opencode coordinator: override block")

append(".opencode/agent/reviewer.md", "checking is not enough", """
5. **On this repo, checking is not enough.** Gates are necessary and not sufficient: all five
   slice-3 rounds were green on fmt, clippy, tests, doc and secret scan before a sceptic broke them,
   and twice a round's own fix introduced a new blocker. On the write path, on what leaves the
   machine, and on what the operator is told, your mandate is to BREAK the change — not to confirm
   it. Grade each finding "reachable by accident" or "contrived"; that distinction is what lets the
   operator decide when to stop.
""", "opencode reviewer: break, do not confirm")

append(".opencode/agent/deployer.md", "Not in this repo", """
> **Not in this repo.** herdr-tg has no deploy target, and its service is STOPPED by decision after
> an adversarial review found four ways it could type the wrong thing into a real terminal.
> Restarting it is the operator's call and only his. Do not treat any prep for it as autonomous.
""", "opencode deployer: no deploy target")

append(".opencode/agent/builder.md", "never add a call site", """
- **In herdr-tg, never add a call site to `send_text`, `send_keys` or `send_input`.** One audited
  path exists, `crates/herdr-tg/src/deliver.rs`, and a source-scanning guard enforces it. That guard
  has been walked past six times; do not become the seventh.
- Every cargo command needs `env -u RUSTUP_TOOLCHAIN TMPDIR=<a real absolute dir> PATH="$HOME/.cargo/bin:$PATH"`,
  or seven transport tests fail for a reason you did not cause — and so does `git commit`.
""", "opencode builder: the write rule and the cargo prefix")

for label in applied: print(f"  applied  {label}")
for label in skipped: print(f"  already  {label}")
if failed:
    for f in failed: print(f"  FAILED   {f}", file=sys.stderr)
    sys.exit(1)
PYEOF

echo
echo "backups: $BACKUP  (revert with: bash scripts/wire-crew-review-2026-08-31.sh --revert)"
