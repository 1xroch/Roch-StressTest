// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding a running tool's windows (Windows).
//!
//! A windowed tool that is closed mid-run often exits with an access violation
//! -- Prime95 closing with eight workers and 28 GB in flight does it every time
//! -- and on its own the code says nothing about which happened. What separates
//! a crash from somebody shutting the window is whether the window went first,
//! so the runner watches for it here.

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};

struct Search {
    pid: u32,
    found: Vec<usize>,
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: lparam is the &mut Search passed to EnumWindows below, alive for
    // the whole enumeration; the window handle comes from the system.
    unsafe {
        let search = &mut *(lparam as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        // Filtered by owner before anything else is asked, so no message ever
        // goes to another program's window.
        if pid == search.pid && IsWindowVisible(hwnd) != 0 {
            search.found.push(hwnd as usize);
        }
    }
    1
}

/// Visible top-level windows belonging to `pid`.
pub fn windows_of(pid: u32) -> Vec<usize> {
    let mut search = Search { pid, found: Vec::new() };
    // SAFETY: the callback only reads window ownership and writes into the
    // Search, which outlives the call.
    unsafe {
        EnumWindows(Some(visit), (&raw mut search) as LPARAM);
    }
    search.found
}
