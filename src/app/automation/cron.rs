//! Numeric five-field cron, evaluated in UTC. No provider calls or local DST ambiguity.
use chrono::{Datelike, Timelike, Utc};

pub(super) struct Cron {
    fields: Vec<Vec<u32>>,
    day_any: bool,
    weekday_any: bool,
}
impl Cron {
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<_> = text.split_whitespace().collect();
        if parts.len() != 5 || text.len() > 256 {
            return Err(
                "Use five numeric cron fields: minute hour day month weekday (UTC).".into(),
            );
        }
        let mut fields = Vec::new();
        for (part, (min, max)) in parts
            .iter()
            .zip([(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)])
        {
            let mut values = Vec::new();
            for item in part.split(',') {
                let (range, step) = match item.split_once('/') {
                    Some((range, step)) => {
                        (range, step.parse::<u32>().map_err(|_| "Invalid cron step")?)
                    }
                    None => (item, 1),
                };
                if step == 0 || step > max + 1 {
                    return Err("Invalid cron step".into());
                }
                let (start, end) = if range == "*" {
                    (min, max)
                } else if let Some((a, b)) = range.split_once('-') {
                    (
                        a.parse().map_err(|_| "Invalid cron range")?,
                        b.parse().map_err(|_| "Invalid cron range")?,
                    )
                } else {
                    let n = range.parse().map_err(|_| "Invalid cron number")?;
                    (n, if item.contains('/') { max } else { n })
                };
                if start < min || end > max || start > end {
                    return Err("Cron field out of range".into());
                }
                values.extend((start..=end).step_by(step as usize));
            }
            values.sort_unstable();
            values.dedup();
            fields.push(values);
        }
        Ok(Self {
            fields,
            day_any: parts[2].starts_with('*'),
            weekday_any: parts[4].starts_with('*'),
        })
    }
    pub fn next(&self, after: i64) -> Result<i64, String> {
        let mut t = after - after.rem_euclid(60) + 60;
        // Gregorian dates repeat within this bounded horizon, including leap day.
        for _ in 0..(8 * 366 * 24 * 60) {
            let d = chrono::DateTime::<Utc>::from_timestamp(t, 0)
                .ok_or("Invalid schedule timestamp")?;
            let day = self.fields[2].contains(&d.day());
            let w = d.weekday().num_days_from_sunday();
            let weekday = self.fields[4].contains(&w) || (w == 0 && self.fields[4].contains(&7));
            let date = match (self.day_any, self.weekday_any) {
                (false, false) => day || weekday,
                _ => day && weekday,
            };
            if !self.fields[3].contains(&d.month()) || !date {
                t += (24 * 60 * 60 - d.num_seconds_from_midnight()) as i64;
            } else if !self.fields[1].contains(&d.hour()) {
                t += (3600 - d.minute() * 60 - d.second()) as i64;
            } else if self.fields[0].contains(&d.minute()) {
                return Ok(t);
            } else {
                t += 60;
            }
            if t > after + 8 * 366 * 24 * 60 * 60 {
                break;
            }
        }
        Err("Cron has no occurrence in the next eight years.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    #[test]
    fn numeric_cron_ranges_steps_sunday_and_day_or() {
        let t = Utc
            .with_ymd_and_hms(2026, 1, 4, 8, 1, 0)
            .unwrap()
            .timestamp(); // Sunday
        assert_eq!(
            Cron::parse("*/15 8-10 * * 0,7").unwrap().next(t).unwrap(),
            t + 14 * 60
        );
        assert_eq!(Cron::parse("2 8 31 * 7").unwrap().next(t).unwrap(), t + 60);
        assert_eq!(
            Cron::parse("0 0 29 2 *").unwrap().next(t).unwrap(),
            Utc.with_ymd_and_hms(2028, 2, 29, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
        for text in [
            "* * * *",
            "60 * * * *",
            "*/0 * * * *",
            "0 0 * 13 *",
            "0 0 * * MON",
            "0 5-2 * * *",
            "0 0 * * 8",
        ] {
            assert!(Cron::parse(text).is_err(), "{text}");
        }
        assert!(Cron::parse("0 0 31 2 *").unwrap().next(t).is_err());
    }
}
