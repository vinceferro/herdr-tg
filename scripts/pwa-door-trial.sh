#!/usr/bin/env bash
# The hermetic PWA-door trial — one real hub, the real kickoff-door binary, no Telegram.
#
#     bash scripts/pwa-door-trial.sh
#
# This is the door another org runs before installing anything. It stands up a real hub over a
# real Unix socket, connects a real bridge speaking hub-proto, and then spawns the REAL gateway
# binary — `kickoff-door`, the one program in this workspace that listens — on a loopback port
# of its own. A scripted client shaped like the PWA's own (its anchor poll, its SSE subscribe,
# its write POSTs) then proves the whole loop.
#
# TWO round trips run, because there are two planes and the door is in front of both:
#
#   * with Telegram counted instead of called — the ask envelope arrives on the stream stamped
#     with its conversation and lane, a POSTed choice reaches the bridge's socket as a `choice`
#     frame, the POST is answered from the hub's own result file, a refusing bridge's word lands
#     on the ring, a POSTed message naming a lane reaches that lane's session, a second
#     conversation's answers reach it and nobody else, the 401 is byte-identical, replay from an
#     old cursor is exact, and nothing on the HTTP wire names this machine;
#
#   * with NO Telegram surface in the process at all — the plane the box runs on now that the
#     bot token is revoked. The hub is built the way `kickoff-channel serve --to app` builds it, and
#     trial waits on the ring's own sequence rather than on anything a carrier was asked to do.
#     It proves a question reaches the ring stamped with its conversation and its lane, that
#     `GET /v1/events` serves it, that a POSTed choice reaches the session that asked and nobody
#     else, that the POST is answered from the hub's verdict and not from optimism, that the name
#     the POST answered with is the name the ring echoes, that the question stops being open and
#     the hub's own retirement is on the ring, and the same wire law.
#
# Nothing here installs anything, sends anything to Telegram, or touches a real state home:
# the token is written into the trial's own throwaway state directory the way `kickoff-channel
# door-token` would leave it (the verb itself is unit-held, and it mints into the real state
# home this trial refuses to go near).
#
# Like the fleet trial, this is a thin wrapper: each trial is one `#[ignore]`d test inside the
# hub's own test module, because that is the only place either hub can be built at all. What this
# script adds over `cargo test` is the environment, a build of the gateway binary onto PATH (the
# tests start it BY NAME — a fixed program no inbound string chooses, which is why it is on the
# spawn guard's allowlist), and the refusals below.
#
# HERMETIC, and it proves it rather than promising it:
#   * its own TMPDIR (under /tmp/herdr-gate-tmp, made if it vanished), made fresh and removed
#     at the end — every socket, registry, ledger, ring, drop and token the trial writes lives
#     under it;
#   * it refuses, before it makes anything, to run with its work pointed at the real state
#     directory;
#   * it gives the run a state home of its own and fails if ANYTHING appears in it, which is
#     what catches the one way a hermetic run leaks: code that works out where to keep its
#     state from the environment instead of being told;
#   * it lists ~/.local/state/herdr-tg before and after and fails if a path appeared or
#     vanished.
#
# It touches no systemd unit, sends nothing to Telegram, and enrols nothing on this box. It
# binds one loopback TCP port for the life of the run — the gateway under trial.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REAL_STATE="${XDG_STATE_HOME:-$HOME/.local/state}/herdr-tg"
# Both round trips, run one after the other and each checked for having RUN — see the refusal at
# the bottom of the loop.
THE_TRIALS="
hub::tests::the_pwa_s_door_round_trip_reaches_the_hub_and_back_through_the_real_gateway
hub::tests::the_pwa_s_door_round_trip_reaches_a_hub_that_has_no_telegram_surface_at_all
"

say() { printf '%s\n' "$*"; }
die() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

[ $# -eq 0 ] || die "usage: pwa-door-trial.sh — it takes no arguments"

[ -x "$HOME/.cargo/bin/cargo" ] || command -v cargo >/dev/null 2>&1 ||
  die "cargo is not on PATH; mise shims hide it, which is why this script sets PATH itself"

# The gate prefix the whole repo keeps, with the TMPDIR this door's gates are run under — made
# again if it vanished, because a directory another process may clean is not one to depend on.
GATE_TMP=/tmp/herdr-gate-tmp
mkdir -p "$GATE_TMP" || die "could not make $GATE_TMP, which the gates run under"

# Fail closed on being pointed at the real thing, BEFORE anything is made. Every one of these has
# a way to end up naming the operator's own state directory — an exported variable, a symlink, a
# home that is itself a link — and a trial that wrote into it would mint a door token into a real
# state home.
ROOT="${TMPDIR_ROOT:-$GATE_TMP}"
[ -d "$ROOT" ] || die "there is nowhere to work: $ROOT is not a directory"
case "$ROOT" in
  "$REAL_STATE"|"$REAL_STATE"/*) die "the run directory would be inside the real state directory" ;;
esac
if [ -e "$REAL_STATE" ] && [ "$(readlink -f "$ROOT")" = "$(readlink -f "$REAL_STATE")" ]; then
  die "the run directory would resolve to the real state directory"
fi

# A run of its own.
WORK="$(mktemp -d "$ROOT/pwa-door-trial.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# The trap, and it is the strong half of the proof. Everything in the trial is TOLD where its
# state lives; this is the state home anything that worked it out from the environment instead
# would land in, so a single entry here is a leak caught by name rather than inferred from a
# directory that happens not to have changed.
AMBIENT="$WORK/ambient-state"
mkdir -p "$AMBIENT"

# And the real state directory itself, listed by PATH — not by size or by modification time, for
# the reason the fleet trial's comment gives: a serving hub re-stamps files that are already
# there, and calling that the trial's doing would be a false alarm on exactly the box this
# fixture is for. A trial that leaked would CREATE something.
paths_under_the_real_state() {
  [ -d "$REAL_STATE" ] || { printf 'absent\n'; return; }
  find "$REAL_STATE" -printf '%P\n' 2>/dev/null | LC_ALL=C sort
}
BEFORE="$(paths_under_the_real_state)"

# The gateway binary, built and put on PATH — the test starts it by name, and the spawn guard
# holds that name to this one arrangement.
TARGET="${CARGO_TARGET_DIR:-$REPO/target}"
say "Building kickoff-door…"
env -u RUSTUP_TOOLCHAIN TMPDIR="$GATE_TMP" PATH="$HOME/.cargo/bin:$PATH" \
  cargo build -q -p kickoff-channel --bin kickoff-door ||
  die "the gateway binary did not build"
[ -x "$TARGET/debug/kickoff-door" ] ||
  die "the gateway binary is not at $TARGET/debug/kickoff-door — if CARGO_TARGET_DIR is set in \
your environment, this script honours it; make sure the path above is the one cargo built into"

say "Running the hermetic PWA-door trials — one hub each, the real gateway, no Telegram…"
say
# One cargo run per trial rather than one run with two filters, so that the "it RAN" refusal
# below is made of EACH trial's own result line. Two filters share one "2 passed", and a trial
# that had been renamed away would hide behind the other one's pass — which is the same shape as
# a filter matching nothing, arrived at by a longer road.
RC=0
N=0
for THE_TRIAL in $THE_TRIALS; do
  N=$((N + 1))
  # Kept as well as shown, because an exit code is not enough to say the trial RAN — see below.
  SAID="$WORK/what-it-said-$N"
  set +e
  ( cd "$REPO" && env -u RUSTUP_TOOLCHAIN TMPDIR="$WORK" XDG_STATE_HOME="$AMBIENT" \
      PATH="$TARGET/debug:$HOME/.cargo/bin:$PATH" \
      cargo test -q -p kickoff-channel --lib -- --ignored --nocapture --exact "$THE_TRIAL" ) 2>&1 | tee "$SAID"
  THIS=${PIPESTATUS[0]}
  set -e
  [ "$THIS" -eq 0 ] || RC=$THIS
done

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

[ "$RC" -eq 0 ] || die "a PWA-door trial failed; the lines above say which property"

# And they RAN. A filter that matches nothing is not an error to `cargo test`: it prints "0 passed"
# and exits 0, so a trial that had been renamed or moved reported a clean run while running no
# test whatever — the exact shape found in the installer once already.
N=0
for THE_TRIAL in $THE_TRIALS; do
  N=$((N + 1))
  grep -qE '^test result: ok\. 1 passed' "$WORK/what-it-said-$N" ||
    die "$THE_TRIAL did not run: nothing here matched its name, and a filter that matches nothing \
is reported as a pass. Whoever renamed or moved it must change the name at the top of this script; \
until then this run proves nothing"
done

say
say "Nothing was kept outside the run's own directory, nothing was spent, nothing was sent, and"
say "the real state directory holds what it held. What this did NOT prove is written at the top of"
say "each test, in crates/kickoff-channel/src/hub/tests.rs — search for the trial's name."
say "The shapes the door speaks are pinned by the PWA org's own stub suite, in their repo; this"
say "trial is the real-binary half of that agreement."
