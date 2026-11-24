// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! The two Linpack packages (Windows): front-ends over the same Intel benchmark.
//!
//! Linpack Xtreme is a console menu that asks for memory, trials and time and
//! picks the right build for the processor itself, so it is opened and
//! answered there. Linpack Extended is a Node driver, and it is used rather
//! than gone around: it chains tests, keeps Min/Avg/Max GFlops per size, and
//! stops on a bad solve or a residual that moved, printing "FAIL - severe
//! instability detected" or "RESIDUAL MISMATCH - instability detected".
//!
//! Its Intel binary refuses AMD processors: it prints "runs on only genuine
//! Intel processors" and exits with status zero, so nothing is tested and
//! nothing looks wrong. Linpack Extended refuses to start on AMD for that
//! reason; Linpack Xtreme ships an AMD build and is the one to use there.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::config::{Config, ConfigExt, as_int};
use crate::hardware;
use crate::settings;
use crate::tool::{
    Field, LaunchSpec, QuickActions, Tool, clean_profile_name, glob_files, glob_in, limit_note, path_text, remove_stale, write_text,
};

/// The leading dimension Intel's documentation asks for: the nearest odd
/// multiple of 16 (8 without AVX) at or above the problem size. Wrong costs
/// throughput rather than correctness -- but 20% slow is 20% less stress.
pub fn leading_dimension(problem_size: i64) -> i64 {
    let step = 16;
    let mut lda = (problem_size + step - 1) / step * step;
    if lda % (step * 2) == 0 {
        lda += step;
    }
    lda
}

/// The largest problem whose matrix (8 * lda * n bytes) fits in `memory_mb`.
pub fn problem_size_for(memory_mb: f64) -> i64 {
    let target = memory_mb.max(1.0) * 1024.0 * 1024.0;
    let step = 16;
    let mut size = (target / 8.0).sqrt() as i64;
    size -= size % step;
    while size > step {
        if (8 * leading_dimension(size) * size) as f64 <= target {
            return size;
        }
        size -= step;
    }
    step
}

pub struct LinpackXtreme;

impl Tool for LinpackXtreme {
    fn key(&self) -> &'static str {
        "linpack_xtreme"
    }

    fn name(&self) -> &'static str {
        "Linpack Xtreme"
    }

    fn blurb(&self) -> &'static str {
        "The heaviest sustained AVX load of the set, and the quickest way to find a core or memory setting that is only \
         nearly stable. This is the package to use on AMD -- it ships a build that runs there. Watch temperatures: nothing \
         else here pulls this much current."
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }

    /// The menu, not the binary underneath: it picks the build for the CPU.
    fn exe_globs(&self) -> Vec<&'static str> {
        vec!["LinpackXtreme*/LinpackXtreme_x64.exe", "LinpackXtreme*/LinpackXtreme_x32.exe"]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0, 0, 100_000, "min", "0 lets the trials you asked it for finish in their own time.")]
    }

    fn has_tab(&self) -> bool {
        false
    }

    fn quick_actions(&self, root: &Path) -> QuickActions {
        vec![("Open".into(), self.quick_config(root))]
    }

    fn quick_summary(&self, root: &Path) -> String {
        limit_note(&self.quick_config(root))
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String> {
        let exe = self.locate(root).ok_or("LinpackXtreme_x64.exe was not found. Expected its folder beside this program.")?;
        // Its own console, nothing read from it: the front-end keeps no log.
        Ok(LaunchSpec {
            argv: vec![path_text(&exe)],
            cwd: exe.parent().unwrap_or(root).to_path_buf(),
            error_key: self.key().into(),
            summary: "Linpack Xtreme (settings answered at its menu)".into(),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            leave_open: true,
            new_console: true,
            ..Default::default()
        })
    }
}

/// cmd has no tee, and the driver has no log option, so its output would be on
/// screen or readable, never both. The package ships node, so node splits it.
const TEE_JS: &str = "// Written by Roch StressTest. cmd has no tee, and Linpack Extended has\n// no log option, so its output is split here: on to the console it is\n// running in, and into a file the runner can read.\nconst fs = require(\"fs\");\nconst out = fs.createWriteStream(process.argv[2]);\nprocess.stdin.on(\"data\", function (chunk) {\n  process.stdout.write(chunk);\n  out.write(chunk);\n});\nprocess.stdin.on(\"end\", function () { out.end(); });\n";

/// One test in the chain, as config.json holds it.
pub const TEST_KEYS: [(&str, i64); 4] = [("minutes", 30), ("problem size", 38736), ("leading dimension", 38736), ("alignment value", 4)];

/// The config Quick Start describes and the tab opens on, when it exists.
const QUICK_PROFILE: &str = "11GB";
/// Always first: the package's own config.
const DEFAULT_PROFILE: &str = "Default";

pub struct LinpackExtended;

/// One test with every value present and the leading dimension sane: 0 means
/// work it out, and one smaller than the problem cannot be used.
pub fn clean_test(test: &Map<String, Value>) -> Map<String, Value> {
    let mut clean = Map::new();
    for (key, default) in TEST_KEYS {
        clean.insert(key.into(), Value::from(test.get(key).and_then(as_int).unwrap_or(default)));
    }
    let get = |map: &Map<String, Value>, key: &str| map.get(key).and_then(as_int).unwrap_or(0);
    clean.insert("minutes".into(), Value::from(get(&clean, "minutes").max(1)));
    clean.insert("problem size".into(), Value::from(get(&clean, "problem size").max(1000)));
    if get(&clean, "leading dimension") < get(&clean, "problem size") {
        let lda = leading_dimension(get(&clean, "problem size"));
        clean.insert("leading dimension".into(), Value::from(lda));
    }
    clean
}

/// What one test's matrix takes, which is most of what it uses.
pub fn test_memory_gb(test: &Map<String, Value>) -> f64 {
    let size = test.get("problem size").and_then(as_int).unwrap_or(0);
    let lda = test.get("leading dimension").and_then(as_int).filter(|lda| *lda > 0).unwrap_or(size);
    8.0 * lda as f64 * size as f64 / 1024f64.powi(3)
}

pub fn default_test() -> Map<String, Value> {
    TEST_KEYS.iter().map(|(key, value)| (key.to_string(), Value::from(*value))).collect()
}

impl LinpackExtended {
    fn package_dir(&self, root: &Path) -> Option<PathBuf> {
        // .../dependencies/linpack/linpack_xeon64.exe
        self.locate(root)?.parent()?.parent()?.parent().map(Path::to_path_buf)
    }

    fn tests_of(config: &Config) -> Vec<Map<String, Value>> {
        config
            .get("tests")
            .and_then(Value::as_array)
            .map(|tests| tests.iter().filter_map(|test| test.as_object().cloned()).collect())
            .unwrap_or_default()
    }

    /// The config.json these settings make, in the format linpack.js reads.
    pub fn config_json(config: &Config) -> Value {
        let mut chain: Vec<Map<String, Value>> = LinpackExtended::tests_of(config).iter().map(clean_test).collect();
        if chain.is_empty() {
            chain.push(default_test());
        }
        let mut block = Map::new();
        block.insert("reduce output below X problem size".into(), Value::from(config.int("reduce_below")));
        block.insert("track stats below X problem size".into(), Value::from(config.int("track_below")));
        block.insert("stop after residual mismatch".into(), Value::Bool(config.flag("residual_check")));
        // Read as `config.settings.KMP_AFFINITY ?? ""`, so leaving the key out is
        // the same as an empty one, and is what configs passed around look like.
        let affinity = config.text("affinity").trim().to_string();
        if !affinity.is_empty() {
            block.insert("KMP_AFFINITY".into(), Value::from(affinity));
        }
        let tests: Map<String, Value> =
            chain.into_iter().enumerate().map(|(index, test)| ((index + 1).to_string(), Value::Object(test))).collect();
        json!({
            "test order": (1..=tests.len()).collect::<Vec<_>>(),
            "settings": block,
            "tests": tests,
        })
    }

    fn config_text(config: &Config) -> String {
        // Indented as Python's json.dumps(indent=2) did, which is also the
        // package's own style.
        serde_json::to_string_pretty(&LinpackExtended::config_json(config)).unwrap_or_default() + "\n"
    }

    fn run_options() -> Config {
        let mut options = Config::new();
        options.insert("threads".into(), Value::from(hardware::logical_cores() as i64));
        options.insert("show_window".into(), Value::Bool(true));
        options
    }

    pub fn quick_profile_name(&self, root: &Path) -> String {
        let names: Vec<String> = self.profiles(root).unwrap_or_default().into_iter().map(|(name, _)| name).collect();
        if names.iter().any(|name| name == QUICK_PROFILE) { QUICK_PROFILE.into() } else { names.into_iter().next().unwrap_or_default() }
    }
}

impl Tool for LinpackExtended {
    fn key(&self) -> &'static str {
        "linpack_extended"
    }

    fn name(&self) -> &'static str {
        "Linpack Extended"
    }

    fn blurb(&self) -> &'static str {
        "The same Intel benchmark from the Linpack Extended package. Intel processors only -- its binary refuses to run \
         anywhere else. On AMD, use Linpack Xtreme instead."
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        vec!["Linpack-Extended*/dependencies/linpack/linpack_xeon64.exe"]
    }

    fn unsupported_reason(&self, _root: &Path) -> String {
        if hardware::is_amd() {
            return "Linpack Extended ships the Intel-only build (linpack_xeon64.exe), which refuses to run on an AMD \
                    processor -- it prints \"runs on only genuine Intel processors\" and exits without testing anything. \
                    Use Linpack Xtreme on this machine; it ships an AMD build."
                .into();
        }
        String::new()
    }

    fn fields(&self) -> Vec<Field> {
        let threads = hardware::logical_cores() as i64;
        vec![
            Field::flag(
                "residual_check",
                "Stop on residual mismatch",
                false,
                "\"stop after residual mismatch\". A residual that changes between identical trials is an error even when \
                 the check column still says pass.",
            ),
            Field::text(
                "affinity",
                "KMP_AFFINITY",
                "",
                "Blank leaves it out, and is the package's own answer to an OMP error at startup. It is also the only way \
                 the Threads setting below reaches the benchmark: when it is set, linpack.js replaces the environment with it.",
            ),
            Field::int(
                "reduce_below",
                "Reduce output below",
                0,
                0,
                200_000,
                "n",
                "\"reduce output below X problem size\". Quieter console for small tests. 0 is off.",
            ),
            Field::int(
                "track_below",
                "Track stats below",
                0,
                0,
                200_000,
                "n",
                "\"track stats below X problem size\". Min/Avg/Max GFlops for tests under this size. 0 is off.",
            ),
            Field::int("threads", "Threads", threads, 1, 512, "", "Not part of the config. Only used while KMP_AFFINITY is blank."),
            Field::flag(
                "show_window",
                "Show Linpack's window",
                true,
                "Not part of the config. Its output is copied to a file at the same time, so nothing stops being checked.",
            ),
        ]
    }

    fn quick_config(&self, root: &Path) -> Config {
        let mut config = self.defaults();
        config.insert("tests".into(), Value::Array(vec![Value::Object(default_test())]));
        let profiles = self.profiles(root).unwrap_or_default();
        let name = self.quick_profile_name(root);
        if let Some((_, path)) = profiles.iter().find(|(candidate, _)| *candidate == name)
            && let Ok(values) = self.profile_values(path)
        {
            config.extend(values);
        }
        config.extend(LinpackExtended::run_options());
        config
    }

    fn quick_actions(&self, root: &Path) -> QuickActions {
        let mut actions = Vec::new();
        for (name, path) in self.profiles(root).unwrap_or_default() {
            if let Ok(values) = self.profile_values(&path) {
                let mut config = self.defaults();
                config.extend(values);
                config.extend(LinpackExtended::run_options());
                actions.push((name, config));
            }
        }
        if actions.is_empty() {
            actions.push(("Start".into(), self.quick_config(root)));
        }
        actions
    }

    fn quick_summary(&self, root: &Path) -> String {
        let names: Vec<String> = self.profiles(root).unwrap_or_default().into_iter().map(|(name, _)| name).collect();
        if names.is_empty() {
            "No saved configs -- make one on the Linpack Extended tab.".into()
        } else {
            format!("Run {}", names.join(" / "))
        }
    }

    fn profiles(&self, root: &Path) -> Option<Vec<(String, PathBuf)>> {
        let Some(package) = self.package_dir(root) else { return Some(Vec::new()) };
        let mut found: Vec<(String, PathBuf)> = glob_files(&glob_in(&package.join("profiles"), "*.json"), true)
            .into_iter()
            .map(|path| (path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(), path))
            .collect();
        found.sort_by_key(|(name, _)| (!name.eq_ignore_ascii_case(DEFAULT_PROFILE), name.to_lowercase()));
        Some(found)
    }

    /// A config.json read back into this tab's settings.
    fn profile_values(&self, path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        let data: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        let block = data.get("settings").and_then(Value::as_object).cloned().unwrap_or_default();
        let tests = data.get("tests").and_then(Value::as_object).cloned().unwrap_or_default();
        let order: Vec<String> = match data.get("test order").and_then(Value::as_array) {
            Some(order) => order
                .iter()
                .map(|key| key.as_i64().map(|n| n.to_string()).or_else(|| key.as_str().map(str::to_string)).unwrap_or_default())
                .collect(),
            None => {
                let mut keys: Vec<String> = tests.keys().cloned().collect();
                keys.sort();
                keys
            }
        };
        let chain: Vec<Value> = order
            .iter()
            .filter_map(|key| tests.get(key).and_then(Value::as_object))
            .map(|test| {
                Value::Object(
                    TEST_KEYS
                        .iter()
                        .map(|(name, default)| (name.to_string(), Value::from(test.get(*name).and_then(as_int).unwrap_or(*default))))
                        .collect(),
                )
            })
            .collect();
        let mut values = Config::new();
        values.insert("tests".into(), Value::Array(chain));
        values.insert(
            "residual_check".into(),
            Value::Bool(block.get("stop after residual mismatch").and_then(Value::as_bool).unwrap_or(false)),
        );
        values.insert("affinity".into(), Value::from(block.get("KMP_AFFINITY").and_then(Value::as_str).unwrap_or("")));
        values.insert("reduce_below".into(), Value::from(block.get("reduce output below X problem size").and_then(as_int).unwrap_or(0)));
        values.insert("track_below".into(), Value::from(block.get("track stats below X problem size").and_then(as_int).unwrap_or(0)));
        Ok(values)
    }

    fn save_profile(&self, root: &Path, name: &str, config: &Config) -> Result<PathBuf, String> {
        let package = self.package_dir(root).ok_or("A config needs Linpack Extended.")?;
        let name = clean_profile_name(name);
        if name.is_empty() {
            return Err("A config needs a name.".into());
        }
        let path = package.join("profiles").join(format!("{name}.json"));
        write_text(&path, &LinpackExtended::config_text(config))?;
        Ok(path)
    }

    fn profile_hint(&self) -> String {
        "Saved as <name>.json in Linpack Extended's profiles folder, and shown as a button on Quick Start.".into()
    }

    fn profile_preview(&self, config: &Config) -> String {
        LinpackExtended::config_text(config).trim_end().to_string()
    }

    fn profile_preview_label(&self) -> Option<&'static str> {
        Some("config.json")
    }

    fn has_test_chain(&self) -> bool {
        true
    }

    /// Run the package the way it runs itself: the chosen config written to
    /// config.json (whose path linpack.js has built in), the shipped one copied
    /// aside the first time, then `node linpack.js`.
    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String> {
        let exe = self.locate(root).ok_or("Linpack Extended's binary was not found. Expected its folder beside this program.")?;
        let blocked = self.unsupported_reason(root);
        if !blocked.is_empty() {
            return Err(blocked);
        }
        let dependencies = exe.parent().and_then(Path::parent).ok_or("Linpack Extended's folder is not as expected.")?.to_path_buf();
        let package = dependencies.parent().unwrap_or(&dependencies).to_path_buf();
        let node = dependencies.join("node").join("node.exe");
        let driver = dependencies.join("linpack.js");
        for (needed, what) in [(&node, "node.exe"), (&driver, "linpack.js")] {
            if !needed.is_file() {
                return Err(format!("Linpack Extended is incomplete: {what} is missing from its dependencies folder."));
            }
        }
        let settings_path = package.join("config.json");
        let original = package.join("config.json.roch-original");
        if settings_path.exists() && !original.exists() {
            let _ = std::fs::copy(&settings_path, &original);
        }
        let document = LinpackExtended::config_json(config);
        write_text(&settings_path, &(serde_json::to_string_pretty(&document).unwrap_or_default() + "\n"))?;
        let chain: Vec<Map<String, Value>> = document["tests"]
            .as_object()
            .map(|tests| tests.values().filter_map(|test| test.as_object().cloned()).collect())
            .unwrap_or_default();

        let work = settings::run_dir("linpack");
        let log = work.join("linpack-extended-output.txt");
        remove_stale(&log);

        // What "Linpack Extended.bat" sets: its node refuses a recent Windows
        // without it. The thread counts reach the benchmark only while
        // KMP_AFFINITY is blank -- the one case linpack.js leaves the child's
        // environment alone -- and are harmless otherwise.
        let threads = config.int_or("threads", hardware::logical_cores() as i64);
        let env = vec![
            ("NODE_SKIP_PLATFORM_CHECK".to_string(), "1".to_string()),
            ("OMP_NUM_THREADS".to_string(), threads.to_string()),
            ("MKL_NUM_THREADS".to_string(), threads.to_string()),
        ];
        let minutes = |test: &Map<String, Value>| test.get("minutes").and_then(as_int).unwrap_or(0);
        let summary = format!(
            "Linpack Extended {}, {threads} threads",
            chain
                .iter()
                .map(|test| format!(
                    "n={} ({:.1} GB, {} min)",
                    test.get("problem size").and_then(as_int).unwrap_or(0),
                    test_memory_gb(test),
                    minutes(test)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        // linpack.js ends by itself once the last test's minutes are up and says
        // so; this is the countdown, with a minute of room.
        let seconds = chain.iter().map(minutes).sum::<i64>().max(0) as u64 * 60 + 60;
        let complete = vec!["All tests successfully passed"];

        if !config.flag_or("show_window", true) {
            return Ok(LaunchSpec {
                argv: vec![path_text(&node), path_text(&driver)],
                cwd: dependencies,
                env,
                console: true,
                no_window: true,
                error_key: self.key().into(),
                summary,
                duration_seconds: seconds,
                completion_patterns: complete,
                ..Default::default()
            });
        }
        let tee = work.join("tee.js");
        write_text(&tee, TEE_JS)?;
        let raw = format!(
            "/c \"\"{}\" \"{}\" 2>&1 | \"{}\" \"{}\" \"{}\"\"",
            node.display(),
            driver.display(),
            node.display(),
            tee.display(),
            log.display()
        );
        Ok(LaunchSpec {
            argv: vec!["cmd".into()],
            cmdline: Some(raw),
            cwd: dependencies,
            env,
            new_console: true,
            watch_files: vec![log],
            error_key: self.key().into(),
            summary,
            duration_seconds: seconds,
            completion_patterns: complete,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eleven_gigabytes_is_problem_size_38736() {
        assert_eq!(problem_size_for(11448.0), 38736);
        assert_eq!(leading_dimension(38736), 38736 + 16 * ((38736 % 32 == 0) as i64));
        assert_eq!(leading_dimension(38720) % 32, 16);
    }

    #[test]
    fn a_config_round_trips_through_config_json() {
        let mut config = Config::new();
        config.insert("tests".into(), json!([{"minutes": 30, "problem size": 38736, "leading dimension": 38736, "alignment value": 4}]));
        config.insert("residual_check".into(), Value::Bool(true));
        let text = LinpackExtended::config_text(&config);
        assert!(text.starts_with("{\n  \"test order\": [\n    1\n  ],\n  \"settings\": {"), "{text}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        std::fs::write(&path, &text).unwrap();
        let back = LinpackExtended.profile_values(&path).unwrap();
        assert!(back.flag("residual_check"));
        assert_eq!(LinpackExtended::tests_of(&back)[0]["problem size"], 38736);
    }
}
