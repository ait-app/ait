use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use domain::schedule::Cadence;
use domain::storage::schedule::Error;

/// Days in one Gregorian cycle; dates and weekdays repeat exactly after it.
const GREGORIAN_CYCLE_DAYS: u32 = 146_097;

/// Returns the next trigger, treating a cadence without any future trigger as invalid.
pub(crate) fn next(cadence: &Cadence, after: DateTime<Utc>) -> Result<DateTime<Utc>, Error> {
    upcoming(cadence, after)?.ok_or(Error::Invalid)
}

/// Returns the next trigger after `after`, or `None` when the cadence never fires again.
pub(crate) fn upcoming(
    cadence: &Cadence,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, Error> {
    match cadence {
        Cadence::Once { at } => Ok(Some(*at)),
        Cadence::Every { every_ms } if *every_ms > 0 => after
            .checked_add_signed(Duration::milliseconds(*every_ms))
            .map(Some)
            .ok_or(Error::Invalid),
        Cadence::Every { .. } => Err(Error::Invalid),
        Cadence::Cron {
            expression,
            timezone,
        } => {
            let fields = parse(expression)?;
            let zone: Tz = timezone
                .as_deref()
                .unwrap_or("UTC")
                .parse()
                .map_err(|_| Error::Invalid)?;
            let seconds = after
                .timestamp()
                .div_euclid(60)
                .checked_add(1)
                .and_then(|n| n.checked_mul(60))
                .ok_or(Error::Invalid)?;
            let cursor = DateTime::from_timestamp(seconds, 0).ok_or(Error::Invalid)?;
            let mut date = cursor.with_timezone(&zone).date_naive();
            // Every date and weekday combination recurs within one Gregorian cycle.
            for _ in 0..=GREGORIAN_CYCLE_DAYS {
                if let Some(at) = first_on(&fields, zone, date, cursor) {
                    return Ok(Some(at));
                }
                date = date.succ_opt().ok_or(Error::Invalid)?;
            }
            Ok(None)
        }
    }
}

/// Returns the earliest instant at or after `cursor` whose local time on `date` matches.
fn first_on(
    fields: &[u64; 5],
    zone: Tz,
    date: NaiveDate,
    cursor: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let [minutes, hours, days, months, weekdays] = *fields;
    if !(has(days, date.day())
        && has(months, date.month())
        && has(weekdays, date.weekday().num_days_from_sunday()))
    {
        return None;
    }
    let mut best: Option<DateTime<Utc>> = None;
    for hour in (0..24).filter(|hour| has(hours, *hour)) {
        for minute in (0..60).filter(|minute| has(minutes, *minute)) {
            let local = date.and_hms_opt(hour, minute, 0)?;
            match zone.from_local_datetime(&local) {
                // Later local times resolve to later instants, so a single match ends the day.
                LocalResult::Single(at) => {
                    let at = at.with_timezone(&Utc);
                    if at >= cursor {
                        return Some(best.map_or(at, |best| best.min(at)));
                    }
                }
                // Repeated fall-back times fire on both passes.
                LocalResult::Ambiguous(first, second) => {
                    for at in [first, second] {
                        let at = at.with_timezone(&Utc);
                        if at >= cursor && best.is_none_or(|best| at < best) {
                            best = Some(at);
                        }
                    }
                }
                // Spring-forward gaps never occur locally.
                LocalResult::None => {}
            }
        }
    }
    best
}

fn has(mask: u64, value: u32) -> bool {
    mask & (1u64 << value) != 0
}

fn parse(expression: &str) -> Result<[u64; 5], Error> {
    let parts: Vec<_> = expression.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(Error::Invalid);
    }
    let mut fields = [0; 5];
    for (index, (min, max)) in [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)]
        .into_iter()
        .enumerate()
    {
        for part in parts[index].split(',') {
            let (base, step) = match part.split_once('/') {
                Some((base, step)) => {
                    let parsed: u32 = step.parse().map_err(|_| Error::Invalid)?;
                    if parsed == 0 || parsed.to_string() != step {
                        return Err(Error::Invalid);
                    }
                    (base, parsed)
                }
                None => (part, 1),
            };
            let (start, end) = if base == "*" {
                (min, max)
            } else if let Some((start, end)) = base.split_once('-') {
                (number(start)?, number(end)?)
            } else {
                let value = number(base)?;
                (value, value)
            };
            if start < min || end > max || start > end {
                return Err(Error::Invalid);
            }
            let mut value = start;
            loop {
                fields[index] |= 1u64 << value;
                let Some(next) = value.checked_add(step) else {
                    break;
                };
                if next > end {
                    break;
                }
                value = next;
            }
        }
    }
    Ok(fields)
}
fn number(value: &str) -> Result<u32, Error> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid);
    }
    value.parse().map_err(|_| Error::Invalid)
}
#[cfg(test)]
mod tests;
