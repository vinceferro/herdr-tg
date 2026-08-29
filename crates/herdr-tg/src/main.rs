//! `herdr-tg` — slice 1: a read-only CLI over the herdr session daemon.
//!
//! # What this binary may and may not do
//!
//! Five subcommands, all **read-only**: `status`, `read`, `doctor`, `watch`, and (slice 2) `serve`,
//! which puts the same read-only surface behind a Telegram bot. There is deliberately no sixth. herdr's three write RPCs — the ones that type real keystrokes into the operator's
//! real terminals — exist in `herdr-client` as typed, mock-tested code, and this crate does not so
//! much as NAME them. That is machine-checked: `herdr-client`'s `tests/no_live_write_call_site.rs`
//! greps this whole source tree and fails the suite on any mention, a doc comment included (which
//! is why they are described here rather than spelled). The binary is what a timer, a cron job, or
//! (from slice 2) a Telegram message can reach; the write path must not be reachable that way
//! until the operator decides it is.
//!
//! Reads are `source: "visible"` only, by construction: [`herdr_client::HerdrClient`] exposes no
//! other read source, and `recent` / `recent_unwrapped` harvest-scroll the operator's real viewport.
//!
//! # The `--json` surfaces are the proof surface
//!
//! `status --json` and `read --json` emit the **full RPC envelope re-serialized from the client's
//! own typed structs** — `{"id":…,"result":{"type":…,"<key>":…}}` — never a passthrough of the
//! bytes that came off the socket. That distinction is the whole point: proof gate 3 diffs this
//! output against `herdr api snapshot`, so a passthrough would make the gate prove nothing about
//! the decoder. Any field herdr emits that the client does not model drops out of our side and
//! turns the diff red. That is the drift alarm.
//!
//! The `id` in that envelope is **synthesised here**, not the id that went out on the wire: the
//! client generates request ids internally and never surfaces them, because herdr blanks the id to
//! `""` on a parse error and echoes it on a semantic one, so correlating on it is a bug waiting to
//! happen. Nothing in the proof reads it (`normalize.jq` begins at `.result.snapshot`).
//!
//! # Exit codes
//!
//! `0` ok · `1` other · `2` usage (clap) · `3` herdr unreachable · `4` protocol skew ·
//! `5` herdr protocol error. Proof gate 6 asserts **3** (missing socket) and **4** (a server
//! speaking protocol 19) exactly, and PLAN.md's failure table branches on them.

mod audit;
mod bot;
mod cmd;
mod config;
mod deliver;
mod notify;
mod render;
mod routing;

use std::io::{IsTerminal, Write};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use herdr_client::{HerdrClient, HerdrError};

/// Steer a herd of coding agents from the socket herdr already speaks.
#[derive(Debug, Parser)]
#[command(name = "herdr-tg", version, about, long_about = None)]
struct Cli {
    /// Socket to dial. Overrides `$HERDR_SOCKET_PATH` and the `~/.config/herdr/herdr.sock`
    /// fallback.
    ///
    /// Proof gate 6 drives the failure paths through `$HERDR_SOCKET_PATH` rather than this flag,
    /// but the flag is what makes a probe socket usable without mutating the environment.
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// The herd: workspaces, panes, agent status. Human table by default.
    ///
    /// `--json` is the PROOF surface: the full `{"id","result":{"type":"session_snapshot",
    /// "snapshot":…}}` envelope, re-serialized from the client's own typed structs. The envelope
    /// (including the `result` wrapper) is mandatory, not decoration — `scripts/normalize.jq`
    /// begins at `.result.snapshot`, and a bare snapshot makes the harness report "produced no
    /// parseable JSON", which reads as a crash rather than as a shape mismatch.
    Status {
        /// Emit the full RPC envelope instead of the human table.
        #[arg(long)]
        json: bool,
        /// Restrict the view to one workspace, by id (`wA`) or by label (`desktop-lab`).
        ///
        /// D2 is one bot per workspace, so this is the shape slice 2 needs. It filters the decoded
        /// snapshot client-side rather than calling `pane.list`, so that `--json` still emits a
        /// complete, valid `session_snapshot` envelope.
        #[arg(long, value_name = "ID_OR_LABEL")]
        workspace: Option<String>,
    },

    /// Read a pane's visible screen. Text to stdout, byte-for-byte.
    ///
    /// Always `source: "visible"` — the client offers nothing else. `--json` emits the full
    /// `{"type":"pane_read","read":…}` envelope so the proof can assert `source == "visible"` and
    /// `truncated == false`; without it the text goes to stdout raw, with no added newline, so it
    /// compares byte-identical to `herdr pane read --source visible --format text`.
    Read {
        /// Pane id, e.g. `wA:p1`.
        pane: String,
        /// Ask for the last N lines instead of the whole visible screen.
        ///
        /// Still clamped to the viewport by herdr, so this cannot trip the scroll harvest. Not
        /// zero: `lines=0` returns an empty string with `truncated:true`, a silently useless read.
        #[arg(long, value_name = "N")]
        lines: Option<NonZeroU32>,
        /// Emit the full RPC envelope instead of the raw text.
        #[arg(long)]
        json: bool,
    },

    /// Is this bridge's view of herdr still valid? Version, protocol, capabilities, socket.
    ///
    /// The one command that exercises the version policy outside startup, and the one an operator
    /// can run from a phone-driven session when nothing else answers. Exits **4** when the server
    /// is older than the minimum this client supports.
    Doctor {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Decode the event stream for one pane.
    ///
    /// `--once` opens ONE filtered subscription pinned to `--expect-status`, prints the first
    /// matching decoded event, and exits — herdr replays a pane's current status at subscribe time
    /// when the filter already matches it, which is what makes this deterministic and read-only.
    /// That is proof gate 5. Without `--once` it prints decoded events until the stream closes.
    Watch {
        /// Pane id to subscribe to. There is no global agent-status subscription in protocol 20 —
        /// `pane.agent_status_changed` requires a `pane_id`.
        #[arg(long)]
        pane: String,
        /// Print the first matching event and exit.
        #[arg(long)]
        once: bool,
        /// Pin the subscription to this status (`idle` · `working` · `blocked` · `done` ·
        /// `unknown`). A matching filter replays at subscribe time; an unfiltered subscription
        /// fires only on a real transition and never replays.
        #[arg(long, value_name = "STATUS")]
        expect_status: Option<String>,
        /// How long to wait for a matching event. `--once` only; without it the stream runs until
        /// the server closes it.
        #[arg(long, default_value = "5000", value_name = "MS")]
        timeout_ms: u64,
    },

    /// Run the Telegram bridge: long-poll the Bot API and answer the allowlisted chat.
    ///
    /// Still read-only (slice 2): `/status`, `/doctor`, `/help`. The bridge binds nothing — it
    /// dials out to api.telegram.org and to the local herdr socket, so the box needs no ingress
    /// (D7). The token comes from `$HERDR_TG_TOKEN`, never from the config file; the chat-id
    /// allowlist is the identity gate and fails closed, so an empty allowlist answers nobody.
    Serve {
        /// Structure only — workspace, allowlist, socket. Never the token.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("herdr-tg: could not start the async runtime: {err}");
            return ExitCode::from(1);
        }
    };

    let result = runtime.block_on(run(cli));

    // stdout is line-buffered into a pipe; proof gate 4 compares it with `cmp`, so a lost tail
    // would look like a decoder bug. Flush explicitly rather than trusting the runtime teardown.
    let flushed = std::io::stdout().flush();

    match (result, flushed) {
        (Err(err), _) => {
            report(&err);
            ExitCode::from(exit_code(&err) as u8)
        }
        (Ok(()), Err(err)) => {
            eprintln!("herdr-tg: could not flush stdout: {err}");
            ExitCode::from(1)
        }
        (Ok(()), Ok(())) => ExitCode::SUCCESS,
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let socket_override = cli.socket.clone();
    let client = connect(cli.socket)?;
    match cli.cmd {
        Cmd::Status { json, workspace } => {
            cmd::status::run(&client, json, workspace.as_deref()).await
        }
        Cmd::Read { pane, lines, json } => cmd::read::run(&client, &pane, lines, json).await,
        Cmd::Doctor { json } => cmd::doctor::run(&client, json).await,
        Cmd::Watch {
            pane,
            once,
            expect_status,
            timeout_ms,
        } => cmd::watch::run(&client, &pane, once, expect_status.as_deref(), timeout_ms).await,
        Cmd::Serve { config } => {
            let cfg = config::Config::load(config.as_deref())?;
            // `--socket` still wins; the config's socket is the next fallback, so a probe session
            // can be targeted from the file the unit already reads.
            let client = match (&socket_override, &cfg.socket) {
                (None, Some(path)) => HerdrClient::new(path.clone()),
                _ => client,
            };
            bot::serve(cfg, client).await
        }
    }
}

/// `--socket`, else `$HERDR_SOCKET_PATH`, else `$HOME/.config/herdr/herdr.sock`.
///
/// The fallback is not a convenience: every `HERDR_*` variable is pane-injected, so the production
/// `systemd --user` unit sees none of them. Proof gate 2 runs this binary under `env -i` with an
/// empty PATH and no `HERDR_*` to prove the fallback is what actually reaches the socket — and, at
/// the same time, that a client which shelled out to `herdr` would get rc=127 instead.
fn connect(socket: Option<PathBuf>) -> anyhow::Result<HerdrClient> {
    match socket {
        Some(path) => Ok(HerdrClient::new(path)),
        None => Ok(HerdrClient::from_env()?),
    }
}

/// `RUST_LOG` if set, `warn` otherwise, always to **stderr**.
///
/// stdout is a data channel here (`read` is compared byte-for-byte, `--json` is parsed by `jq`), so
/// nothing diagnostic may ever land on it.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}

/// The documented exit code for a failure.
///
/// [`HerdrError::exit_code`] owns the mapping (3 unreachable · 4 protocol skew · 5 herdr protocol
/// error), so the binary and the library cannot drift apart. Anything else is `1`.
fn exit_code(err: &anyhow::Error) -> i32 {
    err.downcast_ref::<HerdrError>()
        .map_or(1, HerdrError::exit_code)
}

/// One line on stderr, and never a panic.
///
/// Only the top-level `Display` is printed. Every [`HerdrError`] variant already embeds its own
/// source in its message (`herdr unreachable: <path> (<io error>)`), so walking the chain here
/// would print the cause twice — and gate 6 greps this line for `herdr unreachable`. The full
/// chain goes to the debug log for anyone who has turned `RUST_LOG` up.
fn report(err: &anyhow::Error) {
    eprintln!("herdr-tg: {err}");
    for cause in err.chain().skip(1) {
        tracing::debug!(cause = %cause, "caused by");
    }
}
