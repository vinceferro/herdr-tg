# A sleep per caller is not a queue

N callers that each sleep the same interval and then retry do not take turns. They wake together,
one wins whatever they were waiting for, and the rest have already spent their wait. Give each one
a single retry and the losers fall straight through to the failure branch.

Found 2026-09-01 in `hub.rs::send_into`. Telegram's rate limit has a one-second gap and a per-minute
ceiling; the code slept once for the gap and shed on the second refusal. Correct for one sender, and
wrong from two upward. Measured on the real code: ten projects sending one message each produced
**two sends and eight sheds with sixteen of the eighteen per-minute tokens unspent**. Six bridges
opening with a question left five agents blocked and four topics bound-but-empty.

Two things fixed it, and both were needed:

* **A permit.** A `tokio::sync::Mutex` held for the whole of one caller's wait. It hands out turns in
  order, so each caller waits once rather than racing.
* **A typed refusal.** `ChatBudget::take` says WHICH limit refused — `Gap` or `Ceiling` — instead of
  returning a duration the caller guesses from. Guessing by size held for one sender too.

**The test has to be concurrent.** `pacing_waits_but_a_real_flood_is_shed` was two sequential awaits
from one task and could not see any of this;
`several_projects_sending_at_once_all_get_through` is the one that goes red (4 of 6 shed).

Same family as [[a-fix-nobody-attacked-is-a-draft]]: the single-caller case passed every gate.
