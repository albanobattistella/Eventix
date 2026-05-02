// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use toml_edit::{DocumentMut, value};

type MigrationFn = fn(&mut DocumentMut);

struct Migration {
    version: u32,
    filename: &'static str,
    migrate: MigrationFn,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 0,
    filename: "settings.toml",
    migrate: migrate_settings_v0_to_v1,
}];

pub fn migrate_if_needed(path: &PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    // ignore previously created backup files (this is okay, because although alarms are stored as
    // <cal-id>.toml, the calendar id is a generated uuid).
    if Regex::new(r"\.v\d+\.toml$").unwrap().is_match(filename) {
        return Ok(());
    }

    let content = fs::read_to_string(path).context("reading toml for migration")?;
    let mut doc = content
        .parse::<DocumentMut>()
        .context("parsing toml for migration")?;

    let mut version = doc.get("version").and_then(|v| v.as_integer()).unwrap_or(0) as u32;

    if version >= crate::CURRENT_VERSION {
        return Ok(());
    }

    // Create backup of the original version
    let mut backup_path = path.clone();
    backup_path.set_extension(format!("v{}.toml", version));
    fs::copy(path, &backup_path).context("creating backup before migration")?;

    // Perform migrations in a loop until we reach CURRENT_VERSION
    while version < crate::CURRENT_VERSION {
        for m in MIGRATIONS {
            if m.version == version {
                let matches = match m.filename {
                    "alarms" => {
                        filename.ends_with(".toml")
                            && filename != "settings.toml"
                            && filename != "misc.toml"
                    }
                    f => f == filename,
                };

                if matches {
                    (m.migrate)(&mut doc);
                }
            }
        }

        // Increment version. Even if no specific migration was found (e.g. for files that
        // don't need changes in this version), we still advance to the next version.
        version += 1;
        doc.insert("version", value(version as i64));
    }

    // Write back
    fs::write(path, doc.to_string()).context("writing migrated toml")?;

    Ok(())
}

fn migrate_settings_v0_to_v1(doc: &mut DocumentMut) {
    if let Some(collections) = doc.get_mut("collection").and_then(|v| v.as_table_mut()) {
        for (_, col) in collections.iter_mut() {
            if let Some(syncer) = col.get_mut("syncer").and_then(|v| v.as_table_mut()) {
                for (_, ty) in syncer.iter_mut() {
                    if let Some(ty_table) = ty.as_table_mut()
                        && let Some(pw_cmd) = ty_table.remove("password_cmd")
                    {
                        let mut pw_source = toml_edit::Table::new();
                        pw_source.insert("type", value("Command"));
                        pw_source.insert("command", pw_cmd);
                        ty_table.insert("password_source", toml_edit::Item::Table(pw_source));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_migrate_v0_to_v1_settings() {
        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");

        let v0_content = r#"
[collection.work]
syncer.VDirSyncer.email.name = "Alice"
syncer.VDirSyncer.email.address = "alice@example.com"
syncer.VDirSyncer.url = "https://dav.example.com"
syncer.VDirSyncer.read_only = false
syncer.VDirSyncer.username = "alice"
syncer.VDirSyncer.password_cmd = ["pass", "show", "work"]

[collection.home]
syncer.FileSystem.path = "/data"
"#;
        fs::write(&settings_path, v0_content).unwrap();

        migrate_if_needed(&settings_path).unwrap();

        let migrated_content = fs::read_to_string(&settings_path).unwrap();
        assert!(migrated_content.contains("version = 1"));
        assert!(migrated_content.contains("type = \"Command\""));
        assert!(migrated_content.contains("command = [\"pass\", \"show\", \"work\"]"));
        assert!(!migrated_content.contains("password_cmd"));

        // Check backup
        let backup_path = dir.path().join("settings.v0.toml");
        assert!(backup_path.exists());
        assert_eq!(fs::read_to_string(backup_path).unwrap(), v0_content);
    }

    #[test]
    fn test_migrate_v1_no_op() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("misc.toml");

        let v1_content = r#"version = 1
locale_type = "English"
"#;
        fs::write(&path, v1_content).unwrap();

        migrate_if_needed(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, v1_content);

        // No backup should be created if no migration was performed
        let backup_path = dir.path().join("misc.v1.toml");
        assert!(!backup_path.exists());
    }
}
