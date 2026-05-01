// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{anyhow, Result};
use clap::Parser;
use dbus_secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;

/// Simple tool to retrieve passwords from the secret service.
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// The attribute key to search for (e.g. "user")
    key: String,
    /// The attribute value to search for (e.g. the email address)
    value: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let ss = SecretService::connect(EncryptionType::Plain)?;
    let collection = ss.get_default_collection()?;

    let mut search = HashMap::new();
    search.insert(args.key.as_str(), args.value.as_str());

    let items = collection.search_items(search)?;

    if let Some(item) = items.first() {
        let secret = item.get_secret()?;
        let password = String::from_utf8(secret)?;
        print!("{}", password);
        Ok(())
    } else {
        Err(anyhow!("No password found for {}={}", args.key, args.value))
    }
}
