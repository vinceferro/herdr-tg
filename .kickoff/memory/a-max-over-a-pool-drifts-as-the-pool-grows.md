---
name: a-max-over-a-pool-drifts-as-the-pool-grows
description: kickoff's memory-retrieval cutoff fires if ANY hit in the top-K clears a fixed floor, so a bigger corpus or a bigger K raises the maximum while the bar stays put — measured at 44% to 79% false firing on real operator turns across 16 to 233 memories
metadata:
  type: feedback
---

Any gate of the shape "take the best of a sample, compare it to a constant" gets looser as the
sample grows. It is multiple comparisons, and it looks like a tuning problem right up until you
retune it and it comes back.

Measured 2026-09-02 against kickoff's `hook.mjs::evaluateCutoff`, using 124 real operator turns
mined from this box's transcripts:

| corpus | recall@3 | noise suppressed | fires on real turns |
| --- | --- | --- | --- |
| 16 | 0.88 | 6/6 | 44% |
| 66 | 0.88 | 4/6 | 71% |
| 233 | 0.88 | 4/6 | 79% |

**Recall never moved.** Only precision did, and the same flaw is reachable without touching the
corpus at all — fixing the corpus and raising `MEMORY_HOOK_K` from 1 to 10 moves firing 40%→47%
at 16 memories and 75%→84% at 233.

**Scoping the index is the fix that worked**: 16-own vs 233-flat is 6/6 vs 4/6 noise and 44% vs 79%
firing at identical recall. Precision for free.

**A shared "core" has to be tiny.** Own-16 plus the 54 cross-cutting memories (`feedback`,
`reference`, `user`, `convention`) from eight other projects fired on 74% of real turns — nearly the
flat corpus. Other people's working lessons are general enough to match almost anything, and that
generality is exactly what makes them noise.

**An outlier gate does NOT fix it**, and this is worth not re-discovering: replacing the floor with
"is hit #1 N standard deviations above the rest of the top-K" was worse at every setting. The top-K
IS the relevant tail, so a true hit has nothing to stand out from. An outlier test needs a
background sample of IRRELEVANT items.

Two method notes that cost real time. The transcript queries needed filtering — `<bash-input>`,
`<task-notification>` and tool blobs are not operator turns, and the first run's numbers were
meaningless until they were stripped. And the 16 recall labels were paraphrases written by the same
agent that then measured them, which is precisely
[[build-a-two-sided-corpus-before-tuning-a-classifier]] — the precision half uses real turns and is
the half to trust.

Mailed to claude-kickoff 2026-09-02; the retriever is theirs.
