// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! The runner against the real tools, on Linux: terminal windows, time limits,
//! Stop, a closed window, an injected failure, and no process left behind.
//!
//! Ignored by default -- they open windows, load the GPU and take minutes. Run
//! them on a desktop with the tools fetched (packaging/linux/fetch-tools.sh):
//!
//!     cargo test --test real_tools -- --ignored --test-threads=1

#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use roch_stresstest::config::{Config, ConfigExt};
use roch_stresstest::runner::{Event, Runner, State};
use roch_stresstest::tool::{LaunchSpec, Tool};
use roch_stresstest::tools;
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tool(key: &str) -> Box<dyn Tool> {
    tools::tools().into_iter().find(|tool| tool.key() == key).expect("a tool")
}

/// Processes whose command line mentions any of `names`.
fn processes(names: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        if !entry.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let text = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        if names.iter().any(|name| text.contains(name)) && !text.contains("real_tools") {
            found.push(format!("{} {}", entry.file_name().to_string_lossy(), text.trim()));
        }
    }
    found
}

const TOOL_PROCESSES: [&str; 4] = ["mprime", "memtest_vulkan ", "y-cruncher", "Binaries/"];

/// Run `spec`, calling `during` once after `at`; the final state and note.
fn run(spec: LaunchSpec, at: Duration, during: impl FnOnce()) -> (State, String) {
    let mut runner = Runner::new();
    runner.start(spec, "real tools").unwrap();
    let started = Instant::now();
    let mut during = Some(during);
    loop {
        if started.elapsed() >= at
            && let Some(action) = during.take()
        {
            action();
        }
        match runner.events.recv_timeout(Duration::from_millis(200)) {
            Ok(Event::State(State::Running, _)) | Err(_) => {}
            Ok(Event::State(state, note)) => {
                runner.stop_and_wait();
                std::thread::sleep(Duration::from_secs(1));
                assert!(processes(&TOOL_PROCESSES).is_empty(), "left behind: {:?}", processes(&TOOL_PROCESSES));
                return (state, note);
            }
            Ok(_) => {}
        }
        assert!(started.elapsed() < Duration::from_secs(300), "the run never ended");
    }
}

fn ycruncher(show: bool) -> LaunchSpec {
    let tool = tool("ycruncher");
    let config: Config = tool
        .quick_config(&root())
        .with("algorithms", Value::from("BKT"))
        .with("memory", Value::from(1))
        .with("per_test", Value::from(10))
        .with("duration", Value::from(1))
        .with("pause", Value::Bool(false))
        .with("show_window", Value::Bool(show));
    tool.build(&config, &root()).unwrap()
}

fn memtest() -> LaunchSpec {
    let tool = tool("memtest_vulkan");
    tool.build(&tool.quick_config(&root()), &root()).unwrap()
}

#[test]
#[ignore]
fn ycruncher_in_a_terminal_reaches_its_own_end() {
    let (state, note) = run(ycruncher(true), Duration::MAX, || {});
    assert_eq!(state, State::Passed, "{note}");
    assert!(note.contains("Test Finished"), "{note}");
}

#[test]
#[ignore]
fn ycruncher_piped_reaches_its_own_end() {
    let (state, note) = run(ycruncher(false), Duration::MAX, || {});
    assert_eq!(state, State::Passed, "{note}");
}

#[test]
#[ignore]
fn memtest_passes_at_the_time_limit() {
    let mut spec = memtest();
    spec.duration_seconds = 20;
    let (state, note) = run(spec, Duration::MAX, || {});
    assert_eq!(state, State::Passed, "{note}");
}

#[test]
#[ignore]
fn a_failure_line_in_the_log_fails_the_run() {
    let spec = memtest();
    let log = spec.watch_files[0].clone();
    let (state, note) = run(spec, Duration::from_secs(8), move || {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(file, "Error found. Mode NEXT_RE_READ, total errors 0x1 out of 0x40000000 (0.0000001%)").unwrap();
    });
    assert_eq!(state, State::Failed, "{note}");
}

#[test]
#[ignore]
fn closing_the_terminal_is_a_stop_not_a_failure() {
    let mut spec = memtest();
    spec.duration_seconds = 60;
    let (state, note) = run(spec, Duration::from_secs(8), || {
        // The terminal goes, as when its window is closed: the parent of the
        // wrapper the tool runs under, whichever terminal that is.
        for line in processes(&["code=$1; feed=$2"]) {
            let pid = line.split_whitespace().next().unwrap();
            // The wrapper itself, not the terminal whose arguments carry it.
            if std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim() != "sh" {
                continue;
            }
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            let parent: i32 =
                stat.rsplit(')').next().and_then(|rest| rest.split_whitespace().nth(1)).and_then(|field| field.parse().ok()).unwrap_or(0);
            let comm = std::fs::read_to_string(format!("/proc/{parent}/comm")).unwrap_or_default();
            if parent > 1 && comm.trim() != "sh" {
                // SAFETY: sends SIGTERM to the terminal this test started.
                unsafe {
                    libc_kill(parent, 15);
                }
            }
        }
    });
    assert_eq!(state, State::Stopped, "{note}");
    assert!(note.contains("not a hardware failure"), "{note}");
}

#[test]
#[ignore]
fn prime95_opens_at_its_questions_and_stops_cleanly() {
    let tool = tool("prime95");
    let spec = tool.build(&tool.quick_config(&root()), &root()).unwrap();
    let mut runner = Runner::new();
    runner.start(spec, "real tools").unwrap();
    std::thread::sleep(Duration::from_secs(6));
    assert!(!processes(&["mprime"]).is_empty(), "mprime is not running");
    runner.stop();
    let (state, _) = loop {
        if let Ok(Event::State(state, note)) = runner.events.recv_timeout(Duration::from_secs(30))
            && state != State::Running
        {
            break (state, note);
        }
    };
    assert_eq!(state, State::Stopped);
    std::thread::sleep(Duration::from_secs(1));
    assert!(processes(&TOOL_PROCESSES).is_empty(), "left behind: {:?}", processes(&TOOL_PROCESSES));
}

unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, signal: i32) -> i32;
}
