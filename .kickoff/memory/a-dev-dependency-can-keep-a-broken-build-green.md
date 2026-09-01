# A dev-dependency's features can keep a broken build green

`cargo test` compiles dev-dependencies; `cargo build` does not. Cargo unifies features across
everything in the graph, so a feature a dev-dependency turns on is a feature the LIBRARY code
silently gets — but only under `cargo test`.

Found 2026-09-01 in `lock.rs`. It declared `rustix` with `default-features = false, features =
["fs"]`, which does not include `std`, so there is no `AsFd` impl for `std::fs::File` and no
`From<Errno> for io::Error`. It compiled anyway, because the dev-dependency `tempfile` pulls rustix
with `std` on. The whole suite was green while `cargo build` would have failed.

The five gates did not catch it: `cargo test`, `cargo clippy --all-targets` and `cargo doc` all
build with dev-dependencies present. Only `cargo build` (or `cargo check` with no `--all-targets`)
sees the real feature set.

**When adding a dependency with `default-features = false`, run `cargo check -p <crate>` — no
`--all-targets` — before believing the suite.** Name every feature the library code needs, even the
ones that look like they must be on by default.

Same family as [[a-proof-staged-in-tmp-proves-nothing-under-privatetmp]]: the harness supplied
something the real thing does not, and the pass meant nothing.
