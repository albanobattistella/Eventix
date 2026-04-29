// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono::{Duration, Local, NaiveDate};
use eventix_ical::objects::{CalCompType, EventLike};
use eventix_locale::Locale;
use eventix_state::State;
use std::sync::Arc;

use crate::comps::occurrence::{OccurrenceMode, OccurrenceTemplate};
use crate::objects::DayOccurrence;

pub struct Day {
    pub date: Option<NaiveDate>,
    pub occurrences: Vec<String>,
}

pub struct Events {
    pub days: Vec<Day>,
}

impl Events {
    pub fn new(state: &State, locale: &Arc<dyn Locale + Send + Sync>) -> Events {
        Self::new_with_days(state, locale, 7)
    }

    pub fn new_with_days(
        state: &State,
        locale: &Arc<dyn Locale + Send + Sync>,
        days: u32,
    ) -> Events {
        let timezone = locale.timezone();

        let now = Local::now();
        let start = now.with_timezone(locale.timezone());
        let end = start + Duration::days(days as i64);

        let settings = state.settings();
        let pers_alarms = state.personal_alarms();

        let next_ev_occs = state
            .store()
            .directories()
            .iter()
            .filter(|s| !state.misc().calendar_disabled(s.id()))
            .flat_map(move |s| {
                s.occurrences_between(start, end, |c| c.ctype() == CalCompType::Event)
            })
            .filter(|o| !o.is_excluded())
            .collect::<Vec<_>>();

        let mut days = Vec::new();
        let mut cur_date = start.date_naive();
        let end_date = end.date_naive();
        while cur_date < end_date {
            let day_occs = DayOccurrence::occurrences_on(
                &next_ev_occs,
                settings,
                pers_alarms,
                cur_date,
                timezone,
            );
            if !day_occs.is_empty() {
                let occurrences = day_occs
                    .iter()
                    .map(|occ| {
                        OccurrenceTemplate::new(
                            locale.clone(),
                            occ,
                            OccurrenceMode::Sidebar,
                            cur_date,
                            &start,
                        )
                        .render()
                        .expect("rendering occurrence template failed")
                    })
                    .collect();
                days.push(Day {
                    date: Some(cur_date),
                    occurrences,
                });
            }

            cur_date += Duration::days(1);
        }

        Self { days }
    }
}
