// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every preset and Quick Start button builds a launch for the tools this
//! repository holds (and, on Linux, the ones fetch-tools.sh put beside them).

#[test]
fn the_selftest_passes_on_this_repository() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (lines, failures) = roch_stresstest::selftest::run(&root);
    println!("{}", lines.join("\n"));
    assert_eq!(failures, 0, "{}", lines.join("\n"));
}
