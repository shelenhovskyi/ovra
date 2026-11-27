//! When a schedule runs next, in its own time zone (bundled database, so
//! nothing is read from the OS).

use jiff::tz::TimeZone;
use jiff::{Span, Timestamp};

use crate::protocol::schedules::ScheduleTime;

/// `HH:MM` as hour and minute, if it is a time of day.
pub(super) fn parse(time: &str) -> Option<(i8, i8)> {
    let (h, m) = time.split_once(':')?;
    let digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || !digits(m) {
        return None;
    }
    let (h, m) = (h.parse::<i8>().ok()?, m.parse::<i8>().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((h, m))
}

/// Whether `name` is a time zone the bundled database knows.
pub(super) fn known_zone(name: &str) -> bool {
    TimeZone::get(name).is_ok()
}

/// The first run strictly after `after_ms`, in ms since the Unix epoch;
/// `None` if `when` cannot be read. A local time that a clock change skips
/// runs at the first moment after the gap.
pub(super) fn next_after(when: &ScheduleTime, after_ms: u64) -> Option<u64> {
    let tz = TimeZone::get(&when.time_zone).ok()?;
    let (hour, minute) = parse(&when.time)?;
    let after = i64::try_from(after_ms).ok()?;
    let today = Timestamp::from_millisecond(after)
        .ok()?
        .to_zoned_datetime(tz.clone())
        .date();
    // A week and a day: today's time may have passed.
    for day in 0..=7 {
        let date = today.checked_add(Span::new().days(day)).ok()?;
        let weekday = u8::try_from(date.weekday().to_monday_one_offset()).ok()?;
        if !when.days.contains(&weekday) {
            continue;
        }
        let at = date
            .at(hour, minute, 0, 0)
            .to_zoned_datetime(tz.clone())
            .ok()?
            .timestamp()
            .as_millisecond();
        if at > after {
            return u64::try_from(at).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn when(days: &[u8], time: &str, zone: &str) -> ScheduleTime {
        ScheduleTime {
            days: days.to_vec(),
            time: time.into(),
            time_zone: zone.into(),
        }
    }

    /// `2026-11-27T10:00:00Z` (a Friday) and so on, as ms.
    fn ms(iso: &str) -> u64 {
        let ts: Timestamp = iso.parse().unwrap_or_else(|e| panic!("{e}"));
        u64::try_from(ts.as_millisecond()).unwrap_or_default()
    }

    const EVERY_DAY: &[u8] = &[1, 2, 3, 4, 5, 6, 7];

    #[test]
    fn times_of_day_are_two_digits_each() {
        assert_eq!(parse("03:05"), Some((3, 5)));
        assert_eq!(parse("23:59"), Some((23, 59)));
        for bad in ["3:05", "24:00", "12:60", "12-00", "", "ab:cd", "+1:00"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert!(known_zone("Europe/Kyiv"));
        assert!(!known_zone("Mars/Base"));
    }

    #[test]
    fn every_day_runs_today_if_the_time_is_still_ahead_else_tomorrow() {
        let utc = when(EVERY_DAY, "12:00", "UTC");
        let friday_ten = ms("2026-11-27T10:00:00Z");
        assert_eq!(
            next_after(&utc, friday_ten),
            Some(ms("2026-11-27T12:00:00Z"))
        );
        let at_noon = ms("2026-11-27T12:00:00Z");
        assert_eq!(next_after(&utc, at_noon), Some(ms("2026-11-28T12:00:00Z")));
    }

    #[test]
    fn chosen_days_skip_the_others_in_the_local_zone() {
        // Monday and Wednesday at 03:00 in Kyiv (UTC+2 in November).
        let kyiv = when(&[1, 3], "03:00", "Europe/Kyiv");
        let friday = ms("2026-11-27T10:00:00Z");
        assert_eq!(next_after(&kyiv, friday), Some(ms("2026-11-30T01:00:00Z")));
        let monday_after = ms("2026-11-30T01:00:00Z");
        assert_eq!(
            next_after(&kyiv, monday_after),
            Some(ms("2026-12-02T01:00:00Z"))
        );
        assert_eq!(next_after(&when(&[], "03:00", "UTC"), friday), None);
        assert_eq!(next_after(&when(&[1], "03:00", "Mars/Base"), friday), None);
    }

    #[test]
    fn the_local_time_stays_across_a_clock_change() {
        // Kyiv moves to summer time on 2027-03-28 at 03:00 (to 04:00).
        let kyiv = when(EVERY_DAY, "09:00", "Europe/Kyiv");
        let before = ms("2027-03-27T12:00:00Z");
        assert_eq!(next_after(&kyiv, before), Some(ms("2027-03-28T06:00:00Z")));
        // 03:30 does not exist that day: it runs when the clock reads 04:30.
        let gap = when(EVERY_DAY, "03:30", "Europe/Kyiv");
        assert_eq!(next_after(&gap, before), Some(ms("2027-03-28T01:30:00Z")));
    }
}
