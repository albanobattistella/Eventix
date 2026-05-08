// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser utilities for RFC 5545.
//!
//! This module implements various abstractions to deal with the iCalendar format according to RFC
//! 5545.
//!
//! The lowest level provide the [`LineReader`] and [`LineWriter`] that allow their users to work
//! with logical lines, while parsing or producing physical lines as expected by the format (at
//! most 75 bytes per line).
//!
//! On top of them, a line can be turned into a [`Property`] (and a property back into a line),
//! having a name, value, and optionally [`Parameter`]s.
//!
//! Finally, [`PropertyConsumer`] and [`PropertyProducer`] provide means to parse multiple lines
//! into a recursive object structure and back into a vector of [`Property`]s.

mod line;
mod prop;

use std::num::ParseIntError;

use thiserror::Error;

pub use self::line::{LineReader, LineWriter};
pub use self::prop::{Parameter, Property, PropertyConsumer, PropertyProducer};

/// Errors that occur during parsing of iCalendar objects.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("Parse error in line {line}: {ty}")]
pub struct ParseError {
    pub line: usize,
    pub ty: ParseErrorType,
}

impl ParseError {
    pub fn new(line: usize, ty: ParseErrorType) -> Self {
        Self { line, ty }
    }

    /// Returns the error type.
    pub fn ty(&self) -> &ParseErrorType {
        &self.ty
    }

    /// Returns the line number where the error occurred.
    pub fn line_num(&self) -> usize {
        self.line
    }

    /// Sets the line number of this error.
    pub fn with_line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }
}

/// The type of error that occurred during parsing.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ParseErrorType {
    #[error("Missing name end")]
    MissingNameEnd,
    #[error("Missing parameter end")]
    MissingParamEnd,
    #[error("Missing parameter value")]
    MissingParamValue,
    #[error("Unexpected property: {0}")]
    UnexpectedProp(String),
    #[error("Unexpected END:{0}")]
    UnexpectedEnd(String),
    #[error("Unexpected BEGIN:{0}")]
    UnexpectedBegin(String),
    #[error("Duplicate property: {0}")]
    DuplicateProp(String),
    #[error("Invalid weekday description end")]
    UnexpectedWDayEnd,
    #[error("Unexpected rrule {0}")]
    UnexpectedRRule(String),
    #[error("Unexpected end of file")]
    UnexpectedEOF,
    #[error("Invalid percentage: {0}")]
    InvalidPercent(u8),
    #[error("Invalid priority: {0}")]
    InvalidPriority(u8),
    #[error("Invalid sequence: {0}")]
    InvalidSequence(i64),
    #[error("Malformed date: {0}")]
    MalformedDate(String),
    #[error("Invalid date: {0}")]
    InvalidDate(String),
    #[error("Invalid number: {0}")]
    InvalidNumber(ParseIntError),
    #[error("Invalid status: {0}")]
    InvalidStatus(String),
    #[error("Invalid role: {0}")]
    InvalidRole(String),
    #[error("Invalid frequency: {0}")]
    InvalidFrequency(String),
    #[error("Invalid side: {0}")]
    InvalidSide(String),
    #[error("Invalid weekday: {0}")]
    InvalidWeekday(String),
    #[error("Invalid action: {0}")]
    InvalidAction(String),
    #[error("Invalid duration: {0}")]
    InvalidDuration(String),
    #[error("Invalid UTC offset: {0}")]
    InvalidUtcOffset(String),
    #[error("Missing required property: {0}")]
    MissingRequiredProp(String),
    #[error("Non-existent local time: {0}")]
    NonExistentTime(String),
    #[error("Ambiguous local time: {0}")]
    AmbiguousTime(String),
}

impl From<ParseErrorType> for ParseError {
    fn from(error_type: ParseErrorType) -> Self {
        Self {
            line: 0,
            ty: error_type,
        }
    }
}

impl From<ParseIntError> for ParseError {
    fn from(err: ParseIntError) -> Self {
        ParseError::from(ParseErrorType::InvalidNumber(err))
    }
}

/// A wrapper around a [`LineReader`] and a [`ParseError`] that allows to easily return errors with
/// line numbers.
pub trait LineResultExt<T> {
    /// Maps the error to include the current line number of the given [`LineReader`].
    fn with_line<R: std::io::BufRead>(self, reader: &LineReader<R>) -> Result<T, ParseError>;
}

impl<T, E: Into<ParseError>> LineResultExt<T> for Result<T, E> {
    fn with_line<R: std::io::BufRead>(self, reader: &LineReader<R>) -> Result<T, ParseError> {
        self.map_err(|e| e.into().with_line(reader.line_num()))
    }
}
