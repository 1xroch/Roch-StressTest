// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! y-cruncher's component stress tester.
//!
//! The best-behaved tool of the set: a real command line, a real time limit, and
//! a `logfile:` option that mirrors everything it prints -- which is what lets
//! it run in a visible window and still be watched. The algorithm names come
//! from Command Lines.txt in the distribution. Naming any algorithm disables
//! the rest, so ticking none passes none: that is how y-cruncher is told to run
//! the lot.
//!
//! Its tab is a profile editor. A profile is one line calling y-cruncher, saved
//! beside it -- a .bat on Windows, a shell script on Linux -- and every profile
//! there gets a Quick Start button, so one dropped in by hand works too.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{Config, ConfigExt, config};
use crate::errors;
use crate::settings;
use crate::tool::{Field, LaunchSpec, QuickActions, Tool, clean_profile_name, duration_note, glob_files, path_text, remove_stale};

/// The valid values for [algorithm], in the order the manual lists them. Only
/// the canonical names are offered, so nothing saved depends on an alias.
pub const ALGORITHMS: [(&str, &str); 8] = [
    ("BKT", "Basecase + Karatsuba + Toom-Cook"),
    ("BBP", "Bailey-Borwein-Plouffe digit extraction"),
    ("SFTv4", "Small in-cache FFT"),
    ("SNT", "Small in-cache NTT"),
    ("SVT", "Small in-cache vector transform"),
    ("FFTv4", "Fast Fourier transform"),
    ("NTT63", "Classic 64-bit NTT"),
    ("VSTv3", "Vector-scalable transform"),
];

/// Every spelling y-cruncher accepts, mapped to the name offered here, so a
/// hand-typed VT3 is read back as the VSTv3 tick it means.
fn canonical(token: &str) -> Option<&'static str> {
    let upper = token.to_ascii_uppercase();
    if let Some((name, _)) = ALGORITHMS.iter().find(|(name, _)| name.to_ascii_uppercase() == upper) {
        return Some(name);
    }
    Some(match upper.as_str() {
        "VT3" | "VST" => "VSTv3",
        "N63" | "N64" => "NTT63",
        "SFT" => "SFTv4",
        "FFT" => "FFTv4",
        _ => return None,
    })
}

/// Priority values from the manual. They differ by system: on Linux 0 is
/// "maximum", which for anyone but root is the ordinary priority every other
/// program runs at, and -1 its default, the lowest.
fn priorities() -> &'static [(&'static str, i64)] {
    if cfg!(windows) {
        &[("Below normal", -1), ("Normal", 0), ("Above normal", 1), ("High", 2)]
    } else {
        &[("Below normal", -1), ("Normal", 0)]
    }
}

/// What y-cruncher runs at when a profile says nothing about priority.
const NATIVE_PRIORITY: &str = "Below normal";

const PROFILE_EXT: &str = if cfg!(windows) { "bat" } else { "sh" };
const EXE_NAME: &str = if cfg!(windows) { "y-cruncher.exe" } else { "y-cruncher" };

/// How much longer than its own -TL y-cruncher is given before the runner steps
/// in. -TL is only checked between tests, so it routinely runs past it to
/// finish the test in progress; a backstop on the dot would kill it mid-test.
const BACKSTOP_GRACE_SECONDS: u64 = 300;

pub struct YCruncher;

impl YCruncher {
    /// Every profile beside y-cruncher, sorted by name.
    pub fn profile_files(&self, root: &Path) -> Vec<PathBuf> {
        let Some(exe) = self.locate(root) else { return Vec::new() };
        let folder = exe.parent().unwrap_or(root);
        let mut found = Vec::new();
        for dir in [folder.to_path_buf(), YCruncher::own_profiles()] {
            found.extend(glob_files(&crate::tool::glob_in(&dir, &format!("*.{PROFILE_EXT}")), true));
        }
        found.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));
        found
    }

    /// Where profiles go when y-cruncher's own folder cannot be written: installed by a package,
    /// it belongs to root. Profiles from both places are listed.
    fn own_profiles() -> PathBuf {
        settings::app_data_dir().join("y-cruncher profiles")
    }

    fn stem(path: &Path) -> String {
        path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// The y-cruncher arguments these settings make, as a profile holds them.
    /// Everything before "stress" is a startup parameter and everything after
    /// it belongs to the stress test, per the manual. Nothing is written that
    /// y-cruncher would do anyway.
    pub fn profile_arguments(config: &Config) -> Vec<String> {
        let mut startup = Vec::new();
        if config.flag("pause") {
            startup.push("pause:1".to_string());
        }
        let priority = config.text("priority");
        if priority != NATIVE_PRIORITY
            && let Some((_, number)) = priorities().iter().find(|(label, _)| *label == priority)
        {
            startup.push(format!("priority:{number}"));
        }
        let mut test = Vec::new();
        let memory = config.int("memory");
        if memory > 0 {
            test.push(format!("-M:{memory}GB"));
        }
        let per_test = config.int("per_test");
        if per_test > 0 {
            test.push(format!("-D:{per_test}"));
        }
        let minutes = config.int("duration");
        if minutes > 0 {
            test.push(format!("-TL:{}", minutes * 60));
        }
        // Extras go on whichever side of "stress" they are shaped for: a startup
        // parameter is "name:value" with no leading dash.
        for token in config.text("extra").split_whitespace() {
            if token.starts_with('-') || !token.contains(':') {
                test.push(token.to_string());
            } else {
                startup.push(token.to_string());
            }
        }
        let mut selected: Vec<&str> = Vec::new();
        for token in config.text("algorithms").split([',', ' ']).filter(|token| !token.is_empty()) {
            if let Some(name) = canonical(token)
                && !selected.contains(&name)
            {
                selected.push(name);
            }
        }
        let mut arguments = startup;
        arguments.push("stress".into());
        arguments.extend(test);
        arguments.extend(selected.into_iter().map(str::to_string));
        arguments
    }

    /// -M's value in whole gigabytes, or None when it cannot be read.
    fn gigabytes(text: &str) -> Option<i64> {
        let upper = text.to_ascii_uppercase();
        let upper = upper.trim_end_matches("IB").trim_end_matches('B');
        let (number, scale) = match upper.chars().last()? {
            'K' => (&upper[..upper.len() - 1], 1024f64.powi(-2)),
            'M' => (&upper[..upper.len() - 1], 1024f64.powi(-1)),
            'G' => (&upper[..upper.len() - 1], 1.0),
            'T' => (&upper[..upper.len() - 1], 1024.0),
            _ => (upper, 1024f64.powi(-3)),
        };
        let value: f64 = number.parse().ok()?;
        Some(((value * scale).round() as i64).max(1))
    }

    /// The y-cruncher arguments out of a profile, or none. These files are one
    /// line calling y-cruncher, which is all this needs to understand.
    pub fn profile_file_arguments(path: &Path) -> Vec<String> {
        let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("::") || line.to_ascii_lowercase().starts_with("rem ") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            // The last token naming y-cruncher is the program: a folder before it on the line
            // ("cd '/opt/.../y-cruncher v0.8.7.9547-static'") can be named y-cruncher too.
            let program = parts.iter().rposition(|part| {
                let program = part.trim_matches(['"', '\'']).to_ascii_lowercase();
                let base = program.rsplit(['/', '\\']).next().unwrap_or(&program).to_string();
                base == "y-cruncher.exe" || base == "y-cruncher"
            });
            if let Some(index) = program {
                return parts[index + 1..].iter().map(|part| part.to_string()).collect();
            }
        }
        Vec::new()
    }
}

impl Tool for YCruncher {
    fn key(&self) -> &'static str {
        "ycruncher"
    }

    fn name(&self) -> &'static str {
        "y-cruncher"
    }

    fn blurb(&self) -> &'static str {
        "Component stress tester. VSTv3 and FFTv4 are the AVX-heavy pair that finds an unstable memory clock quickly; the \
         small in-cache tests stay on the core."
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        if cfg!(windows) { vec!["y-cruncher*/y-cruncher.exe", "y-cruncher.exe"] } else { vec!["y-cruncher*/y-cruncher", "y-cruncher"] }
    }

    fn fields(&self) -> Vec<Field> {
        let names: Vec<&str> = ALGORITHMS.iter().map(|(name, _)| *name).collect();
        let priority_labels: Vec<&str> = priorities().iter().map(|(label, _)| *label).collect();
        vec![
            Field::multi(
                "algorithms",
                "Algorithms",
                "VSTv3",
                &names,
                "Tick as many as you want; they run one after another. None ticked runs all of them, which is y-cruncher's \
                 own default.",
            ),
            Field::int("memory", "Memory to use", 0, 0, 1024, "GB", "-M. 0 leaves it out and y-cruncher picks."),
            Field::int(
                "per_test",
                "Seconds per test",
                60,
                0,
                86_400,
                "s",
                "-D. How long each algorithm runs before the next. 0 leaves it out and y-cruncher picks.",
            ),
            Field::int(
                "duration",
                "Stop after",
                60,
                0,
                100_000,
                "min",
                "-TL, in seconds on the command line. y-cruncher checks it between tests, so it stops at the first test \
                 boundary at or after this; the countdown allows five minutes past it before stepping in. 0 runs until \
                 you press Stop.",
            ),
            Field::flag("pause", "Pause at the end", false, "pause:1. Keeps the finished result on screen."),
            Field::choice(
                "priority",
                "Process priority",
                "Normal",
                &priority_labels,
                "priority:. y-cruncher runs at below normal when a profile does not say, which shares the machine but \
                 under-stresses it.",
            ),
            Field::text(
                "extra",
                "Other options",
                "",
                "Anything else to pass, exactly as y-cruncher takes it. Options before 'stress' in a saved profile that \
                 this tab has no box for are kept here too.",
            ),
            Field::flag(
                "show_window",
                "Show y-cruncher's window",
                true,
                "Only for Start on this tab; a saved profile always shows it. Failures are read from its log file either way.",
            ),
        ]
    }

    fn quick_values(&self) -> Config {
        config([
            ("algorithms", Value::from("VSTv3")),
            ("memory", Value::from(28)),
            ("duration", Value::from(30)),
            ("per_test", Value::from(0)),
            ("pause", Value::Bool(true)),
        ])
    }

    fn quick_actions(&self, root: &Path) -> QuickActions {
        let base = self.quick_config(root);
        let mut actions = vec![("Open".to_string(), base.with("quick_mode", Value::from("open")))];
        for file in self.profile_files(root) {
            let label = YCruncher::stem(&file).to_uppercase();
            let mut config = base.with("quick_mode", Value::from("profile"));
            config.insert("profile_path".into(), Value::from(path_text(&file)));
            actions.push((label, config));
        }
        actions
    }

    fn quick_summary(&self, root: &Path) -> String {
        let names: Vec<String> = self.profile_files(root).iter().map(|file| YCruncher::stem(file).to_uppercase()).collect();
        if names.is_empty() {
            return duration_note(&self.quick_config(root));
        }
        format!("Open the menu, or run {}", names.join(" / "))
    }

    fn profiles(&self, root: &Path) -> Option<Vec<(String, PathBuf)>> {
        Some(self.profile_files(root).into_iter().map(|file| (YCruncher::stem(&file), file)).collect())
    }

    /// A profile read back into this tab's settings. Anything the tab has no box
    /// for is kept in "Other options", so opening and saving loses nothing.
    fn profile_values(&self, path: &Path) -> Result<Config, String> {
        let mut values = config([
            ("algorithms", Value::from("")),
            ("memory", Value::from(0)),
            ("per_test", Value::from(0)),
            ("duration", Value::from(0)),
            ("pause", Value::Bool(false)),
            ("priority", Value::from(NATIVE_PRIORITY)),
            ("extra", Value::from("")),
        ]);
        let mut extra: Vec<String> = Vec::new();
        let mut selected: Vec<&str> = Vec::new();
        for token in YCruncher::profile_file_arguments(path) {
            let lower = token.to_ascii_lowercase();
            let (name, value) = token.split_once(':').unwrap_or((&token, ""));
            if lower == "stress" {
                continue;
            }
            if lower.starts_with("pause:") {
                values.insert("pause".into(), Value::Bool(value.trim() == "1"));
                if !matches!(value.trim(), "1" | "-2") {
                    extra.push(token.clone());
                }
                continue;
            }
            if lower.starts_with("priority:") {
                match priorities().iter().find(|(_, number)| value.trim() == number.to_string()) {
                    Some((label, _)) => {
                        values.insert("priority".into(), Value::from(*label));
                    }
                    None => extra.push(token.clone()),
                }
                continue;
            }
            let upper = name.to_ascii_uppercase();
            if matches!(upper.as_str(), "-M" | "-D" | "-TL") {
                let number = if upper == "-M" { YCruncher::gigabytes(value) } else { value.parse::<i64>().ok() };
                match (upper.as_str(), number) {
                    (_, None) => extra.push(token.clone()),
                    ("-M", Some(number)) => {
                        values.insert("memory".into(), Value::from(number));
                    }
                    ("-D", Some(number)) => {
                        values.insert("per_test".into(), Value::from(number));
                    }
                    (_, Some(number)) => {
                        values.insert("duration".into(), Value::from(((number as f64 / 60.0).round() as i64).max(1)));
                    }
                }
                continue;
            }
            if let Some(name) = canonical(&token) {
                if !selected.contains(&name) {
                    selected.push(name);
                }
                continue;
            }
            extra.push(token.clone());
        }
        values.insert("algorithms".into(), Value::from(selected.join(" ")));
        values.insert("extra".into(), Value::from(extra.join(" ")));
        Ok(values)
    }

    fn save_profile(&self, root: &Path, name: &str, config: &Config) -> Result<PathBuf, String> {
        let exe = self.locate(root).ok_or("A profile needs y-cruncher.")?;
        let name = clean_profile_name(name);
        if name.is_empty() {
            return Err("A profile needs a name.".into());
        }
        let beside = exe.parent().unwrap_or(root).to_path_buf();
        let folder = if crate::tool::writable(&beside) { beside } else { YCruncher::own_profiles() };
        let _ = std::fs::create_dir_all(&folder);
        let path = folder.join(format!("{name}.{PROFILE_EXT}"));
        let arguments = YCruncher::profile_arguments(config).join(" ");
        let text = if cfg!(windows) {
            format!("{EXE_NAME} {arguments}\r\n")
        } else if folder == exe.parent().unwrap_or(root) {
            // Runnable by hand too: from its own folder, whatever the caller's.
            format!(
                "#!/bin/sh\n# A y-cruncher profile saved by Roch StressTest.\ncd \"$(dirname \"$0\")\" && exec ./{EXE_NAME} {arguments}\n"
            )
        } else {
            let dir = exe.parent().unwrap_or(root).display().to_string().replace('\'', "'\\''");
            format!("#!/bin/sh\n# A y-cruncher profile saved by Roch StressTest.\ncd '{dir}' && exec ./{EXE_NAME} {arguments}\n")
        };
        std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
        Ok(path)
    }

    fn profile_hint(&self) -> String {
        format!(
            "Saved as <name>.{PROFILE_EXT} beside {EXE_NAME} (in your Roch StressTest folder when that cannot be written), \
             and shown as a button on Quick Start."
        )
    }

    fn profile_preview(&self, config: &Config) -> String {
        format!("{EXE_NAME} {}", YCruncher::profile_arguments(config).join(" "))
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String> {
        let exe = self.locate(root).ok_or(if cfg!(windows) {
            "y-cruncher.exe was not found. Expected a folder like 'y-cruncher v0.8.7.9547b' beside this program."
        } else {
            "y-cruncher was not found. Expected a folder like 'y-cruncher v0.8.7.9547-static' beside this program \
             (packaging/linux/fetch-tools.sh puts it there)."
        })?;
        let cwd = exe.parent().unwrap_or(root).to_path_buf();
        let work = settings::run_dir("ycruncher");
        let logfile = work.join("ycruncher.log");

        match config.text("quick_mode").as_str() {
            // No arguments: y-cruncher's own menu, as when it is double-clicked.
            // Nothing is watched because nothing has been chosen yet.
            "open" => {
                return Ok(LaunchSpec {
                    argv: vec![path_text(&exe)],
                    cwd,
                    error_key: self.key().into(),
                    summary: "y-cruncher (its own menu)".into(),
                    leave_open: true,
                    new_console: true,
                    ..Default::default()
                });
            }
            "profile" => {
                let profile = PathBuf::from(config.text("profile_path"));
                let arguments = YCruncher::profile_file_arguments(&profile);
                if arguments.is_empty() {
                    return Err(format!(
                        "{} has no y-cruncher command in it that this could read.",
                        profile.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
                    ));
                }
                // The profile decides the test. Two things are added and nothing
                // is taken away: logfile: is the only reason a run in a visible
                // window can be watched, and skip-warnings stops it waiting at a
                // startup prompt for an ENTER nobody is there to press. Startup
                // parameters go before "stress", per the manual.
                let added = vec!["skip-warnings".to_string(), format!("logfile:{}", logfile.display())];
                let mut argv = vec![path_text(&exe)];
                match arguments.iter().position(|argument| argument == "stress") {
                    Some(cut) => {
                        argv.extend(arguments[..cut].iter().cloned());
                        argv.extend(added);
                        argv.extend(arguments[cut..].iter().cloned());
                    }
                    None => {
                        argv.extend(added);
                        argv.extend(arguments.iter().cloned());
                    }
                }
                // -TL is y-cruncher's own limit; the runner allows five minutes
                // past it before stepping in.
                let seconds = arguments
                    .iter()
                    .rev()
                    .find_map(|part| part.to_ascii_lowercase().strip_prefix("-tl:").and_then(|value| value.parse::<u64>().ok()))
                    .map(|limit| limit + BACKSTOP_GRACE_SECONDS)
                    .unwrap_or(0);
                remove_stale(&logfile);
                return Ok(LaunchSpec {
                    argv,
                    cwd,
                    watch_files: vec![logfile],
                    error_key: self.key().into(),
                    summary: format!(
                        "y-cruncher {}: {}",
                        profile.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                        arguments.join(" ")
                    ),
                    duration_seconds: seconds,
                    leave_open: true,
                    new_console: true,
                    ..Default::default()
                });
            }
            _ => {}
        }

        // Exactly the command line a profile saved from this tab would hold, so
        // Start and the profile it saves run the same test, plus skip-warnings
        // and logfile: before "stress". pause:-2 when pause is off, so
        // y-cruncher does not wait on its own default.
        let arguments = YCruncher::profile_arguments(config);
        let cut = arguments.iter().position(|argument| argument == "stress").unwrap_or(0);
        let mut added = vec!["skip-warnings".to_string(), format!("logfile:{}", logfile.display())];
        if !config.flag("pause") {
            added.insert(0, "pause:-2".into());
        }
        let mut argv = vec![path_text(&exe)];
        argv.extend(arguments[..cut].iter().cloned());
        argv.extend(added);
        argv.extend(arguments[cut..].iter().cloned());

        let memory = config.int("memory");
        let per_test = config.int("per_test");
        let minutes = config.int("duration").max(0) as u64;
        let selected: Vec<&String> = arguments[cut..].iter().filter(|argument| canonical(argument).is_some()).collect();
        remove_stale(&logfile);

        // Shown in its own window, or piped and hidden -- never both. A child
        // whose output is redirected leaves its window blank, so the visible case
        // is watched through the log file, which carries the same text.
        let show = config.flag_or("show_window", true);
        Ok(LaunchSpec {
            argv,
            cwd,
            console: !show,
            new_console: show,
            no_window: !show,
            watch_files: if show { vec![logfile] } else { Vec::new() },
            error_key: self.key().into(),
            summary: format!(
                "y-cruncher {}, {}, {}",
                if selected.is_empty() {
                    "all tests".to_string()
                } else {
                    selected.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
                },
                if memory > 0 { format!("{memory} GB") } else { "default memory".into() },
                if per_test > 0 { format!("{per_test}s per test") } else { "default test length".into() }
            ),
            // The backstop, not the limit: -TL is what ends the run.
            duration_seconds: if minutes > 0 { minutes * 60 + BACKSTOP_GRACE_SECONDS } else { 0 },
            // pause:1 holds the window open at the end, which is only visible if
            // the run does not then stop it.
            leave_open: show && config.flag("pause"),
            // A clean exit without this line means the run ended early.
            completion_patterns: errors::YCRUNCHER_COMPLETE.to_vec(),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vt3.bat");
        std::fs::write(&path, "y-cruncher.exe pause:1 priority:0 stress -M:28GB -D:60 -TL:216000 VT3\r\n").unwrap();
        let values = YCruncher.profile_values(&path).unwrap();
        assert_eq!(values.text("algorithms"), "VSTv3");
        assert_eq!(values.int("memory"), 28);
        assert_eq!(values.int("duration"), 3600);
        assert!(values.flag("pause"));
        assert_eq!(values.text("priority"), "Normal");
        assert_eq!(YCruncher::profile_arguments(&values).join(" "), "pause:1 priority:0 stress -M:28GB -D:60 -TL:216000 VSTv3");

        let script = dir.path().join("sft.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\n# A y-cruncher profile.\ncd \"$(dirname \"$0\")\" && exec ./y-cruncher pause:1 stress -TL:1800 SFT\n",
        )
        .unwrap();
        assert_eq!(YCruncher::profile_file_arguments(&script), ["pause:1", "stress", "-TL:1800", "SFT"]);

        // Saved in the user's folder for a y-cruncher that cannot be written beside: the folder,
        // which is named y-cruncher too, is not mistaken for the program.
        let elsewhere = dir.path().join("vt3.sh");
        std::fs::write(
            &elsewhere,
            "#!/bin/sh\ncd '/opt/roch-stresstest/y-cruncher v0.8.7.9547-static' && exec ./y-cruncher pause:1 stress VT3\n",
        )
        .unwrap();
        assert_eq!(YCruncher::profile_file_arguments(&elsewhere), ["pause:1", "stress", "VT3"]);
    }
}
