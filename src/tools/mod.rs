// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! The registry: every tool this program knows how to start, in tab order.
//!
//! The two CPU tests come first because they are what you reach for after a
//! clock change, then the two memory tests, then the two Linpacks -- the
//! harshest -- then OCCT, Cinebench as a load rather than a check, and the two
//! graphics tools last because they test a different piece of hardware.

use std::path::{Path, PathBuf};

use crate::tool::Tool;

pub mod cinebench;
pub mod linpack;
pub mod memtest_vulkan;
pub mod occt;
pub mod prime95;
pub mod ramtest;
pub mod testmem5;
pub mod threedmark11;
pub mod ycruncher;

/// Every tool, whether or not it has a build for this system.
pub fn all_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(prime95::Prime95),
        Box::new(ycruncher::YCruncher),
        Box::new(testmem5::TestMem5),
        Box::new(ramtest::RamTestPro),
        Box::new(linpack::LinpackXtreme),
        Box::new(linpack::LinpackExtended),
        Box::new(occt::Occt),
        Box::new(cinebench::Cinebench),
        Box::new(memtest_vulkan::MemtestVulkan),
        Box::new(threedmark11::ThreeDMark11),
    ]
}

/// The tools with a build for this system. On Linux that is Prime95 (mprime),
/// y-cruncher, OCCT and memtest_vulkan; the rest are Windows programs.
pub fn tools() -> Vec<Box<dyn Tool>> {
    all_tools().into_iter().filter(|tool| tool.supported()).collect()
}

/// Quick Start's two columns, written out rather than split down the middle of
/// the tab order, so related tools sit together. Anything not named here still
/// appears, in whichever column is shorter.
const QUICK_COLUMNS: [&[&str]; 2] = [
    &["cinebench", "prime95", "linpack_extended", "linpack_xtreme", "occt"],
    &["ycruncher", "testmem5", "ramtest", "3dmark11", "memtest_vulkan"],
];

/// Indexes into `tools` for each Quick Start column, in the order shown.
pub fn quick_columns(tools: &[Box<dyn Tool>]) -> [Vec<usize>; 2] {
    let mut columns: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for (side, keys) in QUICK_COLUMNS.iter().enumerate() {
        for key in *keys {
            if let Some(index) = tools.iter().position(|tool| tool.key() == *key) {
                columns[side].push(index);
            }
        }
    }
    for index in 0..tools.len() {
        if !columns.iter().any(|column| column.contains(&index)) {
            let shorter = usize::from(columns[0].len() > columns[1].len());
            columns[shorter].push(index);
        }
    }
    columns
}

/// The folder the tool distributions are unpacked into: beside the
/// executable, or `ROCH_STRESSTEST_TOOLS` when set. A build run from
/// `target/<profile>` uses the repository it was built from.
pub fn tools_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("ROCH_STRESSTEST_TOOLS").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    let in_target = exe_dir.parent().is_some_and(|parent| parent.file_name().is_some_and(|name| name == "target"));
    if in_target && let Some(repo) = exe_dir.parent().and_then(Path::parent) {
        return repo.to_path_buf();
    }
    exe_dir
}
