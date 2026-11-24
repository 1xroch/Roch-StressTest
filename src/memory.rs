// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Freeing memory back to the system (Windows), and the live readout.
//!
//! A memory test only tests memory it can actually get. Windows keeps a
//! standby list -- pages it has finished with but holds in case they are wanted
//! again -- and counts them as in use for an allocation. Ask y-cruncher for
//! 28 GB on a machine that has been up a week and part of that goes to the page
//! file, so the run measures an SSD rather than the DIMMs.
//!
//! Three operations free it, through `NtSetSystemInformation` with
//! SystemMemoryListInformation, the call every memory cleaner uses: empty every
//! working set, purge the standby list, purge the low-priority standby list.
//! They need SeProfileSingleProcessPrivilege, which an administrator has but
//! which is not enabled in the token until asked for. Nothing here is
//! destructive: the purged pages are cached copies of data already on disk.
//!
//! Linux has nothing to clean. Its free figure, MemAvailable, already counts the
//! page cache as available -- the kernel hands it over the moment an allocation
//! wants it -- so the window offers no button there.

use crate::hardware;

/// Whether this system has a cleaner to offer.
pub const SUPPORTED: bool = cfg!(windows);

/// What a clean achieved.
#[derive(Clone, Debug)]
pub struct CleanResult {
    pub before_mb: u64,
    pub after_mb: u64,
    pub done: Vec<&'static str>,
    pub failed: Vec<&'static str>,
}

impl CleanResult {
    pub fn describe(&self) -> String {
        let mut text = format!("Memory cleaned: {} MB free before, {} MB after", group(self.before_mb), group(self.after_mb));
        let freed = self.after_mb.saturating_sub(self.before_mb);
        if freed > 0 {
            text += &format!(" ({} MB freed)", group(freed));
        } else {
            text += " (nothing to free)";
        }
        if !self.failed.is_empty() {
            text += &format!(". Could not clear: {}", self.failed.join(", "));
        }
        text
    }
}

/// A number with thousands separators, as the readout shows it.
pub fn group(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// What each step is called in the report, in the order run. Working sets
/// first: trimming processes moves their pages onto the standby list, where the
/// next step can free them.
const STEPS: [(&str, i32); 3] = [("working sets", 2), ("standby list", 4), ("low-priority standby list", 5)];

/// Empty working sets and purge the standby lists. Never fails: what could not
/// be done is in the result.
pub fn clean() -> CleanResult {
    let before = hardware::available_ram_mb();
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut done = Vec::new();
    let mut failed = Vec::new();
    #[cfg(windows)]
    {
        win::enable_privilege("SeProfileSingleProcessPrivilege");
        win::enable_privilege("SeIncreaseQuotaPrivilege");
        for (label, command) in STEPS {
            if win::set_memory_list(command) {
                done.push(label);
            } else {
                failed.push(label);
            }
        }
    }
    #[cfg(not(windows))]
    failed.extend(STEPS.iter().map(|(label, _)| *label));
    CleanResult { before_mb: before, after_mb: hardware::available_ram_mb(), done, failed }
}

/// (available MB, total MB, percent used) for the live display.
pub fn reading() -> (u64, u64, u64) {
    let total = hardware::total_ram_mb();
    let available = hardware::available_ram_mb();
    let used = (total.saturating_sub(available) * 100 + total / 2).checked_div(total).unwrap_or(0);
    (available, total, used)
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, LUID};
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
        TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 0x50;

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSetSystemInformation(class: i32, information: *const core::ffi::c_void, length: u32) -> i32;
    }

    /// Turn on one privilege in this process's token. Being an administrator
    /// is not the same as having it enabled: it is present but off until asked
    /// for, and the memory call fails with a plain access-denied without it.
    pub fn enable_privilege(name: &str) -> bool {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: the token handle is checked and closed; every pointer is to a
        // local that outlives the call.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut luid: LUID = std::mem::zeroed();
            let ok = LookupPrivilegeValueW(std::ptr::null(), wide.as_ptr(), &mut luid) != 0 && {
                let privileges = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
                };
                // AdjustTokenPrivileges reports success even when it changed
                // nothing, so the real answer is in the last error.
                AdjustTokenPrivileges(token, 0, &privileges, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0 && GetLastError() == 0
            };
            CloseHandle(token);
            ok
        }
    }

    /// Issue one SystemMemoryListInformation command.
    pub fn set_memory_list(command: i32) -> bool {
        // SAFETY: the information buffer is one i32 that outlives the call.
        unsafe { NtSetSystemInformation(SYSTEM_MEMORY_LIST_INFORMATION, (&raw const command).cast(), 4) == 0 }
    }
}
