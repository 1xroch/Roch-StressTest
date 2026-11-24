// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build every preset and every Quick Start button, and check each produces a
//! real launch.
//!
//! A launcher fails in a way unit tests miss: a preset naming a field the tool
//! does not have, or a profile on disk that no longer parses, breaks only when
//! somebody presses that button. This walks every combination the window can
//! produce and builds the launch for each, so that failure happens here. Nothing
//! is executed; building does write the tools' own configuration files (prime.txt
//! in this program's run folder, Linpack Extended's config.json), as pressing
//! the button would.
//!
//! `RochStressTest --selftest` prints the report and exits non-zero on a failure.

use std::path::Path;

use crate::tool::Tool;
use crate::tools;

/// The report, one line per check, and how many failed.
pub fn run(root: &Path) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    let mut failures = 0;
    for tool in tools::tools() {
        let name = tool.name();
        if !tool.available(root) {
            lines.push(format!("SKIP {name} (not found)"));
            continue;
        }
        let blocked = tool.unsupported_reason(root);
        if !blocked.is_empty() {
            // A tool that cannot run here must refuse to build, not build
            // something that will exit having done nothing.
            match tool.build(&tool.quick_config(root), root) {
                Err(_) => lines.push(format!("OK   {name:<17} refuses to run here")),
                Ok(_) => {
                    failures += 1;
                    lines.push(format!("FAIL {name:<17} is blocked but still built a launch"));
                }
            }
            continue;
        }
        let mut configs: Vec<(String, crate::config::Config)> =
            tool.quick_actions(root).into_iter().map(|(label, config)| (format!("button {label}"), config)).collect();
        for preset in tool.presets(root) {
            let mut config = tool.defaults();
            config.extend(preset.values.clone());
            configs.push((format!("preset {}", preset.name), config));
        }
        for (label, config) in configs {
            match check(tool.as_ref(), &config, root) {
                Ok(summary) => lines.push(format!("OK   {name:<17} {label:<22} {summary}")),
                Err(error) => {
                    failures += 1;
                    lines.push(format!("FAIL {name:<17} {label:<22} {error}"));
                }
            }
        }
    }
    lines.push(String::new());
    lines.push(format!("failures: {failures}"));
    (lines, failures)
}

fn check(tool: &dyn Tool, config: &crate::config::Config, root: &Path) -> Result<String, String> {
    let spec = tool.build(config, root)?;
    let program = spec.argv.first().ok_or("no command")?;
    if spec.cmdline.is_none() && !Path::new(program).is_file() {
        return Err(format!("{program} is not a file"));
    }
    if !spec.cwd.is_dir() {
        return Err(format!("{} is not a folder", spec.cwd.display()));
    }
    Ok(spec.summary)
}
