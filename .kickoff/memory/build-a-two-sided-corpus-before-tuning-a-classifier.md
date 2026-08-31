---
name: build-a-two-sided-corpus-before-tuning-a-classifier
description: Four rounds of parser fixes each traded a false negative for a false positive, because the repo held exactly ONE real captured dialog and every other test screen was drawn by the agent that then chose the rule to match it
metadata:
  type: feedback
---

The sequence, and it is worth reading as a shape rather than as history: scanning top-down let prose
above a dialog hide it → scan bottom-up → a status bar *below* a dialog became the dialog → require a
resolved row → an unreadable modal plus chrome handed back the chrome → count control rows → ordinary
coloured transcript rows became dialogs.

Each fix was correct about the defect in front of it and wrong about the population.

**The cause was the corpus.** One genuine capture
(`crates/herdr-tg/tests/fixtures/opencode-permission.ansi`), seventeen hand-drawn. A rule fitted to
screens you imagined passes on screens you imagined. Round 4's sceptic broke it with three perfectly
ordinary renderings; round 5's broke the replacement using real `tmux capture-pane -p -e` output.

**How to apply:** before tuning any classifier over real-world input here, build the corpus first and
build it from BOTH sides — the things it must accept and the things it must reject — and get the
negatives from real captures, not from imagination. Then write the property as a test over the whole
corpus, so the next change cannot trade one error class for the other silently.
