//! SCAFFOLD STUB — build order step 1.
//!
//! The real binary (clap parse, tracing-subscriber init, `status`/`read`/`doctor`/`watch`, and the
//! exit-code map 0 ok · 1 other · 2 usage · 3 unreachable · 4 protocol skew · 5 herdr protocol
//! error) lands in build order step 9. Proof gates 2, 4, 5 and 6 all drive it.

fn main() {
    eprintln!(
        "herdr-tg {}: scaffold only — the CLI lands in slice-1 build order step 9.",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!(
        "built against herdr protocol {}",
        herdr_client::KNOWN_PROTOCOL
    );
    std::process::exit(1);
}
