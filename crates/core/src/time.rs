//! Unix seconds and RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`), and the period clock.

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `None` outside years 0000..=9999.
pub fn rfc3339(unix: i64) -> Option<String> {
    let (year, month, day) = civil(unix.div_euclid(86_400));
    if !(0..=9999).contains(&year) {
        return None;
    }
    let secs = unix.rem_euclid(86_400);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    ))
}

pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let shape = b.len() == 20
        && b.iter().enumerate().all(|(i, &c)| match i {
            4 | 7 => c == b'-',
            10 => c == b'T',
            13 | 16 => c == b':',
            19 => c == b'Z',
            _ => c.is_ascii_digit(),
        });
    if !shape {
        return None;
    }
    let num = |range: std::ops::Range<usize>| s[range].parse::<u32>().ok();
    let (year, month, day) = (i64::from(num(0..4)?), num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let secs = i64::from(hour * 3600 + minute * 60 + second);
    Some(days_from_civil(year, month, day) * 86_400 + secs)
}

/// The period `n` holding clock time `t`; `None` before genesis.
pub fn period_of(t: i64, genesis: i64, period_s: u32) -> Option<u64> {
    let elapsed = u64::try_from(t.checked_sub(genesis)?).ok()?;
    Some(elapsed / u64::from(period_s))
}

pub fn period_start(n: u64, genesis: i64, period_s: u32) -> Option<i64> {
    i64::try_from(n)
        .ok()?
        .checked_mul(i64::from(period_s))?
        .checked_add(genesis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        let cases = [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_794_873_599, "2026-11-16T23:59:59Z"),
            (-1, "1969-12-31T23:59:59Z"),
            (253_402_300_799, "9999-12-31T23:59:59Z"),
        ];
        for (unix, text) in cases {
            assert_eq!(rfc3339(unix).as_deref(), Some(text));
            assert_eq!(parse_rfc3339(text), Some(unix));
        }
        assert_eq!(rfc3339(253_402_300_800), None);
    }

    #[test]
    fn round_trips_across_centuries() {
        let mut t = -62_000_000_000i64;
        while t < 253_000_000_000 {
            let text = rfc3339(t).expect("in range");
            assert_eq!(parse_rfc3339(&text), Some(t), "{text}");
            t += 86_399 * 37 + 13;
        }
    }

    #[test]
    fn rejects_malformed_or_impossible_times() {
        for bad in [
            "2023-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-11-16T24:00:00Z",
            "2026-11-16T12:60:00Z",
            "2026-11-16t12:00:00Z",
            "2026-11-16T12:00:00+00:00",
            "2026-11-16T12:00:00.5Z",
            "2026-1-16T12:00:00Z",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
        assert!(parse_rfc3339("2000-02-29T00:00:00Z").is_some());
    }

    #[test]
    fn periods_follow_the_clock() {
        let genesis = 1_800_000_000;
        assert_eq!(period_of(genesis - 1, genesis, 3600), None);
        assert_eq!(period_of(genesis, genesis, 3600), Some(0));
        assert_eq!(period_of(genesis + 3599, genesis, 3600), Some(0));
        assert_eq!(period_of(genesis + 7200, genesis, 3600), Some(2));
        assert_eq!(period_start(2, genesis, 3600), Some(genesis + 7200));
        assert_eq!(period_start(u64::MAX, genesis, 3600), None);
    }
}
