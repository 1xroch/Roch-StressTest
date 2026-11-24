// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the machine is, in the few numbers a stress test needs to size itself.
//!
//! Reading them wrong is worse than not reading them -- a Linpack size chosen
//! from an over-reported free-memory figure swaps rather than stresses -- so
//! each one falls back to a conservative answer rather than a guess. Windows
//! reads kernel32 and the registry; Linux reads /proc and /sys.

/// Installed memory in MB, or 8192 when it cannot be read.
pub fn total_ram_mb() -> u64 {
    imp::memory().map(|(total, _)| total).unwrap_or(8192)
}

/// Memory free right now in MB, or half of installed when unreadable.
///
/// On Linux this is MemAvailable: free memory plus the page cache the kernel
/// would give up for an allocation, which is what a memory test can get.
pub fn available_ram_mb() -> u64 {
    imp::memory().map(|(_, available)| available).unwrap_or_else(|| total_ram_mb() / 2)
}

/// Logical processors, which is what a worker count is counted in.
pub fn logical_cores() -> usize {
    std::thread::available_parallelism().map(|count| count.get()).unwrap_or(1)
}

/// Physical cores, or half the logical count when they cannot be counted --
/// wrong only in the direction of asking for fewer workers.
pub fn physical_cores() -> usize {
    imp::physical_cores().filter(|cores| *cores > 0).unwrap_or_else(|| (logical_cores() / 2).max(1))
}

/// "GenuineIntel", "AuthenticAMD", or "" when unreadable.
pub fn cpu_vendor() -> String {
    imp::cpu_string("vendor").unwrap_or_default()
}

/// The marketing name, for the run header in the log.
pub fn cpu_name() -> String {
    imp::cpu_string("name").filter(|name| !name.is_empty()).unwrap_or_else(|| "Unknown CPU".into())
}

pub fn is_amd() -> bool {
    cpu_vendor().eq_ignore_ascii_case("AuthenticAMD")
}

/// One line naming the machine, written at the top of every run log.
pub fn describe() -> String {
    format!("{} -- {:.1} GB RAM", describe_cpu(), total_ram_mb() as f64 / 1024.0)
}

/// The same line without the memory size, for the toolbar, which shows memory
/// live a few pixels to the right.
pub fn describe_cpu() -> String {
    format!("{} -- {}C/{}T", cpu_name(), physical_cores(), logical_cores())
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::HashSet;

    fn meminfo_mb(text: &str, key: &str) -> Option<u64> {
        text.lines().find_map(|line| {
            let (name, rest) = line.split_once(':')?;
            (name == key).then(|| rest.split_whitespace().next()?.parse::<u64>().ok().map(|kb| kb / 1024))?
        })
    }

    pub fn memory() -> Option<(u64, u64)> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let total = meminfo_mb(&text, "MemTotal")?;
        let available = meminfo_mb(&text, "MemAvailable").unwrap_or(total / 2);
        Some((total, available))
    }

    /// One per (package, core) pair, so hybrid parts with no SMT on the E-cores
    /// count right where half the logical count would not.
    pub fn physical_cores() -> Option<usize> {
        let mut cores = HashSet::new();
        for entry in std::fs::read_dir("/sys/devices/system/cpu").ok()?.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("cpu") || !name[3..].chars().all(|c| c.is_ascii_digit()) || name.len() == 3 {
                continue;
            }
            let topology = entry.path().join("topology");
            let read = |file: &str| std::fs::read_to_string(topology.join(file)).ok().map(|text| text.trim().to_string());
            if let (Some(package), Some(core)) = (read("physical_package_id"), read("core_id")) {
                cores.insert((package, core));
            }
        }
        Some(cores.len())
    }

    pub fn cpu_string(what: &str) -> Option<String> {
        let key = if what == "vendor" { "vendor_id" } else { "model name" };
        let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        text.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().to_string())
        })
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::SystemInformation::{
        GetLogicalProcessorInformation, GlobalMemoryStatusEx, MEMORYSTATUSEX, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION,
    };

    pub fn memory() -> Option<(u64, u64)> {
        // SAFETY: MEMORYSTATUSEX is plain data; dwLength is set as the call requires.
        unsafe {
            let mut status: MEMORYSTATUSEX = std::mem::zeroed();
            status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            (GlobalMemoryStatusEx(&mut status) != 0).then_some((status.ullTotalPhys / (1024 * 1024), status.ullAvailPhys / (1024 * 1024)))
        }
    }

    /// Counted from the processor-relationship table. It describes the first
    /// processor group only, so past 64 logical processors this undercounts;
    /// the number only seeds a default.
    pub fn physical_cores() -> Option<usize> {
        // SAFETY: the first call only reports the size needed; the buffer
        // passed to the second is that many bytes of plain data.
        unsafe {
            let mut length = 0u32;
            GetLogicalProcessorInformation(std::ptr::null_mut(), &mut length);
            let size = std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
            if length == 0 {
                return None;
            }
            let mut buffer: Vec<SYSTEM_LOGICAL_PROCESSOR_INFORMATION> = vec![std::mem::zeroed(); length as usize / size + 1];
            if GetLogicalProcessorInformation(buffer.as_mut_ptr(), &mut length) == 0 {
                return None;
            }
            let count = length as usize / size;
            Some(buffer[..count].iter().filter(|entry| entry.Relationship == RelationProcessorCore).count())
        }
    }

    pub fn cpu_string(what: &str) -> Option<String> {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0").ok()?;
        let value: String = key.get_value(if what == "vendor" { "VendorIdentifier" } else { "ProcessorNameString" }).ok()?;
        Some(value.trim().to_string())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn memory() -> Option<(u64, u64)> {
        None
    }
    pub fn physical_cores() -> Option<usize> {
        None
    }
    pub fn cpu_string(_what: &str) -> Option<String> {
        None
    }
}
