// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for [`Occurrence`] properties when obtained via [`CalFile`].
//!
//! These tests parse real `.ics` fixture files and exercise `Occurrence` getters that are
//! most naturally reached through a full parse-and-iterate round-trip.

use chrono::TimeZone;
use chrono_tz::UTC;

use eventix_ical::col::{CalFile, Occurrence};
use eventix_ical::objects::{CalEventStatus, CalTodoStatus};

mod common;
use common::{data_dir, make_id};

fn utc(year: i32, month: u32, day: u32, h: u32, m: u32, s: u32) -> chrono::DateTime<chrono_tz::Tz> {
    UTC.with_ymd_and_hms(year, month, day, h, m, s).unwrap()
}

fn first_occurrence(file: &CalFile) -> Occurrence<'_> {
    file.occurrences_between(
        utc(2025, 1, 1, 0, 0, 0),
        utc(2025, 12, 31, 23, 59, 59),
        |_| true,
    )
    .next()
    .expect("expected at least one occurrence")
}

/// Parses `todo_with_status.ics` and checks that all TODO-specific properties are accessible
/// through the `Occurrence` API and that `is_cancelled` returns `true`.
#[test]
fn todo_cancelled_and_properties_via_occurrence() {
    let path = data_dir().join("todo_with_status.ics");
    let file = CalFile::new_from_file(make_id("cal"), path).unwrap();

    let occ = first_occurrence(&file);

    assert_eq!(occ.todo_status(), Some(CalTodoStatus::Cancelled));
    assert_eq!(occ.todo_percent(), Some(50));
    assert!(occ.todo_completed().is_some(), "expected a COMPLETED date");
    assert!(occ.is_cancelled(), "expected occurrence to be cancelled");
}

/// Parses `event_cancelled.ics` and checks that `event_status` returns `Cancelled` and
/// `is_cancelled` returns `true` via the `Occurrence` API.
#[test]
fn event_cancelled_via_occurrence() {
    let path = data_dir().join("event_cancelled.ics");
    let file = CalFile::new_from_file(make_id("cal"), path).unwrap();

    let occ = first_occurrence(&file);

    assert_eq!(occ.event_status(), Some(CalEventStatus::Cancelled));
    assert!(occ.is_cancelled(), "expected occurrence to be cancelled");
}

#[test]
fn recurring_gap_instance_is_skipped_during_occurrence_expansion() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("dst-gap-recur.ics");
    std::fs::write(
        &path,
        concat!(
            "BEGIN:VCALENDAR\n",
            "VERSION:2.0\n",
            "BEGIN:VEVENT\n",
            "UID:dst-gap-recur\n",
            "DTSTAMP:20250101T000000Z\n",
            "DTSTART;TZID=Europe/Berlin:20260328T023000\n",
            "RRULE:FREQ=DAILY;COUNT=3\n",
            "END:VEVENT\n",
            "END:VCALENDAR\n"
        ),
    )
    .unwrap();

    let file = CalFile::new_from_file(make_id("cal"), path).unwrap();
    let occurrences = file
        .occurrences_between(utc(2026, 3, 27, 0, 0, 0), utc(2026, 4, 2, 0, 0, 0), |_| {
            true
        })
        .collect::<Vec<_>>();

    assert_eq!(occurrences.len(), 2);
    assert_eq!(
        occurrences[0]
            .resolved_occurrence_start()
            .unwrap()
            .with_timezone(&UTC)
            .to_rfc3339(),
        "2026-03-28T01:30:00+00:00"
    );
    assert_eq!(
        occurrences[1]
            .resolved_occurrence_start()
            .unwrap()
            .with_timezone(&UTC)
            .to_rfc3339(),
        "2026-03-30T00:30:00+00:00"
    );
}

#[test]
fn recurring_fold_instance_uses_first_occurrence_during_expansion() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("dst-fold-recur.ics");
    std::fs::write(
        &path,
        concat!(
            "BEGIN:VCALENDAR\n",
            "VERSION:2.0\n",
            "BEGIN:VEVENT\n",
            "UID:dst-fold-recur\n",
            "DTSTAMP:20250101T000000Z\n",
            "DTSTART;TZID=Europe/Berlin:20251025T023000\n",
            "RRULE:FREQ=DAILY;COUNT=2\n",
            "END:VEVENT\n",
            "END:VCALENDAR\n"
        ),
    )
    .unwrap();

    let file = CalFile::new_from_file(make_id("cal"), path).unwrap();
    let occurrences = file
        .occurrences_between(
            utc(2025, 10, 24, 0, 0, 0),
            utc(2025, 10, 28, 0, 0, 0),
            |_| true,
        )
        .collect::<Vec<_>>();

    assert_eq!(occurrences.len(), 2);
    assert_eq!(
        occurrences[0]
            .resolved_occurrence_start()
            .unwrap()
            .with_timezone(&UTC)
            .to_rfc3339(),
        "2025-10-25T00:30:00+00:00"
    );
    assert_eq!(
        occurrences[1]
            .resolved_occurrence_start()
            .unwrap()
            .to_rfc3339(),
        "2025-10-26T02:30:00+02:00"
    );
}
