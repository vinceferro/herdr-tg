---
name: cargo-needs-a-real-tmpdir
description: An agent session here inherits TMPDIR as the literal string "%h/.cache/tmp", so 7 herdr-client transport tests fail for a reason that has nothing to do with the change — and the pre-commit hook fails the same way
metadata:
  type: project
---

`TMPDIR` arrives as an unexpanded systemd specifier. Anything that mktemps a socket dir then dies
with `NotFound` on a path containing a literal `%h`, and it reads as 7 genuine transport failures in
`herdr-client`. It is not.

Every cargo command here needs all three parts:

```
env -u RUSTUP_TOOLCHAIN TMPDIR=<a real absolute dir> PATH="$HOME/.cargo/bin:$PATH" cargo <...>
```

`rust-toolchain.toml` and `.kickoff/lefthook-kickoff.yml` already explain the first two (mise hides
cargo and overrides the toolchain file). The TMPDIR part is written down nowhere in the repo.

**The trap that costs the most time:** `git commit` runs the same five gates through the hook, in
*your* environment. Without the override the commit is refused with seven red tests you did not
break. Prefix the commit too: `TMPDIR=<real dir> git commit -F <file>`.
