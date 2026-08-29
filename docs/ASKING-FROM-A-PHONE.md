# Asking a question that survives the trip to a phone

Rules for **agents running in a herdr pane** whose operator answers from Telegram.

This is the other half of `crates/herdr-tg/src/voice.rs`. That module governs what the *bridge*
says; this governs what the *agent* says, and the agent's words are most of what the operator
actually reads. No amount of cleaning on the bridge side can rescue a question that was written for
a 100-column terminal and a reader who has the whole transcript in view.

## The one fact that changes how you should write

**The bridge relays the tail of your rendered screen — roughly the last 12 non-blank lines, capped
at ~900 characters, with the harness's own frame stripped out.**

It cannot see your conversation history. It does not know what you were doing. The operator gets
those lines, on a phone, possibly hours later, possibly while walking.

Everything below follows from that.

## Rules

### 1. The question goes last

The tail is what gets relayed. A question followed by a rendered diff, a table, or a tool result is
a question the operator will not see — they will see the diff.

Ask, then stop.

### 2. Make it answerable without the transcript

Assume the reader has forgotten what this session was doing. One line of context, then the question.

```
The rebase drops 2 commits from the shipping branch.
Force-push anyway? [y/N]
```

Not:

```
Should I proceed?
```

### 3. Offer closed options, and say which is safe

A one-letter answer is the operator's most common reply, and it is unrecoverable if it lands on the
wrong prompt. Give them something to be right about:

- `[y/N]` — capital marks the default
- numbered options, one per line, when there are more than two
- a real harness prompt (opencode's `Allow once / Allow always / Reject`) is best of all: the bridge
  renders it as buttons, and the operator taps a literal option instead of typing

### 4. Keep it under ten lines

Longer than that and the front of it is cut off — and the front is usually your context sentence.
If the decision genuinely needs more, put the detail in a file and reference the path.

### 5. Don't ask while something is still rendering

A spinner, a progress bar, or a streaming tool result occupies the tail. Let it finish, then ask.

### 6. Say when you are done, and what you did

`done` is a push trigger too. The operator gets the tail when you finish, so end with a short
summary rather than the last line of a build log.

```
Done. 3 files changed, tests green, nothing pushed.
Next: the migration, or stop here?
```

### 7. Never put a secret in a question

The operator's answer transits Telegram's servers, and so does your question. That is an accepted
trade for asks and digests (decision D4 in `PLAN.md`), and it is only acceptable because questions
are short and redacted. Do not paste a token, a key, or a `.env` into one.

## Installing these into a project

    bash scripts/install-ask-style.sh /path/to/project

Appends a short block to that project's `AGENTS.md` (opencode) or `CLAUDE.md` (Claude Code),
between markers, so it is idempotent and removable:

    bash scripts/install-ask-style.sh --remove /path/to/project

An agent reads its project's instructions, not this file — so this document is the source and that
block is the copy. Change this file, re-run the script.
