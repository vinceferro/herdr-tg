# An edit script that aborts writes nothing — and the commit message still claims it

A python/sed script that does several replacements and writes the file at the END loses ALL of them
when any assertion in the middle fails. The earlier replacements looked fine in the terminal; none
of them reached disk.

Found 2026-09-01. One script made four edits to `hub.rs`; the third anchor missed, the script raised,
and the file was never written. The two good edits — a settle-loop fix and a buffer bound — were
lost. `cargo check` passed, the suite passed, and the commit message claimed both. Five findings in
the next review round were sceptics discovering that a claimed fix was not in the tree, which is
worse than not having fixed it: the record was wrong.

**Write and read back after EVERY edit, not at the end.** The helper used since:

    def edit(path, old, new, tag):
        s = open(path).read()
        if old not in s: raise SystemExit(f"ANCHOR MISS [{tag}]")
        open(path, 'w').write(s.replace(old, new, 1))
        assert new in open(path).read(), f"WRITE DID NOT LAND [{tag}]"

And before writing a commit message that claims a fix, `grep` the tree for it. A green suite does not
prove an edit landed — it proves the code that IS there compiles and passes.

Same family as [[a-proof-staged-in-tmp-proves-nothing-under-privatetmp]]: the check passed because it
was not looking at what it claimed to be looking at.
