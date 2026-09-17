//! The product name may move without making an adopter move all at once.
//!
//! This is the source-only seam of the `herdr-tg` -> `kickoff-channel` rename. The new name is
//! what a fresh adopter sees and invokes, while the old command remains an honest alias for the
//! units and scripts already installed on a box. State and configuration compatibility are pinned
//! in the modules that resolve them; this test pins the package and executable surface Cargo ships.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the channel crate sits two levels below the workspace root")
        .to_path_buf()
}

fn version(binary: &str) -> String {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .unwrap_or_else(|error| panic!("could not run {binary}: {error}"));
    assert!(
        output.status.success(),
        "{binary} --version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("version output is UTF-8")
}

#[test]
fn cargo_ships_the_new_command_and_keeps_the_old_one_as_an_alias() {
    // `env!` rather than `option_env!`: Cargo defines these for every integration test, and a
    // build that did not define them has not built the two commands this test exists to pin —
    // failing to compile is the honest outcome, not a panic with a message after the fact.
    let canonical = env!("CARGO_BIN_EXE_kickoff-channel");
    let legacy = env!("CARGO_BIN_EXE_herdr-tg");

    assert!(version(canonical).starts_with("kickoff-channel "));
    assert!(version(legacy).starts_with("kickoff-channel "));
}

#[test]
fn the_current_readme_leads_with_the_new_product_name() {
    let readme =
        std::fs::read_to_string(workspace_root().join("README.md")).expect("README.md is readable");
    assert!(
        readme.starts_with("# Kickoff Channel\n"),
        "README.md must introduce the current product, not its former repository name"
    );
}
