//! Backwards-compatible command name for existing installs.

use std::process::ExitCode;

fn main() -> ExitCode {
    kickoff_channel::main()
}
