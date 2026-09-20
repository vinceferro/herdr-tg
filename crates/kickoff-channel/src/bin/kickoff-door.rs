//! The door's HTTP edge — see `src/gateway.rs` for what it is and what it may never do.
//!
//! A binary of its own on purpose: the hub still binds nothing, and everything that listens
//! lives in this one program, where the guards and the docs can say so about exactly one file
//! set.

use std::process::ExitCode;

fn main() -> ExitCode {
    kickoff_channel::gateway::main()
}
