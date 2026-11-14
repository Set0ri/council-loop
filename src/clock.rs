//! The time, with one override for tests: `COUNCIL_NOW=2026-11-13T12:00:00` freezes the clock so that
//! two runs of the same steps write the same run ids, dates and ledgers.

use chrono::{Local, NaiveDate, NaiveDateTime};

/// The local time now, or the frozen time in `COUNCIL_NOW`.
pub fn now() -> NaiveDateTime {
    std::env::var("COUNCIL_NOW")
        .ok()
        .and_then(|text| NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%dT%H:%M:%S").ok())
        .unwrap_or_else(|| Local::now().naive_local())
}

/// Today's date.
pub fn today() -> NaiveDate {
    now().date()
}

/// Today as `2026-11-13`.
pub fn today_iso() -> String {
    today().format("%Y-%m-%d").to_string()
}

/// `2026-11-13` as a date, or `None` when the text is not one.
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}
