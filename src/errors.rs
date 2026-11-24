// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deciding, from a tool's output, whether the machine just failed.
//!
//! A stress test is only useful if somebody notices the failure. Half of these
//! tools announce a problem in a window nobody is watching at 3am, and one of
//! them (Linpack) does not announce it at all -- it prints a table and the
//! failure is a word in the eighth column. So every tool's output is scanned
//! here, by the same code, against patterns that mean "this run is over".
//!
//! Two rules keep the pattern lists honest:
//!
//! * Match what the tool prints when it *fails*, not every line containing the
//!   word "error". Prime95's banner mentions error checking; y-cruncher prints
//!   an error-tolerance figure at startup. Both would fire a naive `\berror\b`.
//! * Never match on a count of zero. "0 errors" is the line a passing TM5 run
//!   writes, and it contains the word.

use std::sync::OnceLock;

use regex::{Regex, RegexBuilder};

/// Common to every tool: a crash of the child process shows up as text before
/// the process dies.
const COMMON: &[&str] = &[r"\bSTATUS_ACCESS_VIOLATION\b", r"\bunhandled exception\b"];

/// Prime95 (and mprime) write these to results.txt. "Torture Test completed"
/// with a non-zero error count is the summary line; the others appear the
/// moment a worker finds a bad result.
const PRIME95: &[&str] = &[
    r"FATAL ERROR",
    r"[Hh]ardware failure",
    r"possible hardware failure",
    r"ILLEGAL SUMOUT",
    r"SUMOUT MISMATCH",
    r"ROUND OFF",
    r"Torture Test completed.*?[1-9]\d*\s+errors",
    r"self-test.*?fail",
];

/// y-cruncher's stress tester stops on the first bad comparison and says so.
/// "Coefficient is too large" catches an arithmetic slip that has not yet
/// corrupted a checksum.
const YCRUNCHER: &[&str] = &[
    // y-cruncher prints "Running VSTv3: Passed" once per iteration, so the
    // negative of that line is the signal that matters most.
    r"Running\s+\S+\s*:\s*(?:FAIL|Failed|ERROR)",
    r"\bFAILED\b",
    r"Redundancy Check Failed",
    r"Coefficient is too large",
    r"\bhardware (?:error|failure)\b",
    r"An error (?:has )?occurred",
    r"Validation Fail",
    // Anchored, because y-cruncher's startup menu prints the row
    // "6   Stop on Error:      Enabled" and an unanchored "ERROR:" fails every
    // run before the first test has even started.
    r"^\s*ERROR\s*:",
    r"Computation Error",
    r"does not match",
];

/// TestMem5 0.13.1 writes Log.txt beside TM5.exe. These are its own message
/// templates, lifted from the strings in TM5.exe and TM5.dll rather than
/// guessed:
///
/// ```text
/// "Error in test #%li through %s."
/// "The testing is completed, is revealed %li of errors!"
/// "detected %li error(s)."
/// "Critical error, programm stopped!"   (its spelling)
/// ```
///
/// and the ones that must NOT fire, which are the same shape: "Testing
/// completed in %s, no errors.", "The testing is completed, of errors is not
/// detected.", "Testing stopped by user". TM5 also separates a failure of the
/// memory under test from a failure of TM5 itself -- "Failed to allocate memory
/// for testing", "WARNING! Failed to lock memory" -- and says so: "This is not a
/// failure of tested memory." Those are never reported as instability.
const TESTMEM5: &[&str] = &[
    r"Error in test",
    r"is revealed [1-9]\d* of errors",
    r"detected [1-9]\d* error",
    r"(?:Critical|Fatal) error, programm stopped",
    r"encountered an error, type",
    r"caused by a failure of tested memory",
];

/// RAM Test Pro's logs/log.txt. "Test errors detected: 0" is the verdict of a
/// clean run; "ERROR! Free up RAM or reduce memory size for test!" is RAM Test
/// Pro failing to get its memory, a setting to fix rather than a DIMM to blame,
/// and deliberately does not match.
const RAMTEST: &[&str] = &[r"Test errors detected:\s*[1-9]\d*", r"^\s*ERROR\s*=", r"^\s*ERROR\s+\d"];

/// Generic memory-test wording, kept for anything not covered above.
const MEMTEST: &[&str] = &[
    // "4 errors" fails; "0 errors" -- the line a clean cycle writes every pass
    // -- cannot match, because the count may not start with a zero.
    r"\b[1-9]\d*\s+errors?\b",
    r"\berrors?\b\s*[:=]\s*[1-9]\d*",
    r"\bERROR\b[^\n]{0,40}\baddress\b",
    r"memory error",
    r"\bBAD\b[^\n]{0,20}\bmemory\b",
];

/// Linpack is handled structurally by [`scan_linpack_row`] -- the failure is a
/// column, not a phrase -- but Linpack Extended's driver also prints these.
const LINPACK: &[&str] = &[r"\bFAIL\b", r"severe instability", r"RESIDUAL MISMATCH"];

/// Cinebench renders and scores; it has no notion of a wrong answer. What it
/// does have is the ways it can fail to run, and a driver that falls over
/// mid-render is exactly the instability worth catching.
const CINEBENCH: &[&str] = &[r"Error loading", r"could not be (?:created|initialized)", r"rendering failed", r"out of memory"];

/// 3DMark's command-line runner reports a failed workload rather than a score.
const THREEDMARK: &[&str] = &[r"^Error:", r"workload .*failed", r"device (?:removed|hung|reset)", r"DXGI_ERROR"];

/// memtest_vulkan's own wording, out of the binary. Every good pass prints
/// "iteration. Passed"; a bad one prints "Error found. Mode ..., total errors"
/// and then the address range.
const MEMTEST_VULKAN: &[&str] = &[r"Error found", r"Errors address range", r"error found", r"^\s*FAILED"];

/// TM5 finishes its cycles and then sits there with its window open, so "done"
/// is read out of the log rather than waited for.
pub const TESTMEM5_COMPLETE: &[&str] = &[r"Testing completed", r"The testing is completed, of errors is not detected"];

/// y-cruncher's last line when a stress run ends of its own accord. Without it
/// a run that ended early exits cleanly and looks like one that ran to the end.
pub const YCRUNCHER_COMPLETE: &[&str] = &[r"Test Finished"];

/// memtest_vulkan stops only when told to, so any ending it announces is the
/// end of the run rather than a verdict.
pub const MEMTEST_VULKAN_ABORTED: &[&str] = &[r"Test cancelled, no device selected", r"early exit during init"];

/// Lines that look like a failure and are not one, checked first. Cinebench
/// R24 and R26 narrate every module they cannot register at startup -- "could
/// not be initialized because ... is missing" -- as WARNING lines, before
/// anything is rendered, and then run perfectly well.
const NOT_FAILURES_CINEBENCH: &[&str] = &[r"^WARNING:"];

/// Compiles a list case-insensitively, without the common crash patterns.
pub fn compile_plain(patterns: &[&str]) -> Vec<Regex> {
    patterns.iter().map(|pattern| RegexBuilder::new(pattern).case_insensitive(true).build().expect("a valid pattern")).collect()
}

fn compile(patterns: &[&[&str]]) -> Vec<Regex> {
    let mut all: Vec<&str> = patterns.iter().flat_map(|list| list.iter().copied()).collect();
    all.extend_from_slice(COMMON);
    compile_plain(&all)
}

struct Tables {
    by_key: Vec<(&'static str, Vec<Regex>)>,
    common: Vec<Regex>,
    excused_cinebench: Vec<Regex>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| Tables {
        by_key: vec![
            ("prime95", compile(&[PRIME95])),
            ("ycruncher", compile(&[YCRUNCHER])),
            ("testmem5", compile(&[TESTMEM5, MEMTEST])),
            ("ramtest", compile(&[RAMTEST])),
            ("linpack", compile(&[LINPACK])),
            ("linpack_extended", compile(&[LINPACK])),
            ("linpack_xtreme", compile(&[LINPACK])),
            ("memtest_vulkan", compile(&[MEMTEST_VULKAN])),
            ("cinebench", compile(&[CINEBENCH])),
            ("3dmark11", compile(&[THREEDMARK])),
        ],
        common: compile(&[]),
        excused_cinebench: compile_plain(NOT_FAILURES_CINEBENCH),
    })
}

/// The first non-empty line of `text` matching any of `compiled`.
pub fn matches(text: &str, compiled: &[Regex]) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| compiled.iter().any(|pattern| pattern.is_match(line)))
        .map(str::to_string)
}

/// The first line of `text` that means this run has failed, or None.
pub fn scan(text: &str, tool_key: &str) -> Option<String> {
    let tables = tables();
    let compiled = tables.by_key.iter().find(|(key, _)| *key == tool_key).map(|(_, list)| list).unwrap_or(&tables.common);
    let excused: &[Regex] = if tool_key == "cinebench" { &tables.excused_cinebench } else { &[] };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !excused.iter().any(|pattern| pattern.is_match(line)))
        .find(|line| compiled.iter().any(|pattern| pattern.is_match(line)))
        .map(str::to_string)
}

/// One Linpack result row: GFlops, the residual as printed, and whether the
/// check column says "pass".
///
/// Linpack keeps running after a bad solve and simply writes something other
/// than "pass" in that column, so a run watched only for crashes looks clean.
pub fn scan_linpack_row(line: &str) -> Option<(f64, String, bool)> {
    static ROW: OnceLock<Regex> = OnceLock::new();
    let row = ROW
        .get_or_init(|| Regex::new(r"^\s*(\d+)\s+(\d+)\s+(\d+)\s+([\d.]+)\s+([\d.]+)\s+([\d.eE+-]+)\s+([\d.eE+-]+)\s+(\w+)\s*$").unwrap());
    let captures = row.captures(line)?;
    let gflops: f64 = captures[5].parse().ok()?;
    Some((gflops, captures[6].to_string(), captures[8].trim().eq_ignore_ascii_case("pass")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_caught_and_clean_lines_are_not() {
        assert!(scan("Running VSTv3: Passed  Test Speed: 2.0", "ycruncher").is_none());
        assert!(scan("Running VSTv3: FAILED", "ycruncher").is_some());
        assert!(scan("6   Stop on Error:      Enabled", "ycruncher").is_none());
        assert!(scan("ERROR: Redundancy Check Failed", "ycruncher").is_some());
        assert!(scan("Testing completed in 1:02:03, no errors.", "testmem5").is_none());
        assert!(scan("Error in test #3 through 0:01:23.", "testmem5").is_some());
        assert!(scan("0 errors", "testmem5").is_none());
        assert!(scan("4 errors", "testmem5").is_some());
        assert!(scan("Test errors detected: 0", "ramtest").is_none());
        assert!(scan("Test errors detected: 2", "ramtest").is_some());
        assert!(scan("ERROR! Free up RAM or reduce memory size for test!", "ramtest").is_none());
        assert!(scan("[Worker #1] FATAL ERROR: Rounding was 0.4990028871", "prime95").is_some());
        assert!(scan("Self-test 960K passed!", "prime95").is_none());
        assert!(scan("     713 iteration. Passed 30.0488 seconds", "memtest_vulkan").is_none());
        assert!(scan("Error found. Mode NEXT_RE_READ, total errors 0x1", "memtest_vulkan").is_some());
        assert!(scan("WARNING: x could not be initialized because y is missing", "cinebench").is_none());
        assert!(scan("Error loading the renderer", "cinebench").is_some());
    }

    #[test]
    fn linpack_rows_are_read_by_column() {
        let row = "38736  38736  4  1234.567  987.6543  1.234e-09  3.21e-02  pass";
        let (gflops, residual, passed) = scan_linpack_row(row).unwrap();
        assert!((gflops - 987.6543).abs() < 1e-9);
        assert_eq!(residual, "1.234e-09");
        assert!(passed);
        assert!(!scan_linpack_row(&row.replace("pass", "fail")).unwrap().2);
        assert!(scan_linpack_row("Size   LDA    Align.  Time(s)    GFlops").is_none());
    }
}
