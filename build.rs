// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stamps the icon and the version from Cargo.toml into RochStressTest.exe.
//!
//! Generated here rather than committed, so the file properties of a build can
//! never carry a version other than the one in Cargo.toml.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let mut numbers: Vec<u32> = version.split('.').map(|part| part.parse().unwrap_or(0)).collect();
    numbers.resize(4, 0);
    let commas = numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let dotted = numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(".");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest_dir.join("assets").join("icon.ico");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let rc = out.join("roch_stresstest.rc");
    let rc_text = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Roch Studio"
      VALUE "FileDescription", "Roch StressTest"
      VALUE "FileVersion", "{dotted}"
      VALUE "InternalName", "RochStressTest"
      VALUE "OriginalFilename", "RochStressTest.exe"
      VALUE "ProductName", "Roch StressTest"
      VALUE "ProductVersion", "{dotted}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    fs::write(&rc, rc_text).expect("write the resource script");

    // A missing resource compiler costs the icon and the file properties, not the build.
    if let Err(error) = embed_resource::compile_for(&rc, ["RochStressTest"], embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=Windows resources were not embedded: {error}");
    }
}
