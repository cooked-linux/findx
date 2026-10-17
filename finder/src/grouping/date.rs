//! Temporal date binning engine and midnight invalidation timer.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Local, Timelike};
use shared::models::FileItem;

use super::models::{GroupDimension, GroupKey};

pub const PRIORITY_TODAY: u32 = 0;
pub const PRIORITY_YESTERDAY: u32 = 1;
pub const PRIORITY_PREV_7_DAYS: u32 = 2;
pub const PRIORITY_PREV_30_DAYS: u32 = 3;
pub const PRIORITY_EARLIER_THIS_YEAR: u32 = 4;
pub const PRIORITY_PREVIOUS_YEARS: u32 = 5;

pub const ID_TODAY: &str = "today";
pub const ID_YESTERDAY: &str = "yesterday";
pub const ID_PREV_7_DAYS: &str = "prev_7_days";
pub const ID_PREV_30_DAYS: &str = "prev_30_days";
pub const ID_EARLIER_THIS_YEAR: &str = "earlier_this_year";
pub const ID_PREVIOUS_YEARS: &str = "previous_years";

pub const TITLE_TODAY: &str = "Today";
pub const TITLE_YESTERDAY: &str = "Yesterday";
pub const TITLE_PREV_7_DAYS: &str = "Previous 7 Days";
pub const TITLE_PREV_30_DAYS: &str = "Previous 30 Days";
pub const TITLE_EARLIER_THIS_YEAR: &str = "Earlier this Year";
pub const TITLE_PREVIOUS_YEARS: &str = "Previous Years";

/// Returns all possible date bin [`GroupKey`]s in priority order.
#[must_use]
pub fn all_date_group_keys() -> Vec<GroupKey> {
    vec![
        GroupKey::new(PRIORITY_TODAY, ID_TODAY, TITLE_TODAY),
        GroupKey::new(PRIORITY_YESTERDAY, ID_YESTERDAY, TITLE_YESTERDAY),
        GroupKey::new(PRIORITY_PREV_7_DAYS, ID_PREV_7_DAYS, TITLE_PREV_7_DAYS),
        GroupKey::new(PRIORITY_PREV_30_DAYS, ID_PREV_30_DAYS, TITLE_PREV_30_DAYS),
        GroupKey::new(
            PRIORITY_EARLIER_THIS_YEAR,
            ID_EARLIER_THIS_YEAR,
            TITLE_EARLIER_THIS_YEAR,
        ),
        GroupKey::new(
            PRIORITY_PREVIOUS_YEARS,
            ID_PREVIOUS_YEARS,
            TITLE_PREVIOUS_YEARS,
        ),
    ]
}

/// Classifies a [`FileItem`] into a temporal date bin using the given dimension and reference time.
#[must_use]
pub fn classify_date(item: &FileItem, dimension: GroupDimension, now: DateTime<Local>) -> GroupKey {
    let timestamp = match dimension {
        GroupDimension::DateAdded => item.metadata.created.or(item.metadata.modified),
        _ => item.metadata.modified.or(item.metadata.created),
    };

    let Some(ts) = timestamp else {
        return GroupKey::new(
            PRIORITY_PREVIOUS_YEARS,
            ID_PREVIOUS_YEARS,
            TITLE_PREVIOUS_YEARS,
        );
    };

    classify_system_time(ts, now)
}

/// Classifies an arbitrary [`SystemTime`] into a relative date bin compared to `now`.
#[must_use]
pub fn classify_system_time(timestamp: SystemTime, now: DateTime<Local>) -> GroupKey {
    let file_dt: DateTime<Local> = DateTime::from(timestamp);
    let file_date = file_dt.date_naive();
    let today_date = now.date_naive();

    if file_date >= today_date {
        return GroupKey::new(PRIORITY_TODAY, ID_TODAY, TITLE_TODAY);
    }

    let yesterday_date = today_date.pred_opt().unwrap_or(today_date);
    if file_date == yesterday_date {
        return GroupKey::new(PRIORITY_YESTERDAY, ID_YESTERDAY, TITLE_YESTERDAY);
    }

    let seven_days_ago = today_date - chrono::Duration::days(7);
    if file_date >= seven_days_ago {
        return GroupKey::new(PRIORITY_PREV_7_DAYS, ID_PREV_7_DAYS, TITLE_PREV_7_DAYS);
    }

    let thirty_days_ago = today_date - chrono::Duration::days(30);
    if file_date >= thirty_days_ago {
        return GroupKey::new(PRIORITY_PREV_30_DAYS, ID_PREV_30_DAYS, TITLE_PREV_30_DAYS);
    }

    let current_year = today_date.year();
    if file_date.year() == current_year {
        GroupKey::new(
            PRIORITY_EARLIER_THIS_YEAR,
            ID_EARLIER_THIS_YEAR,
            TITLE_EARLIER_THIS_YEAR,
        )
    } else {
        GroupKey::new(
            PRIORITY_PREVIOUS_YEARS,
            ID_PREVIOUS_YEARS,
            TITLE_PREVIOUS_YEARS,
        )
    }
}

/// Calculates the duration from `now` until the next 00:00:00 midnight in local time,
/// including a 50ms buffer to ensure local calendar rollover.
#[must_use]
pub fn duration_until_midnight(now: DateTime<Local>) -> Duration {
    let tomorrow_date = now
        .date_naive()
        .succ_opt()
        .unwrap_or_else(|| now.date_naive());
    let margin = Duration::from_millis(50);

    if let Some(midnight_naive) = tomorrow_date.and_hms_opt(0, 0, 0)
        && let Some(target_dt) = midnight_naive.and_local_timezone(Local).earliest()
        && let Ok(duration) = (target_dt - now).to_std()
    {
        return duration.saturating_add(margin);
    }

    // Fallback based on remaining seconds in naive day
    let seconds_today = now.naive_local().num_seconds_from_midnight();
    let seconds_left = u64::from(86_400u32.saturating_sub(seconds_today)).max(1);
    Duration::from_secs(seconds_left).saturating_add(margin)
}

/// Schedules a GTK / `GLib` one-shot timer that fires at the next local midnight,
/// calling `on_midnight` to invalidate date bins without needing filesystem re-enumeration.
pub fn schedule_midnight_invalidation<F>(on_midnight: F) -> glib::SourceId
where
    F: Fn() + 'static,
{
    let duration = duration_until_midnight(Local::now());
    glib::timeout_add_local_once(duration, on_midnight)
}

/// Asynchronously sleeps until the next local midnight.
pub async fn wait_until_midnight() {
    let duration = duration_until_midnight(Local::now());
    tokio::time::sleep(duration).await;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use chrono::TimeZone;
    use shared::models::FileMetadata;

    use super::*;

    fn create_item_with_time(modified: Option<SystemTime>) -> FileItem {
        let meta = FileMetadata {
            size_bytes: 1024,
            modified,
            accessed: None,
            created: None,
            permissions: 0o644,
            inode: 1,
            device_id: 1,
            mime_type: "text/plain".to_string(),
            is_hidden: false,
            is_executable: false,
        };
        FileItem::new(
            PathBuf::from("/test.txt"),
            "test.txt",
            false,
            false,
            None,
            meta,
            Vec::new(),
        )
    }

    #[test]
    fn test_date_binning_boundaries() {
        let ref_dt = Local
            .with_ymd_and_hms(2026, 10, 15, 14, 30, 0)
            .single()
            .unwrap();

        // 1. Today (same day earlier)
        let t_today = Local
            .with_ymd_and_hms(2026, 10, 15, 9, 0, 0)
            .single()
            .unwrap();
        let item_today = create_item_with_time(Some(SystemTime::from(t_today)));
        assert_eq!(
            classify_date(&item_today, GroupDimension::DateModified, ref_dt).id,
            ID_TODAY
        );

        // 2. Yesterday
        let t_yesterday = Local
            .with_ymd_and_hms(2026, 10, 14, 18, 0, 0)
            .single()
            .unwrap();
        let item_yesterday = create_item_with_time(Some(SystemTime::from(t_yesterday)));
        assert_eq!(
            classify_date(&item_yesterday, GroupDimension::DateModified, ref_dt).id,
            ID_YESTERDAY
        );

        // 3. Previous 7 Days (4 days ago)
        let t_4days = Local
            .with_ymd_and_hms(2026, 10, 11, 12, 0, 0)
            .single()
            .unwrap();
        let item_4days = create_item_with_time(Some(SystemTime::from(t_4days)));
        assert_eq!(
            classify_date(&item_4days, GroupDimension::DateModified, ref_dt).id,
            ID_PREV_7_DAYS
        );

        // 4. Previous 30 Days (20 days ago)
        let t_20days = Local
            .with_ymd_and_hms(2026, 9, 25, 12, 0, 0)
            .single()
            .unwrap();
        let item_20days = create_item_with_time(Some(SystemTime::from(t_20days)));
        assert_eq!(
            classify_date(&item_20days, GroupDimension::DateModified, ref_dt).id,
            ID_PREV_30_DAYS
        );

        // 5. Earlier this Year (5 months ago, May 2026)
        let t_earlier_year = Local
            .with_ymd_and_hms(2026, 5, 1, 12, 0, 0)
            .single()
            .unwrap();
        let item_earlier_year = create_item_with_time(Some(SystemTime::from(t_earlier_year)));
        assert_eq!(
            classify_date(&item_earlier_year, GroupDimension::DateModified, ref_dt).id,
            ID_EARLIER_THIS_YEAR
        );

        // 6. Previous Years (2025)
        let t_prev_year = Local
            .with_ymd_and_hms(2025, 12, 20, 12, 0, 0)
            .single()
            .unwrap();
        let item_prev_year = create_item_with_time(Some(SystemTime::from(t_prev_year)));
        assert_eq!(
            classify_date(&item_prev_year, GroupDimension::DateModified, ref_dt).id,
            ID_PREVIOUS_YEARS
        );
    }

    #[test]
    fn test_duration_until_midnight() {
        let now = Local
            .with_ymd_and_hms(2026, 10, 15, 23, 59, 50)
            .single()
            .unwrap();
        let dur = duration_until_midnight(now);
        // Approx 10 seconds + 50ms margin
        assert!(dur >= Duration::from_secs(10) && dur <= Duration::from_secs(11));
    }
}
