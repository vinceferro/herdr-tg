---
name: a-guard-that-fires-only-on-the-whole-leak
description: Three guards in this repo passed while the thing they named was broken — each fired only on the conjunction of two steps, and the step a maintainer would actually take was the blind one
metadata:
  type: project
---

On 18 September a sceptic pass proved three of this repo's guards blind, by planting each half of
the leak separately and watching the whole suite stay green.

- The phone-egress guard said in its own comment that it "goes red the day it moves". Hoisting the
  ring call out of its `CameFrom::Door` gate — the move, with no name change — left the entire lib
  suite green, 568 passed. It fired only when the move AND the name landed together, and its
  `for line in …` loop iterated **zero** lines, so half of it asserted nothing at all.
- The staging-name guard claimed "a later edit at either end, the name or the sorting, turns this
  red". It spelled the staged name as a literal instead of asking `where_a_result_is_staged` for it.
  Deleting the suffix push from that function — the exact edit that would make a crash leftover be
  eaten as one of his answers — left the suite green.
- The door's HTTP egress sweep, `nothing_the_door_says_over_http_names_this_machine`, made two POSTs
  and both were 401s. It had never read an ok-shape or a timeout body in its life, so it was
  structurally blind to the two fields that round had just added — and it was cited as the evidence
  those fields were safe.

**Why:** a guard is written the moment the defect is fresh, when both halves of it are in mind at
once, so a test that reproduces the whole defect feels like proof. It is not. The maintainer who
breaks it later has only one half in mind.

**How to apply:** a guard is not proved by writing it and watching it pass. Break **each** thing it
names, **separately**, and watch it go red for each one; if a plant leaves it green, it does not
guard that thing and its comment is lying. Derive names from the function that mints them rather
than spelling them out, so both ends are held together. And when a test is offered as evidence that
something is safe, read what it actually exercises before believing it — three of these were cited
as evidence by the round that shipped beside them. Related: [[a-fix-nobody-attacked-is-a-draft]].
