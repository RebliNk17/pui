//! Local-calendar separators for the flat, date-sorted sidebar. Children stay
//! with their parent, and pinned/project/name views keep their own grouping.
use super::super::{sidebar_order::Sort, sidebar_tree::Branch};
use chrono::{DateTime, Datelike, Days, Local, NaiveDate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bucket {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    Older,
    Unknown,
}

impl Bucket {
    pub(super) fn for_branch(branch: &Branch, sort: Sort, today: NaiveDate) -> Option<Self> {
        let timestamp = match sort {
            Sort::Activity => Some(branch.modified),
            Sort::Created => branch.created,
            Sort::Name => return None,
        };
        Some(Self::for_date(
            timestamp.map(|time| DateTime::<Local>::from(time).date_naive()),
            today,
        ))
    }

    fn for_date(date: Option<NaiveDate>, today: NaiveDate) -> Self {
        let Some(date) = date else {
            return Self::Unknown;
        };
        let week = today - Days::new(today.weekday().num_days_from_monday().into());
        if date >= today {
            Self::Today
        } else if date == today - Days::new(1) {
            Self::Yesterday
        } else if date >= week {
            Self::ThisWeek
        } else if date >= week - Days::new(7) {
            Self::LastWeek
        } else if date >= today.with_day(1).unwrap() {
            Self::ThisMonth
        } else {
            Self::Older
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This week",
            Self::LastWeek => "Last week",
            Self::ThisMonth => "This month",
            Self::Older => "Older",
            Self::Unknown => "Unknown date",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn calendar_buckets_are_disjoint_and_cover_the_date_boundaries() {
        let today = date("2026-07-24"); // Friday; weeks start on Monday.
        for (value, expected) in [
            ("2026-07-25", Bucket::Today), // Clock skew stays at the top.
            ("2026-07-24", Bucket::Today),
            ("2026-07-23", Bucket::Yesterday),
            ("2026-07-22", Bucket::ThisWeek),
            ("2026-07-20", Bucket::ThisWeek),
            ("2026-07-19", Bucket::LastWeek),
            ("2026-07-13", Bucket::LastWeek),
            ("2026-07-12", Bucket::ThisMonth),
            ("2026-07-01", Bucket::ThisMonth),
            ("2026-06-30", Bucket::Older),
            ("2025-07-24", Bucket::Older),
        ] {
            assert_eq!(
                Bucket::for_date(Some(date(value)), today),
                expected,
                "{value}"
            );
        }
        assert_eq!(Bucket::for_date(None, today), Bucket::Unknown);
    }

    #[test]
    fn yesterday_and_last_week_take_precedence_across_month_and_year_boundaries() {
        for (today, value, expected) in [
            ("2026-06-01", "2026-05-31", Bucket::Yesterday), // Monday
            ("2026-06-01", "2026-05-25", Bucket::LastWeek),
            ("2026-06-01", "2026-05-24", Bucket::Older),
            ("2026-07-05", "2026-06-30", Bucket::ThisWeek), // Sunday
            ("2026-01-01", "2025-12-31", Bucket::Yesterday),
            ("2026-01-01", "2025-12-29", Bucket::ThisWeek),
            ("2026-01-01", "2025-12-28", Bucket::LastWeek),
            ("2026-01-01", "2025-12-21", Bucket::Older),
            ("2024-03-01", "2024-02-29", Bucket::Yesterday),
        ] {
            assert_eq!(
                Bucket::for_date(Some(date(value)), date(today)),
                expected,
                "{today}: {value}"
            );
        }
    }

    #[test]
    fn day_buckets_use_calendar_days_across_daylight_saving_changes() {
        for (today, yesterday) in [("2026-03-09", "2026-03-08"), ("2026-11-02", "2026-11-01")] {
            assert_eq!(
                Bucket::for_date(Some(date(yesterday)), date(today)),
                Bucket::Yesterday
            );
        }
    }
}
