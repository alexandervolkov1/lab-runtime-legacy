//! Display-only time formatting. Plot coordinates remain Runtime monotonic seconds.

use crate::{presentation::v2::TimeAxisMode, time_context::TimeAnchor};
use std::ops::RangeInclusive;
use time::{OffsetDateTime, UtcOffset};

pub(crate) struct Axis<'a> {
    pub(crate) mode: TimeAxisMode,
    pub(crate) note: &'static str,
    anchor: Option<&'a TimeAnchor>,
}

impl<'a> Axis<'a> {
    pub(crate) fn new(mode: Option<TimeAxisMode>, anchor: Option<&'a TimeAnchor>) -> Self {
        let requested = mode.unwrap_or(TimeAxisMode::LocalCalendar);
        let calendar = matches!(requested, TimeAxisMode::LocalCalendar | TimeAxisMode::Utc);
        let usable = anchor.filter(|a| {
            a.after_ns
                .checked_sub(a.before_ns)
                .is_some_and(|span| span <= 1_000_000_000)
                && OffsetDateTime::from_unix_timestamp_nanos(i128::from(a.utc_us) * 1000)
                    .ok()
                    .is_some_and(|utc| {
                        requested != TimeAxisMode::LocalCalendar
                            || UtcOffset::local_offset_at(utc).is_ok()
                    })
        });
        let (mode, note) = if requested == TimeAxisMode::ExperimentElapsed {
            (
                TimeAxisMode::RuntimeElapsed,
                "Experiment start unavailable; showing Runtime elapsed",
            )
        } else if calendar && usable.is_none() {
            (
                TimeAxisMode::RuntimeElapsed,
                "UTC binding unavailable or uncertain; showing Runtime elapsed",
            )
        } else if calendar {
            (
                requested,
                "OS UTC estimate; absolute accuracy unverified (fixed boot anchor)",
            )
        } else {
            (requested, "Elapsed since Runtime monotonic origin")
        };
        Self {
            mode,
            note,
            anchor: usable,
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self.mode {
            TimeAxisMode::LocalCalendar => "Local calendar time (OS UTC estimate)",
            TimeAxisMode::Utc => "UTC (OS clock estimate)",
            _ => "Runtime elapsed",
        }
    }

    fn utc(&self, seconds: f64) -> Option<OffsetDateTime> {
        if !seconds.is_finite() || seconds.abs() > u64::MAX as f64 / 1e9 {
            return None;
        }
        let a = self.anchor?;
        let ns = (seconds * 1e9).round() as i128;
        OffsetDateTime::from_unix_timestamp_nanos(
            i128::from(a.utc_us) * 1_000 + ns - i128::from(a.after_ns),
        )
        .ok()
    }

    fn calendar(&self, seconds: f64) -> Option<OffsetDateTime> {
        self.calendar_with(seconds, |at| UtcOffset::local_offset_at(at).ok())
    }
    fn calendar_with(
        &self,
        seconds: f64,
        offset: impl Fn(OffsetDateTime) -> Option<UtcOffset>,
    ) -> Option<OffsetDateTime> {
        let utc = self.utc(seconds)?;
        if self.mode == TimeAxisMode::LocalCalendar {
            // Resolve each measurement instant, including historical DST transitions.
            Some(utc.to_offset(offset(utc)?))
        } else {
            Some(utc)
        }
    }

    pub(crate) fn tick(&self, seconds: f64, range: &RangeInclusive<f64>) -> String {
        if self.mode == TimeAxisMode::RuntimeElapsed {
            return elapsed(seconds);
        }
        let Some(at) = self.calendar(seconds) else {
            return format!("{} (calendar unavailable)", elapsed(seconds));
        };
        let dates_differ = self
            .calendar(*range.start())
            .zip(self.calendar(*range.end()))
            .is_some_and(|(first, last)| first.date() != last.date());
        calendar_label(
            at,
            dates_differ || (range.end() - range.start()).abs() >= 86_400.,
        )
    }

    pub(crate) fn tooltip(&self, seconds: f64) -> String {
        if self.mode == TimeAxisMode::RuntimeElapsed {
            return format!("{} (Runtime monotonic {:.3} s)", elapsed(seconds), seconds);
        }
        self.calendar(seconds).map_or_else(
            || format!("{} (calendar unavailable)", elapsed(seconds)),
            |at| {
                format!(
                    "{} {} · {}",
                    calendar_label(at, true),
                    at.offset(),
                    self.note
                )
            },
        )
    }
}

fn calendar_label(at: OffsetDateTime, date: bool) -> String {
    let time = format!("{:02}:{:02}:{:02}", at.hour(), at.minute(), at.second());
    if date {
        format!(
            "{:04}-{:02}-{:02} {time}",
            at.year(),
            u8::from(at.month()),
            at.day()
        )
    } else {
        time
    }
}

fn elapsed(seconds: f64) -> String {
    if !seconds.is_finite() || seconds.abs() > u64::MAX as f64 {
        return "Time unavailable".into();
    }
    let sign = if seconds < 0. { "−" } else { "+" };
    let total = seconds.abs().floor() as u64;
    let (days, hours, minutes, seconds) = (
        total / 86400,
        total / 3600 % 24,
        total / 60 % 60,
        total % 60,
    );
    if days == 0 {
        format!("T{sign}{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("T{sign}{days}d {hours:02}:{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn anchor() -> TimeAnchor {
        TimeAnchor {
            boot_id: "boot-a".into(),
            before_ns: 9_999_999_000,
            after_ns: 10_000_000_000,
            utc_us: 1_735_689_599_000_000,
        }
    }
    #[test]
    fn known_mapping_midnight_multiday_and_relative_origin() {
        let anchor = anchor(); // 2024-12-31 23:59:59 UTC at monotonic 10 s.
        let axis = Axis::new(Some(TimeAxisMode::Utc), Some(&anchor));
        assert_eq!(axis.tick(10., &(9.0..=10.5)), "23:59:59");
        assert_eq!(axis.tick(11., &(9.0..=12.)), "2025-01-01 00:00:00");
        assert!(axis.tooltip(11.).contains("2025-01-01 00:00:00 +00:00:00"));
        assert_eq!(axis.tick(172811., &(0.0..=180000.)), "2025-01-03 00:00:00");
        assert_eq!(elapsed(763.), "T+00:12:43");
        assert_eq!(elapsed(86401.), "T+1d 00:00:01");
        assert_eq!(axis.utc(9.).unwrap().unix_timestamp(), 1_735_689_598);
    }
    #[test]
    fn timezone_and_dst_labels_preserve_distinct_instants() {
        let utc = OffsetDateTime::from_unix_timestamp(1_735_689_599).unwrap();
        assert_eq!(
            calendar_label(utc.to_offset(UtcOffset::from_hms(3, 0, 0).unwrap()), true),
            "2025-01-01 02:59:59"
        );
        // Europe/Berlin 2025 transitions: the offset is resolved per instant,
        // never cached once at launch. Offset text disambiguates the repeated hour.
        let spring = OffsetDateTime::from_unix_timestamp(1_743_296_400).unwrap();
        let fall = OffsetDateTime::from_unix_timestamp(1_761_440_400).unwrap();
        for (transition, before, after, expected_before, expected_after) in [
            (spring, 1, 2, "01:59:59", "03:00:00"),
            (fall, 2, 1, "02:59:59", "02:00:00"),
        ] {
            let mut a = anchor();
            a.utc_us = transition.unix_timestamp() * 1_000_000;
            let axis = Axis::new(Some(TimeAxisMode::LocalCalendar), Some(&a));
            let offset = |at: OffsetDateTime| {
                UtcOffset::from_hms(if at < transition { before } else { after }, 0, 0).ok()
            };
            assert_eq!(
                calendar_label(axis.calendar_with(9., offset).unwrap(), false),
                expected_before
            );
            assert_eq!(
                calendar_label(axis.calendar_with(10., offset).unwrap(), false),
                expected_after
            );
            assert_eq!(
                calendar_label(
                    (transition - time::Duration::seconds(1))
                        .to_offset(UtcOffset::from_hms(before, 0, 0).unwrap()),
                    false
                ),
                expected_before
            );
            assert_eq!(
                calendar_label(
                    transition.to_offset(UtcOffset::from_hms(after, 0, 0).unwrap()),
                    false
                ),
                expected_after
            );
        }
    }
    #[test]
    fn unavailable_experiment_and_uncertain_binding_fall_back() {
        let axis = Axis::new(None, None);
        assert_eq!(axis.mode, TimeAxisMode::RuntimeElapsed);
        assert_eq!(axis.tick(763., &(0.0..=1000.)), "T+00:12:43");
        assert!(axis.note.contains("unavailable"));
        assert_eq!(
            Axis::new(Some(TimeAxisMode::ExperimentElapsed), Some(&anchor())).mode,
            TimeAxisMode::RuntimeElapsed
        );
        let mut a = anchor();
        a.before_ns = 0;
        assert_eq!(Axis::new(None, Some(&a)).mode, TimeAxisMode::RuntimeElapsed);
    }
}
