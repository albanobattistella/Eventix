// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use eventix_ical::{
    objects::{CalCompType, EventLike},
    util,
};
use eventix_locale::Locale;
use eventix_state::EventixState;
use serde::Deserialize;
use std::sync::Arc;

use crate::comps::occurrence::{OccurrenceMode, OccurrenceTemplate};
use crate::html::filters;
use crate::objects::DayOccurrence;
use crate::pages::Page;
use crate::pages::error::HTMLError;
use crate::util::parse_human_date;

struct Day {
    date: Option<NaiveDate>,
    show_month: bool,
    cur_month: bool,
    occurrences: Vec<String>,
    occ_ids: Vec<String>,
}

#[derive(Default, Debug, Deserialize)]
pub struct Request {
    date: Option<String>,
}

/// Fragment-only template for the calendar grid, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/monthly.htm")]
struct MonthlyTemplate {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    weekdays: Vec<String>,
    days: Vec<Day>,
    today: NaiveDate,
    month: String,
    prev_month: String,
    next_month: String,
}

/// Renders only the calendar grid fragment for the given month. Used by the AJAX content endpoint.
pub async fn content(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(&state).await;
    let locale = state.lock().await.locale();
    let timezone = *locale.timezone();
    let now = Local::now().with_timezone(&timezone);

    let weekdays = vec![
        locale.translate("Monday").to_string(),
        locale.translate("Tuesday").to_string(),
        locale.translate("Wednesday").to_string(),
        locale.translate("Thursday").to_string(),
        locale.translate("Friday").to_string(),
        locale.translate("Saturday").to_string(),
        locale.translate("Sunday").to_string(),
    ];

    let date = parse_human_date(req.date, &timezone)?;
    let (pyear, pmonth) = util::prev_month(date.year(), date.month());
    let (nyear, nmonth) = util::next_month(date.year(), date.month());

    let num_days = util::month_days(date.year(), date.month());
    let month_start = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap();
    let month_end = month_start + Duration::days(num_days as i64);
    let start_off = month_start.weekday().num_days_from_monday();
    let end_off = 7 - month_end.weekday().num_days_from_monday();

    let mut date = month_start - Duration::days(start_off as i64);
    let end = month_start + Duration::days((num_days + end_off) as i64);
    let mstart = timezone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .unwrap();
    let mend = timezone
        .from_local_datetime(&end.pred_opt().unwrap().and_hms_opt(23, 59, 59).unwrap())
        .unwrap();

    let state = state.lock().await;
    let store = state.store();

    let ev_occs = store
        .directories()
        .iter()
        .filter(|s| !state.misc().calendar_disabled(s.id()))
        .flat_map(move |s| s.occurrences_between(mstart, mend, |c| c.ctype() == CalCompType::Event))
        .filter(|o| !o.is_excluded())
        .collect::<Vec<_>>();

    let settings = state.settings();
    let pers_alarms = state.personal_alarms();

    let mut days = Vec::new();
    while date < end {
        let day_occs =
            DayOccurrence::occurrences_on(&ev_occs, settings, pers_alarms, date, &timezone);
        let mut occurrences = Vec::new();
        let mut occ_ids = Vec::new();
        for occ in day_occs {
            occ_ids.push(occ.id().to_string());
            occurrences.push(
                OccurrenceTemplate::new(locale.clone(), &occ, OccurrenceMode::Block, date, &now)
                    .render()
                    .context("occurrence template")?,
            );
        }
        days.push(Day {
            date: Some(date),
            show_month: date.day() == 1
                || date.day() == util::month_days(date.year(), date.month()),
            cur_month: date >= month_start && date < month_end,
            occurrences,
            occ_ids,
        });

        date += Duration::days(1);
    }

    let html = MonthlyTemplate {
        page,
        weekdays,
        month: format!(
            "{} {}",
            locale.translate(&month_start.format("%B").to_string()),
            month_start.format("%Y")
        ),
        prev_month: format!("{pyear}-{pmonth}"),
        next_month: format!("{nyear}-{nmonth}"),
        today: now.date_naive(),
        days,
        locale,
    }
    .render()
    .context("monthly content template")?;

    Ok(Html(html))
}
