// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! A launcher and a supervisor. It does not implement a single test: Prime95,
//! y-cruncher, TestMem5, Linpack and the rest do their own work exactly as they
//! would if you started them yourself. What this adds is the part those tools
//! leave to you -- starting them with settings that suit this machine, holding
//! them to a time limit, watching their output for a failure, and stopping the
//! whole process tree when the run ends.
//!
//! Everything below the window is here: [`tools`] says how each program is
//! started, [`runner`] supervises one run on a thread of its own, [`errors`]
//! decides from a tool's output whether the machine just failed, and
//! [`system`] holds what differs between Windows and Linux.

pub mod config;
pub mod errors;
pub mod hardware;
pub mod memory;
pub mod runner;
pub mod selftest;
pub mod settings;
pub mod system;
pub mod tool;
pub mod tools;
#[cfg(windows)]
pub mod winui;

pub const APP_NAME: &str = "Roch StressTest";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
