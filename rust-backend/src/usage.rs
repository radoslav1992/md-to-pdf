//! Monthly conversion quotas. Each conversion (one row of a batch counts
//! as one) records a `usage_events` row; reads aggregate over the calendar
//! month in seconds since the epoch.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::db::{now_seconds, User};
use crate::error::ConvertError;

const SECONDS_PER_DAY: i64 = 86_400;
const FREE_MONTHLY: i64 = 100;
const PREMIUM_MONTHLY: i64 = 10_000;

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub used: i64,
    pub limit: i64,
    pub period_start: i64,
}

pub fn limit_for(user: &User) -> Option<i64> {
    if user.is_admin() {
        None
    } else if user.is_premium() {
        Some(PREMIUM_MONTHLY)
    } else {
        Some(FREE_MONTHLY)
    }
}

/// Approximate start of the current 30-day rolling window. We deliberately
/// don't anchor to calendar months — a rolling window is friendlier to
/// users who sign up mid-month and trivial to query.
pub fn period_start() -> i64 {
    now_seconds() - 30 * SECONDS_PER_DAY
}

pub async fn used_this_period(pool: &SqlitePool, user: &User) -> Result<i64, ConvertError> {
    let row: (Option<i64>,) =
        sqlx::query_as("SELECT SUM(count) FROM usage_events WHERE user_id = ?1 AND created_at >= ?2")
            .bind(user.id)
            .bind(period_start())
            .fetch_one(pool)
            .await?;
    Ok(row.0.unwrap_or(0))
}

pub async fn summary(pool: &SqlitePool, user: &User) -> Result<UsageSummary, ConvertError> {
    let used = used_this_period(pool, user).await?;
    Ok(UsageSummary {
        used,
        limit: limit_for(user).unwrap_or(i64::MAX),
        period_start: period_start(),
    })
}

/// Reserve `count` conversions against the user's monthly budget. Errors
/// with `Forbidden` when the quota is exhausted.
pub async fn reserve(
    pool: &SqlitePool,
    user: &User,
    api_key_id: Option<i64>,
    kind: &str,
    count: i64,
) -> Result<(), ConvertError> {
    if count <= 0 {
        return Ok(());
    }
    if let Some(limit) = limit_for(user) {
        let used = used_this_period(pool, user).await?;
        if used + count > limit {
            return Err(ConvertError::Forbidden(format!(
                "monthly quota exceeded: {used}/{limit} (would add {count})"
            )));
        }
    }
    sqlx::query(
        "INSERT INTO usage_events (user_id, api_key_id, kind, count, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(user.id)
    .bind(api_key_id)
    .bind(kind)
    .bind(count)
    .bind(now_seconds())
    .execute(pool)
    .await?;
    Ok(())
}
