# Canonical herd projection. Applied to BOTH sides of the proof, then `jq -S`.
# DROPPED (volatile or without v1 product meaning) — printed by proof-slice1.sh on failure:
#   revision · state_change_seq · scroll · screen_detection_skipped
#   terminal_title · terminal_title_stripped · layouts · every null
# KEPT (the proof would be vacuous without these):
#   agent_status · focused · focused_*_id · ids · labels · numbers · counts
#   cwd · foreground_cwd · agent · display_agent · agent_session · version · protocol
#   tokens · title · state_labels · interactive_ready · launch_pending
#
# WHY THOSE LAST FIVE MOVED OUT OF THE DROP LIST (review minor, closed 2026-08-28).
# They are modelled in proto::model but have never been seen carrying a value — not live, not in
# any checked-in fixture. While they were ALSO drop-listed they were invisible to BOTH proof
# layers at once: a wrong type would have been deleted from both sides before the compare, so
# nothing in this repo would have noticed. Keeping them costs exactly nothing today (the live
# census is 0 occurrences of each) and buys the same safe failure mode `display_agent` and `name`
# already have: the day herdr starts emitting one, gate 3 goes RED rather than silently dropping
# it. The offline half of the same finding is
# crates/herdr-client/tests/golden.rs::unobserved_optional_fields_decode_from_bytes.
#
# terminal_title / terminal_title_stripped STAY dropped: unlike the five above they are observed
# on every pane and genuinely volatile (opencode retitles every 20-40 s), so keeping them would
# make gate 3 flap. golden.rs pins their decoding against the fixture instead.
def scrub:
  walk(
    if type == "object" then
      del(.revision, .state_change_seq, .scroll, .screen_detection_skipped,
          .terminal_title, .terminal_title_stripped)
      | with_entries(select(.value != null))
    else . end
  );
.result.snapshot | del(.layouts) | scrub
