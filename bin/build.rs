// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::{env, fs};

fn main() {
    let app_id = if env::var("PROFILE").unwrap() == "debug" {
        "io.github.hrniels.Eventix-debug"
    } else {
        "io.github.hrniels.Eventix"
    };
    let icons = ["month", "week", "list", "event", "todo"];

    let icons_path = Path::new("../../data").join("icons");
    let icons_path = fs::canonicalize(&icons_path).expect("Failed to canonicalize icons path");

    let out_dir = env::var("OUT_DIR").unwrap();
    let path = Path::new(&out_dir).join("icons.rs");
    let mut f = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();

    writeln!(f, "pub const APP_ID: &str = \"{app_id}\";\n").unwrap();

    for icon in icons {
        writeln!(
            f,
            "pub const ICON_{}: &[u8] = include_bytes!(\"{}/{}.png\");",
            icon.to_string().to_uppercase(),
            icons_path.to_str().unwrap(),
            icon
        )
        .unwrap();

        println!(
            "cargo:rerun-if-changed={}",
            icons_path.join(format!("{icon}.png")).display()
        );
    }
}
