// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

// No console window behind the program in a release build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gui;

use std::process::ExitCode;

use eframe::egui;
use roch_stresstest::{APP_NAME, VERSION, selftest, tools};

const USAGE: &str = "usage: RochStressTest [--no-elevate] [--tab NAME] [--selftest]

  --no-elevate   Windows: do not ask for administrator rights at launch.
  --tab NAME     Open on a tab: Quick Start, Log, or a tool's name (y-cruncher, ...).
  --selftest     Build every preset and Quick Start button without running anything,
                 print the report, and exit non-zero if any failed.";

/// Windows: whether this process has administrator rights.
fn is_admin() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: IsUserAnAdmin takes no arguments and only reads the token.
        unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Windows: start this program again with administrator rights. TestMem5 and
/// RAM Test Pro need them to lock physical pages and Prime95 to set affinity;
/// asking once at launch beats a tool failing three hours into a run. Returns
/// true when the elevated copy started and this one should exit; when the
/// prompt is declined the window opens anyway and says what that costs.
#[cfg(windows)]
fn relaunch_elevated() -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let Ok(exe) = std::env::current_exe() else { return false };
    let wide = |text: &std::ffi::OsStr| text.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let verb = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    // The elevated copy must not ask again if elevation silently failed.
    let params = wide("--no-elevate".as_ref());
    let directory = std::env::current_dir().map(|dir| wide(dir.as_os_str())).unwrap_or_else(|_| vec![0]);
    // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that outlives the
    // call; a null window handle is allowed.
    let result =
        unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), directory.as_ptr(), SW_SHOWNORMAL) };
    // Values above 32 mean the elevated process was started.
    result as isize > 32
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut no_elevate = false;
    let mut tab = String::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--no-elevate" => no_elevate = true,
            "--tab" => tab = iter.next().cloned().unwrap_or_default(),
            "--selftest" => {
                #[cfg(windows)]
                // SAFETY: attaching to the parent's console only changes where
                // this process's standard handles point.
                unsafe {
                    windows_sys::Win32::System::Console::AttachConsole(u32::MAX);
                }
                let (lines, failures) = selftest::run(&tools::tools_root());
                for line in lines {
                    println!("{line}");
                }
                return if failures == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE };
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unrecognized argument: {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    #[cfg(windows)]
    if !no_elevate && !is_admin() && relaunch_elevated() {
        return ExitCode::SUCCESS;
    }
    let _ = no_elevate;
    let note = (!is_admin()).then_some("Running without administrator rights: TestMem5 and RAM Test Pro will not be able to lock memory.");

    let title = format!("{APP_NAME} {VERSION}");
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(&title)
        // The Wayland app id, which is how the launcher entry finds the window's icon.
        .with_app_id("RochStressTest")
        .with_inner_size([860.0, 780.0])
        .with_min_inner_size([700.0, 560.0]);
    if let Ok(icon) = image::load_from_memory(include_bytes!("../assets/icon.png")) {
        let icon = icon.to_rgba8();
        viewport = viewport.with_icon(egui::IconData { width: icon.width(), height: icon.height(), rgba: icon.into_raw() });
    }
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    match eframe::run_native(APP_NAME, options, Box::new(move |cc| Ok(Box::new(gui::App::new(&cc.egui_ctx, note, &tab))))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{APP_NAME} could not start: {error}");
            ExitCode::FAILURE
        }
    }
}
