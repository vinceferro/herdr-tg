#!/usr/bin/env bash
# The hermetic fleet trial — four conversations, a real hub, the real adapter, no Telegram.
#
#     bash scripts/fleet-trial.sh
#
# This is the door another org runs. Point your dispatcher at what it proves, not at this script:
# it starts four `kickoff-hub-attach` processes against a real hub over a real Unix socket, gives
# each one its own conversation, its own door and its own binding, and shows that each speaks only
# to the session its binding names — with Telegram counted instead of called, and one fake engine
# instead of four opencodes. Three of the four notes name a session of their own; the fourth names
# its own session under the conversation NEXT DOOR, which is what a dispatcher writes when it pairs
# its rooms off by one, and both halves of what a wall does with such a note are proved on it.
# Nothing here dispatches, schedules, supervises or infers health; when your Runner exists, it is
# the only new thing in the picture.
#
# It is a wrapper and deliberately a thin one. The trial itself is one `#[ignore]`d test inside the
# hub's own test module, because that is the ONLY place a hub with a faked Telegram surface can be
# built at all — the hub is not constructible from outside its own module, and `serve`
# hard-constructs the real Telegram surface. A `serve --fake-telegram` would put a fake Telegram in
# the shipped binary, so there is not one and there will not be one.
#
# The trial runs against `--lib`, not `--bins`. It used to be the other way: the crate was a single
# `main.rs` and its test module compiled into the binary target. Renaming the product to
# `kickoff-channel` moved the implementation into `lib.rs` so two commands could enter it, which
# moved every unit test with it — and `--bins` then matched nothing. What this script adds over
# `cargo test` is the environment (the variables
# an agent session gets wrong), a run of its own to work in, and the refusals below.
#
# HERMETIC, and it proves it rather than promising it:
#   * its own TMPDIR, made fresh and removed at the end — every socket, registry, ledger, audit,
#     channel home and binding the trial writes lives under it;
#   * it refuses, before it makes anything, to run with its work pointed at the real state directory;
#   * it gives the run a state home of its own and fails if ANYTHING appears in it, which is what
#     catches the one way a hermetic run leaks: code that works out where to keep its state from the
#     environment instead of being told;
#   * it lists ~/.local/state/herdr-tg before and after and fails if a path appeared or vanished.
#
# It touches no systemd unit, sends nothing to Telegram, and enrols nothing on this box. It does
# bind one loopback TCP port for the life of the run — the fake engine the four walls watch.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REAL_STATE="${XDG_STATE_HOME:-$HOME/.local/state}/herdr-tg"
THE_TRIAL=hub::tests::four_rooms_of_one_repo_reach_only_the_session_their_own_note_names_and_the_room_paired_off_by_one_reaches_nobody

say() { printf '%s\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

[ $# -eq 0 ] || die "usage: fleet-trial.sh — it takes no arguments"

command -v bun >/dev/null 2>&1 || die "bun is not on PATH; the adapter under test is a bun program"
command -v git >/dev/null 2>&1 ||
  die "git is not on PATH; the trial makes a repository for the four rooms to be dispatched in"
[ -x "$HOME/.cargo/bin/cargo" ] || command -v cargo >/dev/null 2>&1 ||
  die "cargo is not on PATH; mise shims hide it, which is why this script sets PATH itself"

# Fail closed on being pointed at the real thing, BEFORE anything is made. Every one of these has a
# way to end up naming the operator's own state directory — an exported variable, a symlink, a home
# that is itself a link — and a trial that wrote into it would rewrite a real conversation's secret.
# Checked on the root rather than on the run directory because refusing after `mktemp` has already
# put a stranger's directory in his state directory is a refusal that has already done the thing.
ROOT="${TMPDIR_ROOT:-/tmp}"
[ -d "$ROOT" ] || die "there is nowhere to work: $ROOT is not a directory"
case "$ROOT" in
  "$REAL_STATE"|"$REAL_STATE"/*) die "the run directory would be inside the real state directory" ;;
esac
if [ -e "$REAL_STATE" ] && [ "$(readlink -f "$ROOT")" = "$(readlink -f "$REAL_STATE")" ]; then
  die "the run directory would resolve to the real state directory"
fi

# A run of its own. Short, because a Unix socket path is 108 bytes and an agent session's scratch
# directory is most of that on its own — the trial makes sockets several directories down.
WORK="$(mktemp -d "$ROOT/fleet-trial.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# The trap, and it is the strong half of the proof. Everything in the trial is TOLD where its state
# lives; this is the state home anything that worked it out from the environment instead would land
# in, so a single entry here is a leak caught by name rather than inferred from a directory that
# happens not to have changed.
AMBIENT="$WORK/ambient-state"
mkdir -p "$AMBIENT"

# And the real state directory itself, listed by PATH — not by size or by modification time. A hub
# serving the operator re-stamps files that are already there every time it ticks, and calling that
# the trial's doing told another org their good run could not be trusted, on exactly the box this
# fixture is for. A trial that leaked would CREATE something: a secret, a registry, a socket, a
# ledger. That is what this looks for.
paths_under_the_real_state() {
  [ -d "$REAL_STATE" ] || { printf 'absent\n'; return; }
  find "$REAL_STATE" -printf '%P\n' 2>/dev/null | LC_ALL=C sort
}
BEFORE="$(paths_under_the_real_state)"

say "Running the hermetic fleet trial — four conversations, one hub, no Telegram…"
say
# Kept as well as shown, because an exit code is not enough to say the trial RAN — see below.
SAID="$WORK/what-it-said"
set +e
( cd "$REPO" && env -u RUSTUP_TOOLCHAIN TMPDIR="$WORK" XDG_STATE_HOME="$AMBIENT" \
    PATH="$HOME/.cargo/bin:$PATH" \
    cargo test -q -p kickoff-channel --lib -- --ignored --nocapture --exact "$THE_TRIAL" ) 2>&1 | tee "$SAID"
RC=${PIPESTATUS[0]}
set -e

ESCAPED="$(find "$AMBIENT" -mindepth 1 2>/dev/null || true)"
if [ -n "$ESCAPED" ]; then
  say
  printf '%s\n' "$ESCAPED"
  die "something in the trial worked out where to keep its state from the environment instead of \
being told; on a box with a real hub that is the operator's own state directory"
fi

AFTER="$(paths_under_the_real_state)"
if [ "$BEFORE" != "$AFTER" ]; then
  say
  diff <(printf '%s\n' "$BEFORE") <(printf '%s\n' "$AFTER") || true
  die "something appeared or vanished under the real state directory while the trial ran; if a hub \
of this box was serving the operator at that moment it may be its doing, so try again on a quiet \
box — but this run cannot be reported as hermetic"
fi

[ "$RC" -eq 0 ] || die "the fleet trial failed; the lines above say which property"

# And it RAN. A filter that matches nothing is not an error to `cargo test`: it prints "0 passed"
# and exits 0, so a trial that had been renamed, moved out of `hub::tests`, or had its `#[ignore]`
# taken off reported a clean fleet trial to another org while running no test whatever. The number
# is what says otherwise, and there is exactly one test behind this door.
grep -qE '^test result: ok\. 1 passed' "$SAID" ||
  die "the trial did not run: nothing here matched its name, and a filter that matches nothing is \
reported as a pass. Whoever renamed or moved it must change the name at the top of this script; \
until then this run proves nothing"

say
say "Nothing was kept outside the run's own directory, and the real state directory holds what it held."
say "What this did NOT prove is written at the top of the test, in"
say "  crates/kickoff-channel/src/hub/tests.rs — search for the trial's name."
say "The contract an adapter attaches by is docs/ATTACHING.md; §13.12 is this trial."
