// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use eventix_locale::Locale;
use eventix_state::PasswordSource;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::sync::Arc;

use crate::html::filters;
use crate::pages::Page;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PasswordMode {
    SecretService,
    Command,
}

impl Display for PasswordMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecretService => write!(f, "SECRETSERVICE"),
            Self::Command => write!(f, "COMMAND"),
        }
    }
}

impl PasswordMode {
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        match buf.as_str() {
            "SECRETSERVICE" => Ok(Some(Self::SecretService)),
            "COMMAND" => Ok(Some(Self::Command)),
            _ => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PasswordSourceRequest {
    #[serde(default, deserialize_with = "PasswordMode::deserialize")]
    pub mode: Option<PasswordMode>,
    pub attr_key: String,
    pub attr_value: String,
    pub cmd: String,
}

impl Default for PasswordSourceRequest {
    fn default() -> Self {
        Self {
            mode: Some(PasswordMode::SecretService),
            attr_key: String::new(),
            attr_value: String::new(),
            cmd: String::new(),
        }
    }
}

impl PasswordSourceRequest {
    pub fn from_source(source: Option<&PasswordSource>) -> Self {
        match source {
            Some(PasswordSource::SecretService { attributes }) => {
                let (attr_key, attr_value) = attributes
                    .iter()
                    .next()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .unwrap_or_default();
                Self {
                    mode: Some(PasswordMode::SecretService),
                    attr_key,
                    attr_value,
                    cmd: String::new(),
                }
            }
            Some(PasswordSource::Command { command }) => Self {
                mode: Some(PasswordMode::Command),
                attr_key: String::new(),
                attr_value: String::new(),
                cmd: command.join(" "),
            },
            None => Self::default(),
        }
    }

    pub fn to_source(&self) -> Option<PasswordSource> {
        match self.mode? {
            PasswordMode::SecretService => {
                if self.attr_key.is_empty() || self.attr_value.is_empty() {
                    None
                } else {
                    let mut attributes = BTreeMap::new();
                    attributes.insert(self.attr_key.clone(), self.attr_value.clone());
                    Some(PasswordSource::SecretService { attributes })
                }
            }
            PasswordMode::Command => {
                let cmd = self.cmd.trim();
                if cmd.is_empty() {
                    None
                } else {
                    Some(PasswordSource::Command {
                        command: cmd.split_whitespace().map(String::from).collect(),
                    })
                }
            }
        }
    }

    pub fn check(&self, locale: &Arc<dyn Locale + Send + Sync>, page: &mut Page) -> bool {
        match self.mode {
            Some(PasswordMode::SecretService)
                if self.attr_key.is_empty() || self.attr_value.is_empty() =>
            {
                page.add_error(
                    locale
                        .translate("error.collection_password_lookup")
                        .to_string(),
                );
                false
            }
            Some(PasswordMode::Command) if self.cmd.trim().is_empty() => {
                page.add_error(
                    locale
                        .translate("error.collection_password_command")
                        .to_string(),
                );
                false
            }
            _ => true,
        }
    }
}

#[derive(Template)]
#[template(path = "comps/pwsource.htm")]
pub struct PasswordSourceTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    name: String,
    id: String,
    value: PasswordSourceRequest,
}

impl PasswordSourceTemplate {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: String,
        value: PasswordSourceRequest,
    ) -> Self {
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            value,
            locale,
        }
    }

    pub fn mode(&self) -> String {
        match self.value.mode {
            Some(m) => format!("{m}"),
            None => String::from("NONE"),
        }
    }
}
