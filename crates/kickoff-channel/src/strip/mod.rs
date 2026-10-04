//! The work strip — this product's rendering of the lane board another organisation frames.
//!
//! The strip is a CONSUMER of the omnibus bus: it renders `wip` frames (one row per lane —
//! agent, lane, state, last beat, proof status) and composes exactly one dispatch affordance
//! (pick an agent and a task file, emit the `dispatch` frame, render the receipt that comes
//! back). It starts nothing and shells out to nothing — the frame owner's dispatchd executes,
//! behind their hub and their dispatch-capable token — so the property `nothing inbound can
//! start a process` holds here for the plainest of reasons: there is no process-starting code
//! in this module to guard.
//!
//! ## The seam, and why it is a replay for now
//!
//! The frame-owning half (the `wip` frame kind, the lane-relay that mints rows, the dispatchd
//! that answers) is being built elsewhere; nothing live emits these frames yet. The strip
//! therefore reads from a **fixture seam** — an NDJSON file of frames replayed at the relay's
//! own cadence — behind a narrow interface, so the live connection replaces the replay without
//! touching the board, the renderer or the picker. The frames themselves are byte-matched to
//! the spec, so the seam carries real shapes, not simplifications of them.
//!
//! ## The one verb the strip accepts while it runs
//!
//! Typing `d` (or `dispatch`) starts the pick; `q` leaves. Everything else a person types is
//! ignored — the strip is a surface for reading and for ONE command, and a surface that grew
//! verbs by accretion would be a second product nobody decided on.

pub(crate) mod board;
pub(crate) mod frames;
pub(crate) mod picker;
pub(crate) mod render;
pub(crate) mod seam;

use std::collections::VecDeque;
use std::io::{BufRead as _, IsTerminal, Write as _};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use board::{Applied, Board};
use frames::{Down, now_secs};
use render::Outlook;
use seam::FixtureSeam;

/// How the strip was asked to run.
pub(crate) struct Options {
    /// The NDJSON replay of down-frames.
    pub replay: PathBuf,
    /// Where dispatch frames are appended. Without one, the strip says so and the pick refuses.
    pub outbox: Option<PathBuf>,
    /// Render the first board and leave — the deterministic, pipeable mode.
    pub once: bool,
    /// The wait between replayed frames: the relay's own coalescing floor.
    pub cadence: Duration,
    /// Silence longer than this is rendered as staleness, never hidden.
    pub stale_after: Duration,
    /// Pin the next dispatch's id instead of minting one — for scripted replays whose receipt
    /// names the id back. A live seam mints its own.
    pub dispatch_id: Option<String>,
}

/// Run the strip until the replay ends, `q` is typed, or (in `--once`) the first board lands.
pub(crate) async fn run(options: Options) -> anyhow::Result<()> {
    let mut seam = FixtureSeam::open(&options.replay, options.outbox.clone(), options.cadence)?;
    let mut board = Board::default();
    // Dispatches sent and not yet answered. Their ids are what receipts correlate on, and an
    // answer that never comes is reported as never coming — the say/ask law: a command's
    // silence is a fact about the command, not something to drop.
    let mut pending: Vec<String> = Vec::new();
    // Receipts that arrived before the dispatch they answer (a replay can do this; a live seam
    // cannot, and holding a stranger's receipt briefly costs nothing). Matched the moment the
    // command lands.
    let mut unmatched: VecDeque<frames::DispatchReceipt> = VecDeque::new();
    let mut notes: VecDeque<String> = VecDeque::new();

    let terminal = std::io::stdout().is_terminal();

    // The keyboard, on its own thread: stdin is blocking and the strip's loop is async, and
    // the alternative — async stdin — is a dependency this repo does not carry for a surface
    // whose input is one line at a time.
    let (lines_tx, lines_rx) = tokio::sync::mpsc::channel::<String>(8);
    // `Option` so the arm below can park once the keyboard goes away instead of polling a
    // closed receiver for ever.
    let mut lines_rx = Some(lines_rx);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if lines_tx.blocking_send(line).is_err() {
                return; // the strip left before this line was typed; nothing is waiting for it
            }
        }
    });

    let mut last_frame_at: Option<Instant> = None;
    let mut drawn_stale = false;
    let mut drawn_second: u64 = 0;

    loop {
        let tick = tokio::time::sleep(Duration::from_millis(250));
        tokio::select! {
            down = seam.next_down() => {
                let Some(down) = down else {
                    // The replay is exhausted. The board that remains is rendered once, as the
                    // last known, and every dispatch still unanswered is said to be
                    // unanswered — an operator left holding "pending" forever would be an
                    // operator lied to about the kindest thing in this product: what happened.
                    for id in pending.drain(..) {
                        note(&mut notes, format!("receipt {id}: none before the replay ended"));
                    }
                    draw(&board, &notes, last_frame_at, true, &options, terminal);
                    return Ok(());
                };
                match down {
                    Down::Wip(frame) => match board.apply(&frame) {
                        Applied::Snapshot => {
                            last_frame_at = Some(Instant::now());
                            draw(&board, &notes, last_frame_at, false, &options, terminal);
                            drawn_stale = false;
                            drawn_second = now_secs().max(0) as u64;
                            if options.once {
                                return Ok(());
                            }
                        }
                        Applied::Refused(why) => {
                            note(&mut notes, format!("refused a frame: {why}"));
                            draw(&board, &notes, last_frame_at, false, &options, terminal);
                        }
                    },
                    Down::Ack(receipt) => {
                        if let Some(position) = pending.iter().position(|id| *id == receipt.ref_id) {
                            pending.remove(position);
                            note(&mut notes, picker::receipt_line(&receipt));
                            draw(&board, &notes, last_frame_at, false, &options, terminal);
                        } else {
                            // A receipt with no command waiting for it. On a live seam this is
                            // another consumer's ack and harmless to hold; on a replay it is
                            // USUALLY this strip's own receipt arriving ahead of its pick —
                            // the replay has no causality, so the scripted ack can beat the
                            // piped keyboard. Dropping it would lose a receipt an operator is
                            // waiting for, so it is kept and offered again when the dispatch
                            // lands, exactly the way a question the machine could not decide
                            // on is kept. The ring is short: the last few receipts are the
                            // ones that can still matter.
                            if unmatched.len() >= 4 {
                                unmatched.pop_front();
                            }
                            unmatched.push_back(receipt);
                        }
                    }
                }
            }
            line = async {
                match &mut lines_rx {
                    Some(receiver) => receiver.recv().await,
                    // The keyboard went away (stdin closed under a pipe). Nothing further can
                    // arrive; a closed receiver polled forever would spin this loop, so this
                    // arm parks instead of answering.
                    None => std::future::pending().await,
                }
            } => {
                let Some(line) = line else {
                    lines_rx = None;
                    continue;
                };
                let typed = line.trim();
                if typed == "q" || typed == "quit" {
                    return Ok(());
                }
                if typed == "d" || typed == "dispatch" {
                    if options.once {
                        continue; // --once renders one board and leaves; picking is not offered.
                    }
                    if !seam.can_dispatch() {
                        note(&mut notes, "dispatch is not wired on this seam (no outbox was named); nothing was sent".to_owned());
                        draw(&board, &notes, last_frame_at, false, &options, terminal);
                        continue;
                    }
                    if let Some(sent) = pick_and_emit(
                        &mut seam,
                        &board,
                        &mut lines_rx,
                        options.dispatch_id.clone(),
                        &mut notes,
                    )
                    .await
                    {
                        // A receipt that arrived ahead of this dispatch (only a replay can do
                        // it) answers it the moment it exists; anything else waits for the
                        // seam to speak.
                        if let Some(position) =
                            unmatched.iter().position(|receipt| receipt.ref_id == sent)
                        {
                            let receipt = unmatched.remove(position).expect("just positioned");
                            note(&mut notes, picker::receipt_line(&receipt));
                        } else {
                            pending.push(sent);
                        }
                    }
                    draw(&board, &notes, last_frame_at, false, &options, terminal);
                }
                // Anything else typed is not a verb this surface has, and inventing silent
                // meanings for stray lines is how a reading surface grows verbs by accident.
            }
            _ = tick => {
                let stale = last_frame_at
                    .is_some_and(|at| at.elapsed() >= options.stale_after);
                let second = now_secs().max(0) as u64;
                // Redraw when the honesty of the board changed (fresh → stale), and — on a
                // terminal only — once a second so the "Ns ago" line ages in front of the
                // person reading it. Piped output redraws on change alone: a log that grows a
                // board a second is a log nobody reads.
                if stale != drawn_stale || (terminal && second != drawn_second) {
                    draw(&board, &notes, last_frame_at, false, &options, terminal);
                    drawn_stale = stale;
                    drawn_second = second;
                }
            }
        }
    }
}

/// One prompt-driven pick: agent, task, reason; then the frame, handed to the seam.
///
/// Returns the id of the dispatch that was sent, if one was. Every refusal is a note rather
/// than an error — a pick that goes wrong is a thing the operator reads on the strip, not a
/// reason for the strip to die.
async fn pick_and_emit(
    seam: &mut FixtureSeam,
    board: &Board,
    lines: &mut Option<tokio::sync::mpsc::Receiver<String>>,
    pinned_id: Option<String>,
    notes: &mut VecDeque<String>,
) -> Option<String> {
    let Some(lines) = lines else {
        note(
            notes,
            "there is no keyboard here; the pick was abandoned".to_owned(),
        );
        return None;
    };

    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "dispatch: spawn a lane");
    let agents = board.agents();
    if agents.is_empty() {
        let _ = writeln!(
            out,
            "  (no agents on the strip yet — type any name and the executor will judge it)"
        );
    } else {
        let offered = agents
            .iter()
            .enumerate()
            .map(|(index, agent)| format!("{}) {agent}", index + 1))
            .collect::<Vec<_>>()
            .join("  ");
        let _ = writeln!(out, "  agents: {offered}");
    }

    let agent_line = ask("  agent (number or name): ", lines, &mut out).await?;
    let task_line = ask(
        "  task (an absolute file path, or --Text for a task typed here): ",
        lines,
        &mut out,
    )
    .await?;
    let reason_line = ask("  reason (one line, can be empty): ", lines, &mut out).await?;
    drop(out);

    let id = pinned_id.unwrap_or_else(seam::mint_dispatch_id);
    let offered = board.agents();
    match picker::compose_spawn(
        &offered,
        id.clone(),
        &agent_line,
        &task_line,
        &reason_line,
        |path| path.is_file(),
    ) {
        Ok(frame) => match seam.emit(&frame) {
            Ok(()) => {
                note(
                    notes,
                    format!("dispatch {} sent to the seam — receipt pending", frame.id),
                );
                Some(frame.id)
            }
            Err(err) => {
                note(notes, format!("dispatch {id} was not sent: {err}"));
                None
            }
        },
        Err(err) => {
            note(notes, err.to_string());
            None
        }
    }
}

/// Print one prompt and take one line. `None` means the keyboard went away mid-pick, which
/// abandons the pick rather than composing a frame from half an answer — a dispatch frame is
/// a command, and commands are not filled in by guesswork.
async fn ask(
    prompt: &str,
    lines: &mut tokio::sync::mpsc::Receiver<String>,
    out: &mut std::io::StdoutLock<'static>,
) -> Option<String> {
    let _ = out.write_all(prompt.as_bytes());
    let _ = out.flush();
    lines.recv().await
}

/// Render the strip now: clear-and-redraw on a terminal, board-after-board when piped, with
/// the recent notes (refusals, dispatches, receipts) carried under the board so a receipt that
/// arrives three frames later is still on the screen a person is reading.
fn draw(
    board: &Board,
    notes: &VecDeque<String>,
    last_frame_at: Option<Instant>,
    feed_ended: bool,
    options: &Options,
    terminal: bool,
) {
    let outlook = Outlook {
        now_secs: now_secs(),
        secs_since_frame: last_frame_at.map(|at| at.elapsed().as_secs() as i64),
        feed_ended,
        stale_after_secs: options.stale_after.as_secs(),
        _marker: std::marker::PhantomData,
    };
    let mut text = render::strip(board, &outlook);
    for note in notes {
        text.push_str("  · ");
        text.push_str(note);
        text.push('\n');
    }

    let mut out = std::io::stdout().lock();
    if terminal {
        // Clear-and-home rather than scroll: the strip is a board, and a board that scrolls
        // every refresh buries the lanes under their own history.
        let _ = out.write_all(b"\x1b[2J\x1b[H");
    } else {
        let _ = out.write_all(b"\n");
    }
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

/// Keep the note tail short: the strip is a board, not a log, and the last few things that
/// happened are what an operator needs beside it — the rest live wherever the seam's outbox
/// and the executor's own records live.
fn note(notes: &mut VecDeque<String>, line: impl Into<String>) {
    if notes.len() >= 5 {
        notes.pop_front();
    }
    notes.push_back(line.into());
}
