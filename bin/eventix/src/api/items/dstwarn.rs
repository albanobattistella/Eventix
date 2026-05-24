// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use eventix_ical::objects::CalendarTimeZoneResolver;
use eventix_state::EventixState;
use formatx::formatx;
use serde::{Deserialize, Serialize};

use crate::api::JsonError;

#[derive(Debug, Deserialize)]
pub struct Request {
    from_date: Option<String>,
    from_time: Option<String>,
    to_date: Option<String>,
    to_time: Option<String>,
    timezone: String,
}

#[derive(Debug, Serialize)]
struct Response {
    warning: Option<String>,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/dstwarn", get(handler))
        .with_state(state)
}

fn parse_date(s: &str) -> anyhow::Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| anyhow!("Invalid date '{}': {}", s, e))
}

fn parse_time(s: &str) -> anyhow::Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").map_err(|e| anyhow!("Invalid time '{}': {}", s, e))
}

fn parse_datetime(date: Option<&str>, time: Option<&str>) -> anyhow::Result<Option<NaiveDateTime>> {
    match (date, time) {
        (Some(date), Some(time)) if !date.is_empty() && !time.is_empty() => Ok(Some(
            NaiveDateTime::new(parse_date(date)?, parse_time(time)?),
        )),
        _ => Ok(None),
    }
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    // do not return an error here as this might happen if the user is not yet finished entering
    // the start/end of the event, for example.
    let Ok(start) = parse_datetime(req.from_date.as_deref(), req.from_time.as_deref()) else {
        return Ok(Json(Response { warning: None }));
    };
    let Ok(end) = parse_datetime(req.to_date.as_deref(), req.to_time.as_deref()) else {
        return Ok(Json(Response { warning: None }));
    };

    let Some((start, end)) = start.zip(end) else {
        return Ok(Json(Response { warning: None }));
    };

    let locale = state.lock().await.locale();
    let resolver = CalendarTimeZoneResolver::default();
    let hit = resolver.first_dst_transition_in_local_range(&req.timezone, start, end);

    Ok(Json(Response {
        warning: hit.map(|hit| {
            let local = hit.local().format("%Y-%m-%d %H:%M").to_string();
            formatx!(locale.translate("warning.dst_range"), local).unwrap()
        }),
    }))
}
