//! The retired command name, kept alive for the callers that still type it.
//!
//! This name is not going away today and it is not staying for ever. It goes when nothing on the
//! box invokes it any more — one organisation's service here shells out to `projects --json` from
//! a unit that is running while you read this, and a command somebody else depends on is not ours
//! to withdraw. Until then the honest thing is to say so on every invocation, so that whoever is
//! reading a journal learns the name they depend on has a successor.
//!
//! **The notice goes to stderr and nowhere else.** That is not tidiness, it is the whole design:
//! the service above PARSES the JSON this program writes to stdout, so one byte of prose there
//! takes a running service down. `tests/the_kickoff_channel_name_is_a_compatible_change.rs` holds
//! the two commands' stdout byte-for-byte together, and holds the notice to stderr.

use std::process::ExitCode;

/// Said once, because this is one process per invocation — a script calling this in a loop gets
/// one notice per call and never a repeat inside a call.
///
/// It stops short of "run the new name instead", deliberately. On a box where the successor has
/// not been installed yet that advice fails at the prompt and leaves the reader with nothing, so
/// what is offered is the state to check and the place that says how to fix it.
fn say_it_is_retired() {
    eprintln!(
        "This command name is retired: the same program is now called kickoff-channel, and this \
         name keeps working until nothing on this box calls it."
    );
    eprintln!(
        "Do not use it in anything new — and check kickoff-channel is installed here before you \
         switch anything over (\"Build and install\" in docs/RUNNING-THE-HUB.md)."
    );
}

fn main() -> ExitCode {
    // FIRST, before the arguments are even parsed. Two reasons, and both are about a caller that
    // exists today. A `--help` or a `--version` never returns here — clap prints and exits inside
    // the parse — so a notice said afterwards would be said on no run a person reads. And one
    // script on this box merges the two streams and keeps the LAST line (`2>&1 | tail -1`), which
    // a notice at the end would become.
    say_it_is_retired();
    kickoff_channel::main()
}
