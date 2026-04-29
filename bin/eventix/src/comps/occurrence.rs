// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono::{DateTime, NaiveDate};
use chrono_tz::Tz;
use eventix_ical::objects::{CalCompType, CalDate, CalPartStat, EventLike};
use eventix_locale::{Locale, TimeFlags};
use std::sync::Arc;

use crate::html::filters;
use crate::objects::DayOccurrence;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OccurrenceMode {
    /// Used in the monthly grid and weekly all-day area.
    Block,
    /// Used in the weekly timed area.
    Timed,
    /// Used in the sidebar next events/tasks.
    Sidebar,
}

#[derive(Template)]
#[template(path = "comps/occurrence.htm")]
pub struct OccurrenceTemplate<'a> {
    pub locale: Arc<dyn Locale + Send + Sync>,
    pub occ: &'a DayOccurrence<'a>,
    pub mode: OccurrenceMode,
    pub day_date: NaiveDate,
    now: &'a DateTime<Tz>,
}

impl<'a> OccurrenceTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        occ: &'a DayOccurrence<'a>,
        mode: OccurrenceMode,
        day_date: NaiveDate,
        now: &'a DateTime<Tz>,
    ) -> Self {
        Self {
            locale,
            occ,
            mode,
            day_date,
            now,
        }
    }

    pub fn now(&self) -> &DateTime<Tz> {
        self.now
    }
}
