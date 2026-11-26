# Roch StressTest

One window for every stress test on your PC. It starts each test with sensible defaults, holds it to a time limit, and stops the run at the first error.

## Install

### Windows

1. Download `RochStressTest-<version>.zip` from [Releases](https://github.com/1xroch/Roch-StressTest/releases).
2. Unzip it to a folder you own.
3. Run `RochStressTest.exe` and allow administrator access.

Windows 10/11 x64. One executable, nothing to install. Cinebench and OCCT are too large to include. Get them from [Maxon](https://www.maxon.net/en/downloads/cinebench-downloads) and [OCBase](https://www.ocbase.com/download), and put them next to the app.

### Linux

1. Download `RochStressTest-<version>-linux-x86_64.tar.gz` from [Releases](https://github.com/1xroch/Roch-StressTest/releases).
2. Unpack it and run `./install.sh`. That puts the app and its tools in `~/.local/opt/RochStressTest`, adds a `rochstresstest` command, and adds Roch StressTest to your launcher. No root needed.

x86-64, glibc 2.35 or newer (Ubuntu 22.04, Fedora 36, current Arch). On Linux it runs **Prime95** (as `mprime`), **y-cruncher**, **memtest_vulkan** and **OCCT**; the rest are Windows-only here. Console tools open in your terminal emulator (foot, Alacritty, kitty, Ghostty, WezTerm, Konsole, GNOME Terminal, xfce4-terminal or xterm; set `ROCH_TERMINAL` to choose). OCCT is not included: download OCCT for Linux from [OCBase](https://www.ocbase.com/download) and put it in an `OCCT` folder beside the app.

## What it does

- **Quick Start:** one click runs any tool's defaults.
- **y-cruncher and Linpack Extended:** build and save your own profiles.
- **TestMem5, RAM Test Pro, Prime95, OCCT, Cinebench, memtest_vulkan, Linpack Xtreme, 3DMark 11:** launched and watched for errors.
- **Memory cleaner** (Windows), a **Stop** button for any run, and light/dark mode.

## Build

```console
cargo build --release
```

Put `RochStressTest` (`.exe` on Windows) next to the tool folders. On Linux, `packaging/linux/fetch-tools.sh` downloads the Linux tool builds beside it. `RochStressTest --selftest` builds every preset and Quick Start button without running anything. [docs/reference.md](docs/reference.md) has the details.

## Credits

- **George Woltman / GIMPS — [Prime95](https://www.mersenne.org/download/)**
- **Alexander J. Yee — [y-cruncher](http://www.numberworld.org/y-cruncher/)**
- **Serj and CoolCmd — [TestMem5](https://github.com/CoolCmd/TestMem5)**, with the 1usmus and anta777 profiles
- **PCStonks — RAM Test Pro**
- **BoringBoredom — [Linpack Extended](https://github.com/BoringBoredom/Linpack-Extended)**
- **Regeneration — [Linpack Xtreme](https://www.ngohq.com/linpack-xtreme.html)**
- **GpuZelenograd — [memtest_vulkan](https://github.com/GpuZelenograd/memtest_vulkan)**
- **OCBase — [OCCT](https://www.ocbase.com/)**, **Maxon — [Cinebench](https://www.maxon.net/en/cinebench)**, **UL — 3DMark 11**
- **[egui](https://github.com/emilk/egui)** for the window

Created by **Roch Studio / [@1xroch](https://x.com/1xroch)**. Licensed GPL-3.0; bundled tools keep their own licenses.

[YouTube](https://www.youtube.com/@MateoPcTech) | [X](https://x.com/1xroch) | [Discord](https://discord.gg/KfzExpKQHB)
