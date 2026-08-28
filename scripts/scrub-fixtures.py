#!/usr/bin/env python3
"""scrub-fixtures.py — de-identify the captured herdr fixtures, and prove they are clean.

WHY THIS EXISTS
---------------
`capture-fixtures.sh` dumps REAL BYTES off the operator's live herd. Those bytes carry the
operator's private working context: the visible text of somebody's terminal, absolute paths under
their home directory, agent session ids, terminal titles naming private work, and their username
and hostname in shell prompts. This repository is PUBLIC. A fixture is committed forever.

So the capture pipeline is: capture -> **scrub** -> **check** -> write. The scrub is not optional
and is not behind a flag; `capture-fixtures.sh` calls it unconditionally and refuses to finish if
the check does not come back clean.

DESIGN RULES (each one is load-bearing — read before editing)
-------------------------------------------------------------
1. **Rule-based, never a lookup table.** A table of "the strings we saw last time" cannot scrub the
   NEXT capture, which will carry different paths, titles and ids. Every rule below keys off the
   FIELD, not off the value, so an unseen value is scrubbed the first time it appears.

2. **Free text is replaced wholesale, never pattern-matched.** `pane.read`'s text and every
   terminal title are regenerated from scratch. You cannot regex your way to "no private content"
   in a human's prose; you can only refuse to carry it. This is what makes the leak un-repeatable.

3. **This file contains no private strings.** It derives the operator's identity at runtime
   (`getpass`/`socket`/`~`). Hardcoding the username or hostname here would leak, in the fix, the
   exact thing the fix exists to remove.

4. **Structure is preserved byte-exactly.** Values change; keys, key ORDER, nesting, and
   optional-field presence/absence do not. `golden::snapshot_roundtrip_loses_nothing` diffs the
   fixture against its own re-serialization and the drift tests read `required` field sets, so a
   scrub that added or dropped one key would silently weaken them. Verified: json round-trips
   these fixtures byte-identically, so the only deltas are the ones this script makes.

5. **Idempotent.** Mappings are assigned in first-seen order over a fixed pool, so re-running on
   already-scrubbed fixtures is a no-op.

6. **The check is independent of the scrub**, and is machine-independent as well as
   machine-specific: it re-derives this box's identity AND applies structural patterns (any home
   path, any session-id shape, any user@host prompt), so it catches a leak shape the rules do not
   yet know about — on this box or on any other.

USAGE
-----
    scrub-fixtures.py [--fixtures DIR]     scrub in place, then check (exit 1 if unclean)
    scrub-fixtures.py --check [--fixtures DIR]   check only, change nothing
    scrub-fixtures.py --report ...         print before/after leak counts
"""

from __future__ import annotations

import argparse
import getpass
import json
import os
import re
import socket
import sys
from pathlib import Path

# ── the synthetic vocabularies ──────────────────────────────────────────────────────────────────
# Deliberately generic, obviously-placeholder, and free of the substrings "idle", "working",
# "blocked" and "done": `render::a_roster_line_never_shows_a_status` asserts a rendered line
# carries no status word, and a workspace label or title containing one would make that test
# fail for a reason that has nothing to do with the property it is defending.

PROJECT_POOL = [
    "acme-monorepo",
    "desktop-lab",
    "agent-kickoff",
    "api-gateway",
    "notes-linkmap",
    "bridge-tg",
]

# One entry carries an em dash on purpose: `render::width` counts CHARS, not bytes, and a fixture
# with no multi-byte title would stop exercising that path.
TITLE_POOL = [
    "sample task one",
    "sample task two",
    "sample task three — with an em dash",
    "sample task four",
    "sample task five",
    "sample task six",
    "sample task seven",
    "sample task eight",
    "sample task nine — continued",
    "sample task ten",
]

HOME_ROOT = "/home/user/projects"
PLACEHOLDER_USER = "user"

# The synthetic terminal screen. Mixed widths, box drawing, arrows and blank lines, matching the
# character profile of a real capture (no ESC, no CR, no TAB — herdr's `format: "text"` is
# already stripped) so the fixture still exercises realistic UTF-8 parsing and width maths.
SCREEN_POOL = [
    "$ cargo test --workspace",
    "   Compiling herdr-client v0.1.0 (/home/user/projects/bridge-tg/crates/herdr-client)",
    "   Compiling herdr-tg v0.1.0 (/home/user/projects/bridge-tg/crates/herdr-tg)",
    "    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.71s",
    "     Running unittests src/lib.rs",
    "",
    "running 12 tests",
    "test proto::model::tests::a_status_round_trips ... ok",
    "test proto::event::tests::the_dot_form_family_decodes ... ok",
    "test transport::tests::a_short_read_is_not_a_parse_error ... ok",
    "",
    "┃ summary ─────────────────────────────────────────────────────────────────────",
    "┃  → 12 passed · 0 failed · 0 ignored",
    "┃  → wall 0.24s · peak rss 41 MiB",
    "┃",
    "│ notes",
    "│  • the decoder is written against captured bytes, never against the doc",
    "│  • every fixture in this tree is synthetic — see scripts/scrub-fixtures.py",
    "│  • widths below are deliberately ragged so the renderer is exercised",
    "",
    "▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀",
    "  step 1 of 4 — resolve the socket path                                   [ ok ]",
    "  step 2 of 4 — handshake                                                 [ ok ]",
    "  step 3 of 4 — snapshot                                                  [ ok ]",
    "  step 4 of 4 — render                                                    [ ok ]",
    "",
    "$ git status --short",
    " M crates/herdr-client/src/proto/model.rs",
    " M crates/herdr-tg/src/render.rs",
    "?? scripts/scrub-fixtures.py",
    "",
    "$ ./scripts/proof-slice1.sh --gates=0,1",
    "gate 0  reference sane         ok    session_snapshot, protocol 20",
    "gate 1  herd non-empty         ok    6 workspaces / 6 panes",
    "",
    "▶ next: re-read the spec section on the two envelope families before touching the decoder,",
    "  because the snake_case frames tag data.type and the dot-form frames do not, and a model",
    "  tagged on data.type parses the first family and silently drops the second.",
    "",
    "· tip · a full `visible` read of an N-row viewport comes back with N-1 newlines, so sizing",
    "·       an excerpt off the wrong one of those two numbers is an off-by-one nobody sees.",
    "",
    "$ echo 'this screen is synthetic placeholder content'",
    "this screen is synthetic placeholder content",
    "",
]


def _identity_needles() -> list[str]:
    """Strings identifying THIS machine and operator, derived at runtime and never hardcoded."""
    needles: set[str] = set()
    try:
        needles.add(getpass.getuser())
    except Exception:
        pass
    for key in ("USER", "LOGNAME", "USERNAME"):
        v = os.environ.get(key)
        if v:
            needles.add(v)
    try:
        host = socket.gethostname()
        if host:
            needles.add(host)
            needles.add(host.split(".")[0])
    except Exception:
        pass
    home = os.path.expanduser("~")
    if home and home != "~":
        needles.add(home)
        base = os.path.basename(home.rstrip("/"))
        if base:
            needles.add(base)
    # Never let a degenerate value ("", "/", "user") make the check vacuous or self-tripping.
    return sorted(n for n in needles if len(n) >= 3 and n not in {PLACEHOLDER_USER, "home"})


# ── structural leak patterns (machine-independent) ──────────────────────────────────────────────

SES_RE = re.compile(r"ses_[A-Za-z0-9]{8,}")
UUID_RE = re.compile(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b")
HOME_RE = re.compile(r"/home/([A-Za-z0-9._-]+)")
PROMPT_RE = re.compile(r"\b[A-Za-z0-9._-]+@[A-Za-z0-9._-]+:[~/]")
TILDE_PATH_RE = re.compile(r"~/[A-Za-z0-9._/-]+")

SES_OK = re.compile(r"^ses_0{16,}[0-9]{1,4}$")
UUID_OK = re.compile(r"^0{8}-0{4}-4000-8000-[0-9a-f]{12}$")


def _leaks_in_text(text: str, needles: list[str]) -> list[str]:
    """Every reason `text` is not safe to publish. Empty list == clean."""
    out: list[str] = []
    for n in needles:
        if n in text:
            out.append(f"identity string {n!r}")
    for m in HOME_RE.finditer(text):
        if m.group(1) != PLACEHOLDER_USER:
            out.append(f"home path {m.group(0)!r}")
    for m in SES_RE.finditer(text):
        if not SES_OK.match(m.group(0)):
            out.append(f"session id {m.group(0)!r}")
    for m in UUID_RE.finditer(text):
        if not UUID_OK.match(m.group(0)):
            out.append(f"uuid {m.group(0)!r}")
    m = PROMPT_RE.search(text)
    if m:
        out.append(f"shell prompt {m.group(0)!r}")
    for m in TILDE_PATH_RE.finditer(text):
        out.append(f"home-relative path {m.group(0)!r}")
    return out


# ── the scrubber ────────────────────────────────────────────────────────────────────────────────


class Scrubber:
    """Field-keyed, first-seen-order, idempotent. Shared across all fixture files at once so a
    project named in `snapshot.json` gets the same synthetic name in `events-mixed.ndjson`."""

    def __init__(self) -> None:
        self.projects: dict[str, str] = {}
        self.titles: dict[str, str] = {}
        self.sessions: dict[str, str] = {}
        self.terminals: dict[str, str] = {}

    # -- mappings ------------------------------------------------------------------------------
    def project(self, original: str) -> str:
        if original not in self.projects:
            i = len(self.projects)
            self.projects[original] = (
                PROJECT_POOL[i] if i < len(PROJECT_POOL) else f"project-{i + 1}"
            )
        return self.projects[original]

    def title_body(self, body: str) -> str:
        if body not in self.titles:
            i = len(self.titles)
            self.titles[body] = (
                TITLE_POOL[i] if i < len(TITLE_POOL) else f"sample task {i + 1}"
            )
        return self.titles[body]

    def session(self, original: str) -> str:
        if original not in self.sessions:
            i = len(self.sessions) + 1
            if original.startswith("ses_"):
                # Preserve the wire shape: `ses_` + the same number of trailing characters.
                width = max(len(original) - 4, 20)
                self.sessions[original] = "ses_" + str(i).rjust(width, "0")
            else:
                self.sessions[original] = f"00000000-0000-4000-8000-{i:012d}"
        return self.sessions[original]

    def terminal(self, original: str) -> str:
        if original not in self.terminals:
            i = len(self.terminals) + 1
            self.terminals[original] = "term_" + f"{i:015x}"
        return self.terminals[original]

    # -- field rules ---------------------------------------------------------------------------
    def path(self, original: str) -> str:
        """A working directory becomes a synthetic one under a placeholder home."""
        base = os.path.basename(original.rstrip("/")) or original
        return f"{HOME_ROOT}/{self.project(base)}"

    def title(self, original: str) -> str:
        """Regenerate a terminal title, keeping only its SHAPE.

        Two shape features are preserved because the model and the renderer both depend on them:
        a leading decoration glyph (herdr's `terminal_title` carries `✳ ` where
        `terminal_title_stripped` does not — the only place that relationship is captured), and an
        `AGENT | ` prefix. The body is keyed on the UNDECORATED text, so a title and its stripped
        twin map to the same synthetic body and stay consistent with each other.
        """
        rest = original
        decoration = ""
        m = re.match(r"^([^\w\s]+\s+)(.*)$", rest, flags=re.UNICODE)
        if m:
            decoration, rest = m.group(1), m.group(2)
        agent = ""
        m = re.match(r"^(\S{1,6} \| )(.*)$", rest)
        if m:
            agent, rest = m.group(1), m.group(2)
        return f"{decoration}{agent}{self.title_body(rest)}"

    def screen(self, original: str) -> str:
        """Regenerate a pane's visible text from scratch, preserving the invariants tests pin.

        Preserved EXACTLY: the newline count (`golden` asserts `line_count() == viewport_rows - 1`
        against the snapshot's own `scroll.viewport_rows`) and the trailing newline. The final
        line is forced NON-EMPTY: `trimmed_tail` strips trailing newlines before splitting, so a
        blank last line would make `trimmed_tail(n)` yield fewer than n lines and turn
        `pane_read_revision_is_zero_while_pane_info_revision_is_not` red.
        """
        newlines = original.count("\n")
        if newlines == 0:
            return "this pane's contents are synthetic placeholder text"
        lines = [SCREEN_POOL[i % len(SCREEN_POOL)] for i in range(newlines)]
        if not lines[-1].strip():
            lines[-1] = "$ "  # never end on a blank line
        return "\n".join(lines) + "\n"

    # -- traversal -----------------------------------------------------------------------------
    def walk(self, node, key: str | None = None, parent: dict | None = None):
        if isinstance(node, dict):
            return {k: self.walk(v, k, node) for k, v in node.items()}
        if isinstance(node, list):
            return [self.walk(v, key, parent) for v in node]
        if not isinstance(node, str):
            return node

        if key in ("cwd", "foreground_cwd"):
            return self.path(node)
        if key == "label" and parent is not None and "workspace_id" in parent:
            return self.project(node)
        if key in ("terminal_title", "terminal_title_stripped", "title"):
            return self.title(node)
        if key == "terminal_id":
            return self.terminal(node)
        # `agent_session` is {source, agent, kind, value}; only `value` is the id.
        if key == "value" and parent is not None and {"source", "kind"} <= set(parent):
            return self.session(node)
        if key == "text" and parent is not None and "pane_id" in parent and "source" in parent:
            return self.screen(node)
        return node


# ── driver ──────────────────────────────────────────────────────────────────────────────────────

# snapshot.json first so workspace order — not pane order — drives the synthetic naming.
ORDER = ["snapshot.json", "pane_read.json", "events-mixed.ndjson", "errors.ndjson", "pong.json"]


def fixture_files(fixdir: Path) -> list[Path]:
    named = [fixdir / n for n in ORDER if (fixdir / n).exists()]
    rest = sorted(
        p
        for p in fixdir.iterdir()
        if p.is_file() and p.suffix in {".json", ".ndjson"} and p not in named
    )
    return named + rest


def read_frames(path: Path) -> list | None:
    """Every JSON frame in an NDJSON-shaped fixture, or None if this file is not that shape.

    The captured fixtures are one compact JSON value per line — that is the wire format, and
    json round-trips them byte-identically, which is what lets this script change values without
    disturbing key order or optional-field presence. `herdr-schema-p20.json` is a pretty-printed
    reference dump instead; returning None marks it CHECK-ONLY so a re-serialization cannot drift
    a 255 KB document the drift tests read `required` field sets out of. It is still checked, and
    the check still fails closed, so a leak that ever lands there stops the capture rather than
    being silently skipped.
    """
    try:
        return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    except json.JSONDecodeError:
        return None


def write_frames(path: Path, frames: list) -> None:
    path.write_text(
        "".join(
            json.dumps(f, ensure_ascii=False, separators=(",", ":")) + "\n" for f in frames
        )
    )


def check(fixdir: Path, verbose: bool = True) -> int:
    needles = _identity_needles()
    total = 0
    for path in fixture_files(fixdir):
        leaks = _leaks_in_text(path.read_text(), needles)
        if leaks:
            total += len(leaks)
            if verbose:
                shown = sorted(set(leaks))
                print(f"  LEAK  {path.name}: {len(leaks)} hit(s)", file=sys.stderr)
                for s in shown[:8]:
                    print(f"          {s}", file=sys.stderr)
                if len(shown) > 8:
                    print(f"          … and {len(shown) - 8} more distinct", file=sys.stderr)
        elif verbose:
            print(f"  clean {path.name}")
    return total


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    here = Path(__file__).resolve().parent.parent
    ap.add_argument(
        "--fixtures", default=str(here / "crates/herdr-client/tests/fixtures"), type=Path
    )
    ap.add_argument("--check", action="store_true", help="verify only; change nothing")
    args = ap.parse_args()
    fixdir: Path = args.fixtures
    if not fixdir.is_dir():
        print(f"scrub-fixtures: no such fixture dir: {fixdir}", file=sys.stderr)
        return 2

    if args.check:
        n = check(fixdir)
        print(
            "scrub-fixtures: CHECK CLEAN"
            if n == 0
            else f"scrub-fixtures: CHECK FAILED — {n} leak hit(s)",
            file=sys.stderr if n else sys.stdout,
        )
        return 1 if n else 0

    # Parse EVERYTHING before writing ANYTHING: a parse failure half way through must not leave
    # the fixture set partly scrubbed and partly raw.
    paths = fixture_files(fixdir)
    parsed = [(p, read_frames(p)) for p in paths]

    s = Scrubber()
    for path, frames in parsed:
        if frames is None:
            print(f"  check-only {path.name} (not line-delimited JSON; never rewritten)")
            continue
        write_frames(path, [s.walk(f) for f in frames])
        print(f"  scrubbed {path.name}")
    print(
        f"  mappings: {len(s.projects)} project(s), {len(s.titles)} title(s), "
        f"{len(s.sessions)} session id(s), {len(s.terminals)} terminal id(s)"
    )
    n = check(fixdir)
    if n:
        print(
            f"scrub-fixtures: REFUSING — {n} leak hit(s) survived the scrub. "
            "A new leak shape has appeared; add a rule for it above rather than deleting this check.",
            file=sys.stderr,
        )
        return 1
    print("scrub-fixtures: OK — fixtures scrubbed and verified clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
