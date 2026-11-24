// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! What differs between Windows and Linux, kept in one place.
//!
//! Everything else asks this module rather than checking the platform itself:
//! how a console tool is given a window of its own, how a run's process tree is
//! stopped, which exit codes mean "somebody interrupted it" rather than "it
//! crashed", and where per-user data lives.
//!
//! On Windows a console tool gets its own console from `CREATE_NEW_CONSOLE`.
//! Linux has no such thing -- a console is a terminal emulator, a separate
//! program -- so the tool is started inside one. That costs the tool's exit
//! code, which no terminal passes on, so the command runs under a small shell
//! wrapper that writes it to a file before the terminal closes.

use std::path::PathBuf;
use std::process::Child;

pub const WINDOWS: bool = cfg!(windows);

/// Windows: given to a console process sent Ctrl+C or Ctrl+Break, or whose
/// console was closed.
pub const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// How a process ended, as one number: the exit code, or on Linux minus the
/// signal that killed it.
pub fn exit_number(status: std::process::ExitStatus) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return -i64::from(signal);
        }
    }
    status.code().map(i64::from).unwrap_or(-1)
}

/// Whether an exit code means the tool was interrupted, not that it failed.
///
/// Closing a tool's window or pressing Ctrl+C in it says nothing about the
/// machine, and must never be reported as a hardware failure. On Linux that is
/// SIGHUP (its terminal closed), SIGINT or SIGTERM: minus the signal from a
/// process started directly, 128 + the signal from the shell wrapper.
pub fn interrupted(code: i64) -> bool {
    if WINDOWS {
        return code as u32 == STATUS_CONTROL_C_EXIT;
    }
    [1, 2, 15].iter().any(|signal| code == -signal || code == 128 + signal)
}

/// An exit code in the form that says what happened.
///
/// Windows status codes are recognisable in hex and meaningless in signed
/// decimal: 3221225477 is nothing, 0xC0000005 is an access violation. On Linux
/// the signal is named: "killed by SIGSEGV" is the clue.
pub fn describe_exit(code: i64) -> String {
    if WINDOWS {
        return format!("code {code} (0x{:08X})", code as u32);
    }
    let signal = if code < 0 {
        -code
    } else if (129..160).contains(&code) {
        code - 128
    } else {
        0
    };
    match signal_name(signal) {
        Some(name) => format!("code {code} ({name})"),
        None => format!("code {code}"),
    }
}

fn signal_name(signal: i64) -> Option<&'static str> {
    Some(match signal {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        15 => "SIGTERM",
        _ => return None,
    })
}

fn home() -> PathBuf {
    std::env::var_os(if WINDOWS { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// Where per-user application data goes: `%LOCALAPPDATA%`, or on Linux
/// `$XDG_DATA_HOME` (normally `~/.local/share`).
pub fn data_home() -> PathBuf {
    if WINDOWS {
        return std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(home);
    }
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local").join("share"))
}

/// Remove a file that may not be there.
pub fn remove_file_quietly(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

// ----- a window for a console tool, on Linux ----------------------------------

const APP_ID: &str = "RochStressTest.tool";

/// Terminal emulators that stay in the foreground for as long as the command
/// they were given runs, and how each is told a title and a command. Order is
/// only a fallback: `$ROCH_TERMINAL`, then the desktop's `$TERMINAL` when it is
/// one of these, come first. Single-instance terminals are asked not to be:
/// one that handed the command to a server process and exited at once would
/// look like a tool that ended immediately.
fn terminal_options(name: &str, title: &str) -> Option<Vec<String>> {
    let owned = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
    Some(match name {
        "foot" => vec![format!("--title={title}"), format!("--app-id={APP_ID}")],
        "alacritty" => owned(&["--title", title, "--class", APP_ID, "-e"]),
        "kitty" => owned(&["--title", title, "--class", APP_ID]),
        "ghostty" => vec![format!("--title={title}"), "--gtk-single-instance=false".into(), "-e".into()],
        "wezterm" => owned(&["start", "--always-new-process", "--class", APP_ID, "--"]),
        "konsole" => vec!["--nofork".into(), "-p".into(), format!("tabtitle={title}"), "-e".into()],
        "gnome-terminal" => owned(&["--wait", "--title", title, "--"]),
        "xfce4-terminal" => owned(&["--disable-server", "--title", title, "-x"]),
        "xterm" => owned(&["-T", title, "-e"]),
        "xdg-terminal-exec" => vec![format!("--title={title}"), "--".into()],
        _ => return None,
    })
}

const TERMINALS: [&str; 10] =
    ["foot", "alacritty", "kitty", "ghostty", "wezterm", "konsole", "gnome-terminal", "xfce4-terminal", "xterm", "xdg-terminal-exec"];

/// The wrapper a tool runs under inside the terminal. `$1` is where the exit
/// code goes, `$2` is text to type into the tool before its input is handed to
/// the keyboard (empty for none), and the rest is the tool's own command line.
/// Typing ahead is how mprime is taken straight to its torture-test questions,
/// as Prime95's dialog opens on Windows: the text is sent, then `cat` passes on
/// whatever is typed in the window, and is stopped once the tool exits.
const WRAPPER: &str = r#"code=$1; feed=$2; shift 2
if [ -n "$feed" ]; then
  { printf '%s' "$feed"; exec cat; } | "$@"
  status=$?
  pkill -P $$ -x cat 2>/dev/null
else
  "$@"
  status=$?
fi
printf '%s\n' "$status" > "$code"
exit "$status"
"#;

/// A program on `$PATH`, as a full path.
pub fn which(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return path.is_file().then_some(path);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(program)).find(|candidate| is_executable(candidate))
}

fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// The terminal command for a title: the program and its options, ready for
/// the command line to be appended. None when no terminal is installed.
pub fn terminal(title: &str) -> Option<Vec<String>> {
    if let Ok(chosen) = std::env::var("ROCH_TERMINAL") {
        let parts: Vec<String> = chosen.split_whitespace().map(str::to_string).collect();
        if let Some(program) = parts.first().and_then(|first| which(first)) {
            let mut command = vec![program.display().to_string()];
            command.extend(parts[1..].iter().cloned());
            return Some(command);
        }
    }
    let preferred = std::env::var("TERMINAL").unwrap_or_default();
    let preferred = preferred.split_whitespace().next().unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
    let mut order: Vec<&str> = TERMINALS.to_vec();
    order.sort_by_key(|name| *name != preferred);
    for name in order {
        if let Some(program) = which(name) {
            let mut command = vec![program.display().to_string()];
            command.extend(terminal_options(name, title)?);
            return Some(command);
        }
    }
    None
}

/// `argv` wrapped to run in a terminal window, or None when there is none.
///
/// The command runs under [`WRAPPER`], which writes its exit status to
/// `code_file`; if that file is missing when the terminal exits, the window was
/// closed before the tool finished.
pub fn in_terminal(argv: &[String], title: &str, code_file: &std::path::Path, feed: &str) -> Option<Vec<String>> {
    let mut command = terminal(title)?;
    command.extend(["sh".into(), "-c".into(), WRAPPER.into(), "roch-stresstest".into()]);
    command.push(code_file.display().to_string());
    command.push(feed.to_string());
    command.extend(argv.iter().cloned());
    Some(command)
}

// ----- stopping a run ----------------------------------------------------------

#[cfg(target_os = "linux")]
mod tree {
    use std::collections::HashMap;

    /// `{parent pid: [child pids]}` for every process on the system.
    fn children_by_parent() -> HashMap<i32, Vec<i32>> {
        let mut table: HashMap<i32, Vec<i32>> = HashMap::new();
        let Ok(entries) = std::fs::read_dir("/proc") else { return table };
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            // The command name is in parentheses and may itself hold spaces or
            // parentheses, so the fields are read after the last one.
            let Some(rest) = stat.rfind(')').map(|at| &stat[at + 1..]) else { continue };
            if let Some(parent) = rest.split_whitespace().nth(1).and_then(|field| field.parse().ok()) {
                table.entry(parent).or_default().push(pid);
            }
        }
        table
    }

    /// Every process started by `pid`, directly or not, deepest first.
    pub fn descendants(pid: i32) -> Vec<i32> {
        let table = children_by_parent();
        let mut found = Vec::new();
        let mut stack = vec![pid];
        while let Some(parent) = stack.pop() {
            for child in table.get(&parent).into_iter().flatten() {
                if !found.contains(child) {
                    found.push(*child);
                    stack.push(*child);
                }
            }
        }
        found.reverse();
        found
    }

    /// Alive and not a zombie (one that has finished and waits to be reaped).
    pub fn alive(pid: i32) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return false };
        stat.rfind(')').and_then(|at| stat[at + 1..].split_whitespace().next()).is_some_and(|state| state != "Z")
    }

    pub fn signal(pid: i32, signal: i32) {
        // SAFETY: kill only sends a signal; a pid that has gone just fails.
        unsafe {
            libc::kill(pid, signal);
        }
    }
}

/// Every process started by `pid`, deepest first (Linux), for tests and the
/// stop below.
#[cfg(target_os = "linux")]
pub fn descendants(pid: u32) -> Vec<u32> {
    tree::descendants(pid as i32).into_iter().map(|pid| pid as u32).collect()
}

/// Stop a process and everything it started.
///
/// Windows: `taskkill /T /F`, because the windowed tools ignore a polite
/// request and Prime95 leaves worker threads mid-FFT if the tree is not taken
/// with it.
///
/// Linux: the whole tree is collected *before* anything is signalled. A tool in
/// a terminal is not in the terminal's process group -- the terminal gives it a
/// session of its own -- and killing the terminal first would orphan it out of
/// reach. Each process is asked to stop (SIGTERM, which mprime and y-cruncher
/// handle by finishing cleanly), then anything still there after five seconds
/// is killed.
pub fn kill_tree(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(target_os = "linux")]
    {
        let root = child.id() as i32;
        let mut everything = tree::descendants(root);
        everything.push(root);
        for pid in &everything {
            tree::signal(*pid, libc::SIGTERM);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline && everything.iter().any(|pid| *pid != root && tree::alive(*pid)) {
            let _ = child.try_wait();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = child.try_wait();
        for pid in &everything {
            if tree::alive(*pid) {
                tree::signal(*pid, libc::SIGKILL);
            }
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interruptions_are_not_failures() {
        if WINDOWS {
            assert!(interrupted(STATUS_CONTROL_C_EXIT as i32 as i64));
            assert!(!interrupted(0xC000_0005u32 as i32 as i64));
        } else {
            for code in [-1, -2, -15, 129, 130, 143] {
                assert!(interrupted(code), "{code}");
            }
            for code in [0, 1, -11, 139] {
                assert!(!interrupted(code), "{code}");
            }
            assert_eq!(describe_exit(139), "code 139 (SIGSEGV)");
            assert_eq!(describe_exit(-9), "code -9 (SIGKILL)");
        }
    }

    #[test]
    fn the_wrapper_records_the_exit_status() {
        if WINDOWS {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let code = dir.path().join("code");
        let status = std::process::Command::new("sh")
            .args(["-c", WRAPPER, "test"])
            .arg(&code)
            .arg("")
            .args(["sh", "-c", "exit 7"])
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(7));
        assert_eq!(std::fs::read_to_string(&code).unwrap().trim(), "7");
    }
}
