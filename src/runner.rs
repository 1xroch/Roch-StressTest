// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Starting a test, watching it, and deciding when it is over.
//!
//! Every tool is supervised the same way:
//!
//! * its output is collected -- from the pipe when it has one, from its log
//!   files when it does not,
//! * every line is checked against that tool's failure patterns,
//! * a wall-clock limit is enforced, because most of these tools cannot limit
//!   themselves,
//! * and the process tree is stopped properly when the run ends, however it ends.
//!
//! All of it runs on a thread of its own and reports through a channel; the
//! window only reads the channel.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use regex::Regex;

use crate::errors;
use crate::settings;
use crate::system;
use crate::tool::LaunchSpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Running,
    Passed,
    Failed,
    Stopped,
    /// Could not start, or cannot run as configured: never a hardware verdict.
    Broken,
}

impl State {
    pub fn text(self) -> &'static str {
        match self {
            State::Idle => "Idle",
            State::Running => "Running",
            State::Passed => "Passed",
            State::Failed => "FAILED",
            State::Stopped => "Stopped",
            State::Broken => "Could not start",
        }
    }
}

/// One thing that happened, on its way to the window.
#[derive(Clone, Debug)]
pub enum Event {
    /// A line of the tool's output.
    Output(String),
    /// The run changed state, and why.
    State(State, String),
    /// Once a second: seconds elapsed, and left when there is a limit.
    Tick { elapsed: u64, remaining: Option<u64> },
    /// A tool-specific running figure (Linpack's GFlops).
    Stat(String),
}

/// How often the watcher wakes to read output and logs and check the clock.
const POLL: Duration = Duration::from_millis(250);

/// Reads whatever is appended to a file after watching started.
///
/// A file that does not exist yet is fine -- Prime95 creates results.txt only
/// when it has something to say. A file that shrinks was rewritten and is read
/// again from the start. The encoding is decided from the byte-order mark:
/// TM5 writes UTF-8 with a BOM, and some logs are UTF-16, which read as UTF-8
/// would put a NUL between every character and match no pattern at all.
struct FileTail {
    path: PathBuf,
    offset: u64,
    encoding: Option<Encoding>,
    pending: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl FileTail {
    fn new(path: &Path) -> FileTail {
        let offset = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        FileTail { path: path.to_path_buf(), offset, encoding: None, pending: Vec::new() }
    }

    fn sniff(chunk: &[u8]) -> Encoding {
        if chunk.starts_with(&[0xFF, 0xFE]) {
            return Encoding::Utf16Le;
        }
        if chunk.starts_with(&[0xFE, 0xFF]) {
            return Encoding::Utf16Be;
        }
        // No mark, but a NUL at every second byte across the first 32: UTF-16
        // without a BOM. Strict, because guessing wrong garbles an ordinary log.
        let head = &chunk[..chunk.len().min(32)];
        if head.len() >= 16 && head.iter().skip(1).step_by(2).all(|byte| *byte == 0) {
            return Encoding::Utf16Le;
        }
        if head.len() >= 16 && head.iter().step_by(2).all(|byte| *byte == 0) {
            return Encoding::Utf16Be;
        }
        Encoding::Utf8
    }

    fn read_new(&mut self) -> String {
        let Ok(meta) = std::fs::metadata(&self.path) else { return String::new() };
        let size = meta.len();
        if size < self.offset {
            self.offset = 0;
            self.encoding = None;
            self.pending.clear();
        }
        if size == self.offset {
            return String::new();
        }
        let Ok(mut file) = std::fs::File::open(&self.path) else { return String::new() };
        use std::io::Seek;
        if file.seek(std::io::SeekFrom::Start(self.offset)).is_err() {
            return String::new();
        }
        let mut chunk = Vec::new();
        if file.read_to_end(&mut chunk).is_err() {
            return String::new();
        }
        self.offset += chunk.len() as u64;
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(&chunk);
        let encoding = *self.encoding.get_or_insert_with(|| FileTail::sniff(&data));
        let text = match encoding {
            Encoding::Utf8 => {
                // Hold back a character split across two reads.
                let cut = match std::str::from_utf8(&data) {
                    Ok(_) => data.len(),
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    Err(_) => data.len(),
                };
                self.pending = data[cut..].to_vec();
                String::from_utf8_lossy(&data[..cut]).into_owned()
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                if data.len() % 2 == 1 {
                    self.pending = vec![data.pop().unwrap()];
                }
                let units: Vec<u16> = data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| {
                        if encoding == Encoding::Utf16Le {
                            u16::from_le_bytes([pair[0], pair[1]])
                        } else {
                            u16::from_be_bytes([pair[0], pair[1]])
                        }
                    })
                    .collect();
                String::from_utf16_lossy(&units)
            }
        };
        text.trim_start_matches('\u{feff}').to_string()
    }
}

/// Runs one LaunchSpec at a time and reports through [`Runner::events`].
pub struct Runner {
    sender: Sender<Event>,
    pub events: Receiver<Event>,
    current: Option<Run>,
}

struct Run {
    thread: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl Default for Runner {
    fn default() -> Self {
        Runner::new()
    }
}

impl Runner {
    pub fn new() -> Runner {
        let (sender, events) = mpsc::channel();
        Runner { sender, events, current: None }
    }

    pub fn running(&self) -> bool {
        self.current.as_ref().is_some_and(|run| !run.thread.is_finished())
    }

    /// Launch `spec`. An error means the process did not start.
    pub fn start(&mut self, spec: LaunchSpec, label: &str) -> Result<(), String> {
        if self.running() {
            return Err("A test is already running.".into());
        }
        let started = Instant::now();
        let code_file = (!system::WINDOWS && spec.new_console && !spec.console).then(|| settings::run_dir("runner").join("exit-status"));
        let (child, pipes) = spawn(&spec, label, code_file.as_deref())?;
        let stop = Arc::new(AtomicBool::new(false));
        let sender = self.sender.clone();
        let _ = sender.send(Event::State(State::Running, if label.is_empty() { spec.summary.clone() } else { label.to_string() }));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("stress-runner".into())
            .spawn(move || supervise(spec, child, pipes, code_file, flag, sender, started))
            .map_err(|error| format!("Could not start the supervisor: {error}"))?;
        self.current = Some(Run { thread, stop });
        Ok(())
    }

    /// Ask the run to end. The supervisor stops the process tree within a
    /// quarter of a second; safe to call when nothing is running.
    pub fn stop(&self) {
        if let Some(run) = &self.current {
            run.stop.store(true, Ordering::SeqCst);
        }
    }

    /// Stop and wait for the tree to be gone, for closing the window: there is
    /// never a stress test left going with no window to stop it.
    pub fn stop_and_wait(&mut self) {
        self.stop();
        if let Some(run) = self.current.take() {
            let _ = run.thread.join();
        }
    }
}

/// A runner that goes away mid-run -- the window torn down, a panic -- takes
/// its test with it: a stress test is never left going with nothing to stop it.
impl Drop for Runner {
    fn drop(&mut self) {
        self.stop_and_wait();
    }
}

type Pipes = Vec<Box<dyn Read + Send>>;

fn spawn(spec: &LaunchSpec, label: &str, code_file: Option<&Path>) -> Result<(Child, Pipes), String> {
    let mut argv = spec.argv.clone();
    if argv.is_empty() {
        return Err("Nothing to run.".into());
    }

    // Linux has no console to give a tool; it gets a terminal window, and its
    // exit status comes back through a file.
    if let Some(code_file) = code_file {
        system::remove_file_quietly(code_file);
        let title = if label.is_empty() { &spec.summary } else { label };
        argv = system::in_terminal(&argv, title, code_file, &spec.stdin_feed).ok_or(
            "No terminal emulator was found to show this tool in. Install one (foot, alacritty, kitty, konsole, \
             gnome-terminal or xterm), or name yours in ROCH_TERMINAL.",
        )?;
    }

    let mut command = Command::new(&argv[0]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        match &spec.cmdline {
            Some(raw) => {
                command.raw_arg(raw);
            }
            None => {
                command.args(&argv[1..]);
            }
        }
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let flags = if spec.new_console {
            CREATE_NEW_CONSOLE
        } else if spec.no_window {
            CREATE_NO_WINDOW
        } else {
            0
        };
        command.creation_flags(flags);
    }
    #[cfg(not(windows))]
    {
        command.args(&argv[1..]);
        // A session of its own, so a Ctrl+C in the terminal this program was
        // started from does not reach the test.
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches nothing in the parent.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    command.current_dir(&spec.cwd);
    command.envs(spec.env.iter().map(|(key, value)| (key.as_str(), value.as_str())));
    if spec.console {
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    } else if code_file.is_some() {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| format!("{}: {error}", argv[0]))?;
    let mut pipes: Pipes = Vec::new();
    if let Some(out) = child.stdout.take() {
        pipes.push(Box::new(out));
    }
    if let Some(err) = child.stderr.take() {
        pipes.push(Box::new(err));
    }
    Ok((child, pipes))
}

/// Read a child's output and send whole lines. Split on carriage returns as
/// well as newlines: Linpack and y-cruncher redraw progress in place, and a
/// reader that only knows \n sits on a growing buffer for minutes.
fn pump(mut stream: Box<dyn Read + Send>, sink: Sender<String>) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        for byte in &chunk[..count] {
            if *byte == b'\n' || *byte == b'\r' {
                if !buffer.is_empty() {
                    let _ = sink.send(String::from_utf8_lossy(&buffer).into_owned());
                    buffer.clear();
                }
            } else {
                buffer.push(*byte);
            }
        }
    }
    if !buffer.is_empty() {
        let _ = sink.send(String::from_utf8_lossy(&buffer).into_owned());
    }
}

struct Verdict {
    outcome: Option<State>,
    note: String,
    saw_completion: bool,
}

struct Checks {
    finished: Vec<Regex>,
    aborted: Vec<Regex>,
    linpack: bool,
    reference_residual: Option<String>,
    trials: u32,
    best: f64,
    worst: f64,
    total: f64,
}

impl Checks {
    /// One line of output: echoed, then judged. Order matters: a failure wins
    /// over a completion on the same line ("is revealed 3 of errors").
    fn line(&mut self, line: &str, spec: &LaunchSpec, events: &Sender<Event>, verdict: &mut Verdict) {
        let line = line.trim_end();
        if line.is_empty() {
            return;
        }
        let _ = events.send(Event::Output(line.to_string()));
        if verdict.outcome.is_some() {
            return;
        }
        if self.linpack
            && let Some((gflops, residual, passed)) = errors::scan_linpack_row(line)
        {
            self.trials += 1;
            self.total += gflops;
            self.best = self.best.max(gflops);
            self.worst = if self.trials == 1 { gflops } else { self.worst.min(gflops) };
            let _ = events.send(Event::Stat(format!(
                "trials {}  GFlops min {:.1} / avg {:.1} / max {:.1}",
                self.trials,
                self.worst,
                self.total / self.trials as f64,
                self.best
            )));
            if !passed {
                let column = line.split_whitespace().last().unwrap_or("");
                *verdict = Verdict {
                    outcome: Some(State::Failed),
                    note: format!("Linpack check column reads '{column}' instead of 'pass'."),
                    ..*verdict
                };
            } else {
                match &self.reference_residual {
                    None => self.reference_residual = Some(residual),
                    Some(reference) if *reference != residual => {
                        verdict.outcome = Some(State::Failed);
                        verdict.note = format!("Residual changed between identical trials: {reference} then {residual}.");
                    }
                    Some(_) => {}
                }
            }
            return;
        }
        if let Some(hit) = errors::scan(line, &spec.error_key) {
            verdict.outcome = Some(State::Failed);
            verdict.note = hit;
        } else if !self.finished.is_empty() && errors::matches(line, &self.finished).is_some() {
            verdict.outcome = Some(State::Passed);
            verdict.note = format!("The tool reported it had finished: {line}");
            verdict.saw_completion = true;
        } else if !self.aborted.is_empty() && errors::matches(line, &self.aborted).is_some() {
            verdict.outcome = Some(State::Stopped);
            verdict.note = format!("The tool stopped before finishing: {line}");
        }
    }
}

fn supervise(
    spec: LaunchSpec,
    mut child: Child,
    pipes: Pipes,
    code_file: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    events: Sender<Event>,
    started: Instant,
) {
    // The log baseline is taken first, before the tool has written anything for
    // this run.
    let mut tails: Vec<FileTail> = spec.watch_files.iter().map(|path| FileTail::new(path)).collect();
    let deadline = (spec.duration_seconds > 0).then(|| started + Duration::from_secs(spec.duration_seconds));

    let (line_sender, lines) = mpsc::channel::<String>();
    let readers: Vec<JoinHandle<()>> = pipes
        .into_iter()
        .map(|pipe| {
            let sink = line_sender.clone();
            std::thread::spawn(move || pump(pipe, sink))
        })
        .collect();
    drop(line_sender);

    let mut checks = Checks {
        finished: errors::compile_plain(&spec.completion_patterns),
        aborted: errors::compile_plain(&spec.abort_patterns),
        // Linpack Extended chains different problem sizes, whose residuals
        // differ by design; its own driver judges those rows.
        linpack: spec.error_key == "linpack",
        reference_residual: None,
        trials: 0,
        best: 0.0,
        worst: 0.0,
        total: 0.0,
    };
    let mut verdict = Verdict { outcome: None, note: String::new(), saw_completion: false };

    // Whether this tool ever showed a window, and whether that window went
    // while the process still ran: somebody closing it, rather than a crash.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut had_window = false;
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut window_closed_first = false;
    let mut last_tick = Instant::now() - Duration::from_secs(2);

    loop {
        while let Ok(line) = lines.try_recv() {
            checks.line(&line, &spec, &events, &mut verdict);
        }
        for tail in &mut tails {
            let text = tail.read_new();
            for line in text.lines() {
                checks.line(line, &spec, &events, &mut verdict);
            }
        }
        if verdict.outcome.is_some() {
            break;
        }
        if stop.load(Ordering::SeqCst) {
            verdict.outcome = Some(State::Stopped);
            verdict.note = "Stopped.".into();
            break;
        }

        #[cfg(windows)]
        if !spec.console && !window_closed_first {
            if !crate::winui::windows_of(child.id()).is_empty() {
                had_window = true;
            } else if had_window && matches!(child.try_wait(), Ok(None)) {
                window_closed_first = true;
            }
        }

        if let Ok(Some(status)) = child.try_wait() {
            // The last thing a tool writes is usually its verdict: drain the
            // pipes and re-read the logs before deciding what the exit meant.
            for reader in readers {
                let _ = reader.join();
            }
            while let Ok(line) = lines.try_recv() {
                checks.line(&line, &spec, &events, &mut verdict);
            }
            for tail in &mut tails {
                let text = tail.read_new();
                for line in text.lines() {
                    checks.line(line, &spec, &events, &mut verdict);
                }
            }
            if verdict.outcome.is_some() {
                break;
            }
            let mut code = system::exit_number(status);
            // A tool in a terminal: that was the terminal's code. The tool's own
            // is in the wrapper's file, and no file means the window was closed
            // before the tool got to the end.
            let mut terminal_closed = false;
            if let Some(file) = &code_file {
                match std::fs::read_to_string(file).ok().and_then(|text| text.trim().parse().ok()) {
                    Some(own) => code = own,
                    None => terminal_closed = true,
                }
            }
            let (outcome, note) = judge_exit(code, terminal_closed, window_closed_first, &checks, verdict.saw_completion);
            verdict.outcome = Some(outcome);
            verdict.note = note;
            let _ = had_window;
            break;
        }

        let now = Instant::now();
        if let Some(deadline) = deadline
            && now >= deadline
        {
            verdict.outcome = Some(State::Passed);
            verdict.note = "Reached the time limit with no errors.".into();
            break;
        }
        if now.duration_since(last_tick) >= Duration::from_secs(1) {
            last_tick = now;
            let _ = events.send(Event::Tick {
                elapsed: now.duration_since(started).as_secs(),
                remaining: deadline.map(|deadline| deadline.saturating_duration_since(now).as_secs()),
            });
        }
        std::thread::sleep(POLL);
    }

    let outcome = verdict.outcome.unwrap_or(State::Stopped);
    // A tool that finished on its own and was asked to stay on screen is left
    // alone: it is sitting at its own result, which is what was asked for.
    let still_running = matches!(child.try_wait(), Ok(None));
    let mut note = verdict.note;
    if spec.leave_open && outcome == State::Passed && verdict.saw_completion && still_running {
        note += " Its window has been left open; close it when you have read the result.";
    } else {
        system::kill_tree(&mut child);
    }
    let _ = events.send(Event::State(outcome, note));
}

/// What an exit means. Interrupted and closed windows are stops, never
/// failures; a clean exit from a tool that announces its completion, without
/// the announcement, is a stop too -- closing a window mid-run is not a pass.
fn judge_exit(code: i64, terminal_closed: bool, window_closed_first: bool, checks: &Checks, saw_completion: bool) -> (State, String) {
    if terminal_closed {
        return (
            State::Stopped,
            "The tool's window was closed before it finished, so the test did not run to the end. This is not a hardware failure.".into(),
        );
    }
    if system::interrupted(code) {
        return (
            State::Stopped,
            format!(
                "The tool's console was closed or interrupted ({}). This is not a hardware failure, and the test did not finish.",
                system::describe_exit(code)
            ),
        );
    }
    if code != 0 && window_closed_first {
        return (
            State::Stopped,
            format!(
                "The tool's window was closed, so the test did not run to the end. It exited with {}, which is what a program \
                 taken down that way gives and says nothing about the machine.",
                system::describe_exit(code)
            ),
        );
    }
    if code != 0 {
        return (State::Failed, format!("The tool exited with {}, which usually means it crashed.", system::describe_exit(code)));
    }
    if !checks.finished.is_empty() && !saw_completion {
        return (State::Stopped, "The tool exited without reporting that it had finished, so the test did not run to the end.".into());
    }
    (State::Passed, "Finished on its own with no errors.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_reads_only_what_is_appended() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.txt");
        std::fs::write(&path, "old line\n").unwrap();
        let mut tail = FileTail::new(&path);
        assert_eq!(tail.read_new(), "");
        std::fs::write(&path, "old line\nnew line\n").unwrap();
        assert_eq!(tail.read_new(), "new line\n");
        // A UTF-16 log, as some Windows tools write, read as text.
        let wide = dir.path().join("wide.txt");
        let mut tail = FileTail::new(&wide);
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("Error found\n".encode_utf16().flat_map(|unit| unit.to_le_bytes())).collect();
        std::fs::write(&wide, &utf16).unwrap();
        assert_eq!(tail.read_new(), "Error found\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_failure_line_fails_the_run_and_a_limit_passes_it() {
        let mut runner = Runner::new();
        let spec = LaunchSpec {
            argv: vec!["sh".into(), "-c".into(), "echo starting; sleep 1; echo 'Error found. Mode X'; sleep 30".into()],
            cwd: std::env::temp_dir(),
            console: true,
            error_key: "memtest_vulkan".into(),
            ..Default::default()
        };
        runner.start(spec, "test").unwrap();
        let state = wait_for_end(&runner);
        assert_eq!(state.0, State::Failed, "{}", state.1);

        let spec = LaunchSpec {
            argv: vec!["sh".into(), "-c".into(), "while true; do echo ok; sleep 0.2; done".into()],
            cwd: std::env::temp_dir(),
            console: true,
            error_key: "memtest_vulkan".into(),
            duration_seconds: 1,
            ..Default::default()
        };
        runner.start(spec, "test").unwrap();
        assert_eq!(wait_for_end(&runner).0, State::Passed);
    }

    #[cfg(unix)]
    fn wait_for_end(runner: &Runner) -> (State, String) {
        loop {
            match runner.events.recv_timeout(Duration::from_secs(20)).expect("an event") {
                Event::State(State::Running, _) => {}
                Event::State(state, note) => return (state, note),
                _ => {}
            }
        }
    }
}
