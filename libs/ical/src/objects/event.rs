// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::BufRead;
use std::ops::{Deref, DerefMut};

use chrono::{Duration, Utc};
use chrono_tz::Tz;

use crate::objects::{
    CalDate, CalDateTime, CalEventStatus, CalendarTimeZoneResolver, DateContext, EventLike,
    ObjectsError, ResolvedDateTime,
};
use crate::parser::{
    LineReader, LineResultExt, ParseError, ParseErrorType, Property, PropertyConsumer,
    PropertyProducer,
};

use super::CalCompType;
use super::component::EventLikeComponent;

/// Represents an iCalendar event.
///
/// Each event has a unique id (uid) and several optional properties such as a summary, a
/// description, or alarms. An event shares many properties with
/// [`CalTodo`](crate::objects::CalTodo), which are implemented in [`EventLikeComponent`]. In
/// contrast to TODOs, events have a [`CalEventStatus`] and an end date instead of a due date.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.6.1>.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalEvent {
    pub(crate) inner: EventLikeComponent,
    status: Option<CalEventStatus>,
    end: Option<CalDate>,
}

/// Represents an edge in a range.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RangeEdge {
    /// The start of a range.
    Start,
    /// The end of a range.
    End,
}

impl CalEvent {
    fn new_empty() -> Self {
        Self {
            inner: EventLikeComponent::new_empty(CalCompType::Event),
            status: None,
            end: None,
        }
    }

    /// Creates a new event with given uid.
    pub fn new<T: ToString>(uid: T) -> Self {
        let mut new = Self::new_empty();
        new.inner = EventLikeComponent::new(uid, CalCompType::Event);
        new
    }

    /// Returns the status of the event.
    pub fn status(&self) -> Option<CalEventStatus> {
        self.status
    }

    /// Sets the status to given value.
    pub fn set_status(&mut self, status: Option<CalEventStatus>) {
        self.status = status;
    }

    /// Returns the end of the event.
    pub fn end(&self) -> Option<&CalDate> {
        self.end.as_ref()
    }

    /// Sets the event end to given value.
    pub fn set_end(&mut self, end: Option<CalDate>) {
        self.end = end;
    }

    /// Returns the resolved elapsed duration between start and end, if both are present.
    pub fn actual_duration(&self, ctx: &DateContext) -> Option<Duration> {
        Some(
            ctx.date(self.end()?).resolved_end(&Tz::UTC)
                - ctx.date(self.start()?).resolved_start(&Tz::UTC),
        )
    }

    /// Shifts the event to `new_start` while preserving its current span.
    ///
    /// Timed inputs are interpreted in `fallback_tz` unless they carry their own `TZID`. Gap times
    /// are snapped forward to the next representable instant. Fold times switch the whole range to
    /// UTC so the chosen instant stays exact.
    pub fn shift_to(
        &mut self,
        ctx: &DateContext,
        new_start: CalDate,
        fallback_tz: &Tz,
    ) -> Result<(), ObjectsError> {
        let duration = self
            .actual_duration(ctx)
            .ok_or(ObjectsError::MissingDuration)?;

        if matches!(new_start, CalDate::Date(..)) || self.is_all_day() {
            let ty = self.ctype().into();
            let start = new_start.as_naive_date();
            // add one second here as the duration for all-day events is one second less to stay on
            // the same day.
            let end = start + (duration + Duration::seconds(1));
            self.set_start(Some(CalDate::Date(start, ty)));
            self.set_end(Some(CalDate::Date(end, ty)));
            return Ok(());
        }

        let final_end_instant =
            new_start.as_start_with_resolver(fallback_tz, ctx.resolver()) + duration;

        self.apply_timed_range(
            ctx,
            Some(new_start),
            None,
            Some(final_end_instant),
            fallback_tz,
        );
        Ok(())
    }

    /// Resizes one edge of the event while keeping the other edge unchanged.
    ///
    /// Timed inputs are interpreted in `fallback_tz` unless they carry their own `TZID`. Gap times
    /// are snapped forward to the next representable instant. Fold times switch the whole range to
    /// UTC so the chosen instant stays exact.
    pub fn resize(
        &mut self,
        ctx: &DateContext,
        edge: RangeEdge,
        new_value: CalDate,
        fallback_tz: &Tz,
    ) -> Result<(), ObjectsError> {
        if self.is_all_day() || matches!(new_value, CalDate::Date(..)) {
            return Err(ObjectsError::ResizeAllDay);
        }

        match edge {
            RangeEdge::Start => {
                self.apply_timed_range(ctx, Some(new_value), None, None, fallback_tz)
            }
            RangeEdge::End => self.apply_timed_range(ctx, None, Some(new_value), None, fallback_tz),
        }

        Ok(())
    }

    fn apply_timed_range(
        &mut self,
        ctx: &DateContext,
        new_start: Option<CalDate>,
        new_end: Option<CalDate>,
        derived_end_instant: Option<ResolvedDateTime>,
        fallback_tz: &Tz,
    ) {
        let final_start = new_start.clone().or_else(|| self.start().cloned());
        let final_end = new_end.clone().or_else(|| self.end().cloned());

        // build start and end instances
        let resolver = ctx.resolver();
        let final_start_instant = final_start
            .as_ref()
            .map(|date| date.as_start_with_resolver(fallback_tz, resolver));
        let final_end_instant = derived_end_instant.or_else(|| {
            final_end
                .as_ref()
                .map(|date| date.as_end_with_resolver(fallback_tz, resolver))
        });

        // Determine whether we need to switch to UTC to specify the requested range. This happens
        // for example when the start is in a DST gap in the desired timezone. In such cases we
        // switch both start and end to UTC.
        let event_tzid = self
            .start()
            .and_then(Self::timed_tzid)
            .or_else(|| self.end().and_then(Self::timed_tzid));
        let force_utc = event_tzid.is_some_and(|tzid| {
            Self::timed_date_requires_utc(final_start.as_ref(), final_start_instant, tzid, resolver)
                || Self::timed_date_requires_utc(
                    final_end.as_ref(),
                    final_end_instant,
                    tzid,
                    resolver,
                )
        });

        if force_utc {
            if let Some(start) = final_start_instant {
                self.set_start(Some(CalDate::DateTime(CalDateTime::Utc(
                    start.with_timezone(&Utc),
                ))));
            }
            if let Some(end) = final_end_instant {
                self.set_end(Some(CalDate::DateTime(CalDateTime::Utc(
                    end.with_timezone(&Utc),
                ))));
            }
            return;
        }

        // build the new start and end in the same shapes as the original ones
        let start_shape = self.start().or(self.end()).cloned();
        let end_shape = self.end().or(self.start()).cloned();

        let update_start = new_start.is_some();
        let update_end = new_end.is_some() || derived_end_instant.is_some();

        if let Some(start) = final_start.filter(|_| update_start) {
            let start = match start {
                CalDate::Date(..) => start,
                CalDate::DateTime(_) => Self::rebuild_timed_date(
                    start_shape.as_ref().unwrap_or(&start),
                    final_start_instant.unwrap(),
                    fallback_tz,
                    resolver,
                ),
            };
            self.set_start(Some(start));
        }

        if let Some(end) = final_end.filter(|_| update_end) {
            let end = match end {
                CalDate::Date(..) => end,
                CalDate::DateTime(_) => Self::rebuild_timed_date(
                    end_shape.as_ref().unwrap_or(&end),
                    final_end_instant.unwrap(),
                    fallback_tz,
                    resolver,
                ),
            };
            self.set_end(Some(end));
        }
    }

    fn timed_tzid(date: &CalDate) -> Option<&str> {
        match date {
            CalDate::DateTime(CalDateTime::Timezone(_, tzid)) => Some(tzid.as_str()),
            _ => None,
        }
    }

    fn timed_date_requires_utc(
        date: Option<&CalDate>,
        instant: Option<ResolvedDateTime>,
        event_tzid: &str,
        resolver: &CalendarTimeZoneResolver,
    ) -> bool {
        let Some(date) = date else {
            return false;
        };
        let Some(instant) = instant else {
            return false;
        };

        if let CalDate::DateTime(CalDateTime::Timezone(local, tzid)) = date
            && tzid == event_tzid
            && resolver.is_fold_local_time(event_tzid, *local)
        {
            return true;
        }

        resolver.instant_is_fold_local_time(instant, event_tzid)
    }

    fn rebuild_timed_date(
        source_shape: &CalDate,
        instant: ResolvedDateTime,
        fallback_tz: &Tz,
        resolver: &CalendarTimeZoneResolver,
    ) -> CalDate {
        match source_shape {
            CalDate::DateTime(CalDateTime::Timezone(_, tzid)) => {
                CalDate::DateTime(CalDateTime::Timezone(
                    resolver.instant_to_local(instant, Some(tzid), fallback_tz),
                    tzid.clone(),
                ))
            }
            CalDate::DateTime(CalDateTime::Floating(_)) => CalDate::DateTime(
                CalDateTime::Floating(instant.with_timezone(fallback_tz).naive_local()),
            ),
            CalDate::DateTime(CalDateTime::Utc(_)) => {
                CalDate::DateTime(CalDateTime::Utc(instant.with_timezone(&Utc)))
            }
            CalDate::Date(..) => unreachable!("timed dates require a timed source shape"),
        }
    }
}

impl Deref for CalEvent {
    type Target = EventLikeComponent;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for CalEvent {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl PropertyProducer for CalEvent {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], "VEVENT")];
        if let Some(ref dtend) = self.end {
            props.push(dtend.to_prop("DTEND"));
        }
        if let Some(ref status) = self.status {
            props.push(Property::new("STATUS", vec![], status.to_string()));
        }
        props.extend(self.inner.to_props());
        props.push(Property::new("END", vec![], "VEVENT"));
        props
    }
}

impl PropertyConsumer for CalEvent {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        _prop: Property,
    ) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let mut comp = Self::new_empty();
        loop {
            let Some(line) = lines.next() else {
                break Err(
                    ParseError::from(ParseErrorType::UnexpectedEOF).with_line(lines.line_num())
                );
            };

            let prop = Property::from_str_at(&line, lines.line_num())?;
            match prop.name().as_str() {
                "END" if prop.value() == "VEVENT" => {
                    break Ok(comp);
                }
                "STATUS" => {
                    comp.status = Some(prop.value().parse().with_line(lines)?);
                }
                "DTEND" => {
                    comp.end = Some(prop.try_into().with_line(lines)?);
                }
                _ => {
                    comp.inner.parse_prop(lines, prop)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, MappedLocalTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
    use chrono_tz::Tz;

    use super::*;
    use crate::objects::{
        CalComponent, CalDate, CalDateTime, CalRRule, Calendar, CalendarTimeZoneResolver,
        EventLike, ResolvedDateTime, UpdatableEventLike,
    };
    use crate::parser::{LineReader, Property};

    fn timed_event(start: CalDate, end: CalDate) -> CalEvent {
        let mut ev = CalEvent::new("uid");
        ev.set_start(Some(start));
        ev.set_end(Some(end));
        ev
    }

    fn ctx() -> DateContext {
        Calendar::default().date_context()
    }

    fn berlin_date(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> CalDate {
        CalDate::DateTime(CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(hh, mm, 0)
                .unwrap(),
            "Europe/Berlin".to_string(),
        ))
    }

    fn new_york_date(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> CalDate {
        CalDate::DateTime(CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(hh, mm, 0)
                .unwrap(),
            "America/New_York".to_string(),
        ))
    }

    fn utc_date(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> CalDate {
        CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(y, m, d, hh, mm, 0).unwrap(),
        ))
    }

    fn local_date(tz: &Tz, dt: NaiveDateTime) -> CalDate {
        CalDate::DateTime(CalDateTime::Timezone(dt, tz.name().to_string()))
    }

    enum DSTEvent {
        Gap,
        Fold,
        None,
    }

    fn assert_tz_event(naive: NaiveDateTime, tz: &Tz, ev: DSTEvent) {
        let is_gap = matches!(tz.from_local_datetime(&naive), MappedLocalTime::None);
        let is_fold = matches!(
            tz.from_local_datetime(&naive),
            MappedLocalTime::Ambiguous(_, _)
        );
        match ev {
            DSTEvent::Gap => {
                assert!(is_gap, "expected {naive} to be in a DST gap in {tz}");
                assert!(!is_fold, "expected {naive} to NOT be in a DST fold in {tz}");
            }
            DSTEvent::Fold => {
                assert!(!is_gap, "expected {naive} to NOT be in a DST gap in {tz}");
                assert!(is_fold, "expected {naive} to be in a DST fold in {tz}");
            }
            DSTEvent::None => {
                assert!(!is_gap, "expected {naive} to NOT be in a DST gap in {tz}");
                assert!(!is_fold, "expected {naive} to NOT be in a DST fold in {tz}");
            }
        }
    }

    #[test]
    fn parse_and_to_props_roundtrip() {
        let data = "UID:uid-1\n\
DTSTAMP:20250102T090000Z\n\
DTSTART:20250102T100000Z\n\
DTEND:20250102T120000Z\n\
STATUS:CONFIRMED\n\
SUMMARY:Meeting\n\
END:VEVENT\n";
        let mut lines = LineReader::new(data.as_bytes());
        let begin_prop = "BEGIN:VEVENT".parse::<Property>().unwrap();
        let ev = CalEvent::from_lines(&mut lines, begin_prop).expect("failed to parse VEVENT");

        // basics
        assert_eq!(ev.uid().as_str(), "uid-1");
        assert_eq!(ev.status(), Some(CalEventStatus::Confirmed));

        // end is a datetime in UTC and must match the exact textual representation when printed
        let end = ev.end().expect("end missing").to_string();
        assert_eq!(end, "TU2025-01-02T12:00:00");

        // start and summary were parsed into the inner component
        let start_prop = ev.start().expect("start missing").to_string();
        assert_eq!(start_prop, "TU2025-01-02T10:00:00");
        assert_eq!(ev.summary(), Some(&"Meeting".to_string()));

        // check produced properties are in the exact order expected by to_props
        let props: Vec<String> = ev.to_props().into_iter().map(|p| p.to_string()).collect();
        let expected = vec![
            "BEGIN:VEVENT".to_string(),
            "DTEND:20250102T120000Z".to_string(),
            "STATUS:CONFIRMED".to_string(),
            "UID:uid-1".to_string(),
            "DTSTAMP:20250102T090000Z".to_string(),
            "DTSTART:20250102T100000Z".to_string(),
            "SUMMARY:Meeting".to_string(),
            "END:VEVENT".to_string(),
        ];
        assert_eq!(props, expected);
    }

    #[test]
    fn status_and_end_setters() {
        let mut ev = CalEvent::new("my-uid");
        ev.set_status(Some(CalEventStatus::Tentative));
        assert_eq!(ev.status(), Some(CalEventStatus::Tentative));

        ev.set_status(None);
        assert_eq!(ev.status(), None);

        // set and clear end
        let dtend = "DTEND:20250103T010203Z"
            .parse::<Property>()
            .unwrap()
            .try_into()
            .unwrap();
        ev.set_end(Some(dtend));
        assert!(ev.end().is_some());
        ev.set_end(None);
        assert!(ev.end().is_none());
    }

    #[test]
    fn tzid_start_with_utc_until_stops_on_correct_occurrence() {
        let mut ev = CalEvent::new("good-until");
        ev.set_start(Some(CalDate::new_datetime(
            NaiveDate::from_ymd_opt(2025, 3, 28).unwrap(),
            NaiveTime::from_hms_opt(9, 0, 0),
            "Europe/Berlin",
        )));

        let mut rrule = CalRRule::default();
        rrule.set_frequency(crate::objects::CalRRuleFreq::Daily);
        rrule.set_until(CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(2025, 3, 31, 7, 0, 0).unwrap(),
        )));
        ev.set_rrule(Some(rrule));

        let resolver = CalendarTimeZoneResolver::default();
        let comp = CalComponent::Event(ev);
        let mut iter = comp.dates_between(
            Tz::Europe__Berlin
                .with_ymd_and_hms(2025, 3, 27, 0, 0, 0)
                .unwrap(),
            Tz::Europe__Berlin
                .with_ymd_and_hms(2025, 4, 2, 0, 0, 0)
                .unwrap(),
            &resolver,
        );

        assert_eq!(
            iter.next().unwrap().1.with_timezone(&Utc),
            ResolvedDateTime::from(
                Utc.with_ymd_and_hms(2025, 3, 28, 8, 0, 0)
                    .unwrap()
                    .fixed_offset()
            )
        );
        assert_eq!(
            iter.next().unwrap().1.with_timezone(&Utc),
            ResolvedDateTime::from(
                Utc.with_ymd_and_hms(2025, 3, 29, 8, 0, 0)
                    .unwrap()
                    .fixed_offset()
            )
        );
        assert_eq!(
            iter.next().unwrap().1.with_timezone(&Utc),
            ResolvedDateTime::from(
                Utc.with_ymd_and_hms(2025, 3, 30, 7, 0, 0)
                    .unwrap()
                    .fixed_offset()
            )
        );
        assert_eq!(
            iter.next().unwrap().1.with_timezone(&Utc),
            ResolvedDateTime::from(
                Utc.with_ymd_and_hms(2025, 3, 31, 7, 0, 0)
                    .unwrap()
                    .fixed_offset()
            )
        );
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn shift_to_local_fold_switches_to_utc() {
        let ctx = ctx();
        let tz = Tz::Europe__Berlin;
        let mut ev = timed_event(
            berlin_date(2024, 10, 26, 9, 0),
            berlin_date(2024, 10, 26, 10, 0),
        );
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));

        let new_start = NaiveDate::from_ymd_opt(2024, 10, 27)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(new_start, &tz, DSTEvent::Fold);

        ev.shift_to(&ctx, local_date(&tz, new_start), &tz).unwrap();

        assert_eq!(ev.start(), Some(&utc_date(2024, 10, 27, 0, 30)));
        assert_eq!(ev.end(), Some(&utc_date(2024, 10, 27, 1, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn shift_to_foreign_fold_switches_to_utc() {
        let ctx = ctx();
        let viewer_tz = Tz::Europe__Berlin;
        let mut ev = timed_event(
            new_york_date(2024, 11, 2, 9, 0),
            new_york_date(2024, 11, 2, 10, 0),
        );
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));

        let new_start = NaiveDate::from_ymd_opt(2024, 11, 3)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        assert_tz_event(new_start, &Tz::America__New_York, DSTEvent::Fold);

        ev.shift_to(
            &ctx,
            CalDate::DateTime(CalDateTime::Timezone(
                new_start,
                "America/New_York".to_string(),
            )),
            &viewer_tz,
        )
        .unwrap();

        assert_eq!(ev.start(), Some(&utc_date(2024, 11, 3, 5, 30)));
        assert_eq!(ev.end(), Some(&utc_date(2024, 11, 3, 6, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn shift_to_local_gap_snaps_forward() {
        let ctx = ctx();
        let tz = Tz::Europe__Berlin;
        let mut ev = timed_event(
            berlin_date(2025, 3, 29, 9, 0),
            berlin_date(2025, 3, 29, 10, 0),
        );
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));

        let new_start = NaiveDate::from_ymd_opt(2025, 3, 30)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(new_start, &tz, DSTEvent::Gap);

        ev.shift_to(&ctx, local_date(&tz, new_start), &tz).unwrap();

        assert_eq!(ev.start(), Some(&berlin_date(2025, 3, 30, 3, 30)));
        assert_eq!(ev.end(), Some(&berlin_date(2025, 3, 30, 4, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn shift_to_foreign_gap_snaps_forward() {
        let ctx = ctx();
        let viewer_tz = Tz::Europe__Berlin;
        let mut ev = timed_event(
            new_york_date(2025, 3, 8, 9, 0),
            new_york_date(2025, 3, 8, 10, 0),
        );
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));

        let new_start = NaiveDate::from_ymd_opt(2025, 3, 9)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(new_start, &Tz::America__New_York, DSTEvent::Gap);

        ev.shift_to(
            &ctx,
            CalDate::DateTime(CalDateTime::Timezone(
                new_start,
                "America/New_York".to_string(),
            )),
            &viewer_tz,
        )
        .unwrap();

        assert_eq!(ev.start(), Some(&new_york_date(2025, 3, 9, 3, 30)));
        assert_eq!(ev.end(), Some(&new_york_date(2025, 3, 9, 4, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn shift_to_utc_event_stays_utc() {
        let ctx = ctx();
        let tz = Tz::Europe__Berlin;
        let mut ev = timed_event(utc_date(2025, 3, 29, 9, 0), utc_date(2025, 3, 29, 10, 0));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));

        let new_start = NaiveDate::from_ymd_opt(2025, 3, 30)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(new_start, &tz, DSTEvent::Gap);

        ev.shift_to(&ctx, local_date(&tz, new_start), &tz).unwrap();

        assert_eq!(ev.start(), Some(&utc_date(2025, 3, 30, 1, 30)));
        assert_eq!(ev.end(), Some(&utc_date(2025, 3, 30, 2, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn shift_to_foreign_in_local_gap_remains() {
        let ctx = ctx();
        let viewer_tz = Tz::Europe__Berlin;
        let mut ev = timed_event(
            new_york_date(2026, 3, 28, 9, 0),
            new_york_date(2026, 3, 28, 10, 0),
        );
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));

        let new_start = NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(new_start, &viewer_tz, DSTEvent::Gap);

        ev.shift_to(
            &ctx,
            CalDate::DateTime(CalDateTime::Timezone(
                new_start,
                "America/New_York".to_string(),
            )),
            &viewer_tz,
        )
        .unwrap();

        assert_eq!(ev.start(), Some(&new_york_date(2026, 3, 29, 2, 30)));
        assert_eq!(ev.end(), Some(&new_york_date(2026, 3, 29, 3, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn shift_to_move_out_of_gap_back_to_event_timezone() {
        let ctx = ctx();
        let tz = Tz::Europe__Berlin;

        let old_start = NaiveDate::from_ymd_opt(2025, 3, 30)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        let old_end = NaiveDate::from_ymd_opt(2025, 3, 30)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(old_start, &tz, DSTEvent::None);
        assert_tz_event(old_end, &tz, DSTEvent::Gap);

        let mut ev = timed_event(
            CalDate::DateTime(CalDateTime::Utc(old_start.and_utc())),
            CalDate::DateTime(CalDateTime::Utc(old_end.and_utc())),
        );
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));

        let new_start = NaiveDate::from_ymd_opt(2025, 3, 31)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        assert_tz_event(new_start, &tz, DSTEvent::None);

        ev.shift_to(&ctx, local_date(&tz, new_start), &tz).unwrap();

        assert_eq!(ev.start(), Some(&utc_date(2025, 3, 31, 7, 0)));
        assert_eq!(ev.end(), Some(&utc_date(2025, 3, 31, 8, 0)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn shift_to_all_day_preserves_day_span() {
        let ctx = ctx();
        let mut ev = timed_event(
            CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 10).unwrap(),
                CalCompType::Event.into(),
            ),
            CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 12).unwrap(),
                CalCompType::Event.into(),
            ),
        );

        ev.shift_to(
            &ctx,
            CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 20).unwrap(),
                CalCompType::Event.into(),
            ),
            &Tz::UTC,
        )
        .unwrap();

        assert_eq!(
            ev.start(),
            Some(&CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 20).unwrap(),
                CalCompType::Event.into()
            ))
        );
        assert_eq!(
            ev.end(),
            Some(&CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 22).unwrap(),
                CalCompType::Event.into()
            ))
        );
    }

    #[test]
    fn resize_start_local_fold_converts_unchanged_end_to_utc_too() {
        let ctx = ctx();
        let tz = Tz::Europe__Berlin;
        let mut ev = timed_event(
            berlin_date(2024, 10, 27, 1, 30),
            berlin_date(2024, 10, 27, 4, 30),
        );
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(4)));

        let new_start = NaiveDate::from_ymd_opt(2024, 10, 27)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(new_start, &tz, DSTEvent::Fold);

        ev.resize(&ctx, RangeEdge::Start, local_date(&tz, new_start), &tz)
            .unwrap();

        assert_eq!(ev.start(), Some(&utc_date(2024, 10, 27, 0, 30)));
        assert_eq!(ev.end(), Some(&utc_date(2024, 10, 27, 3, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(3)));
    }

    #[test]
    fn resize_end_local_gap_snaps_forward() {
        let ctx = ctx();
        let tz = Tz::Europe__Berlin;
        let mut ev = timed_event(
            berlin_date(2025, 3, 30, 1, 30),
            berlin_date(2025, 3, 30, 4, 30),
        );

        let new_end = NaiveDate::from_ymd_opt(2025, 3, 30)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        assert_tz_event(new_end, &tz, DSTEvent::Gap);

        ev.resize(&ctx, RangeEdge::End, local_date(&tz, new_end), &tz)
            .unwrap();

        assert_eq!(ev.start(), Some(&berlin_date(2025, 3, 30, 1, 30)));
        assert_eq!(ev.end(), Some(&berlin_date(2025, 3, 30, 3, 30)));
        assert_eq!(ev.actual_duration(&ctx), Some(Duration::hours(1)));
    }

    #[test]
    fn resize_rejects_all_day_events() {
        let ctx = ctx();
        let mut ev = timed_event(
            CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 10).unwrap(),
                CalCompType::Event.into(),
            ),
            CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 11).unwrap(),
                CalCompType::Event.into(),
            ),
        );

        let res = ev.resize(
            &ctx,
            RangeEdge::Start,
            CalDate::Date(
                NaiveDate::from_ymd_opt(2025, 4, 9).unwrap(),
                CalCompType::Event.into(),
            ),
            &Tz::UTC,
        );

        assert_eq!(res, Err(ObjectsError::ResizeAllDay));
    }
}
