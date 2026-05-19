//! Background jobs. Used for conversions and batches that exceed the
//! synchronous payload caps (e.g. 100+ items, or content that takes more
//! than a few seconds to render). A single tokio worker dequeues rows from
//! the `jobs` table, runs them, and writes results back. Clients poll
//! `GET /api/jobs/:id`.
//!
//! Concurrency model: one worker, one job at a time. SQLite + Chromium do
//! not benefit from parallel workers on the CX23-class hardware this is
//! designed for; if you need more throughput, bump the PDF semaphore (used
//! inside `convert::run`) instead.

use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use std::sync::Arc;

use crate::batch::{self, BatchRequest, BatchResponse};
use crate::convert::{self, ConvertRequest, ConvertResponse};
use crate::db::{now_seconds, AppState, User};
use crate::error::ConvertError;

const MAX_JOBS_PER_USER_ACTIVE: i64 = 20;

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_CANCELED: &str = "canceled";

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Job {
    pub id: i64,
    pub user_id: i64,
    pub api_key_id: Option<i64>,
    pub kind: String,
    pub status: String,
    #[serde(skip_serializing)]
    pub input_json: String,
    pub result_json: Option<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct JobView {
    pub id: i64,
    pub kind: String,
    pub status: String,
    pub error_message: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

impl From<&Job> for JobView {
    fn from(j: &Job) -> Self {
        Self {
            id: j.id,
            kind: j.kind.clone(),
            status: j.status.clone(),
            error_message: j.error_message.clone(),
            result: j
                .result_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok()),
            created_at: j.created_at,
            started_at: j.started_at,
            finished_at: j.finished_at,
        }
    }
}

pub async fn enqueue_convert(
    pool: &SqlitePool,
    user: &User,
    api_key_id: Option<i64>,
    request: &ConvertRequest,
) -> Result<Job, ConvertError> {
    enforce_active_cap(pool, user).await?;
    let input = serde_json::to_string(request)
        .map_err(|e| ConvertError::Internal(format!("serialize job input: {e}")))?;
    insert(pool, user, api_key_id, "convert", &input).await
}

pub async fn enqueue_batch(
    pool: &SqlitePool,
    user: &User,
    api_key_id: Option<i64>,
    request: &BatchRequest,
) -> Result<Job, ConvertError> {
    enforce_active_cap(pool, user).await?;
    let input = serde_json::to_string(request)
        .map_err(|e| ConvertError::Internal(format!("serialize job input: {e}")))?;
    insert(pool, user, api_key_id, "batch", &input).await
}

async fn enforce_active_cap(pool: &SqlitePool, user: &User) -> Result<(), ConvertError> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM jobs WHERE user_id = ?1 AND status IN ('queued', 'running')",
    )
    .bind(user.id)
    .fetch_one(pool)
    .await?;
    if count.0 >= MAX_JOBS_PER_USER_ACTIVE {
        return Err(ConvertError::Conflict(format!(
            "too many active jobs ({MAX_JOBS_PER_USER_ACTIVE}); wait for some to finish"
        )));
    }
    Ok(())
}

async fn insert(
    pool: &SqlitePool,
    user: &User,
    api_key_id: Option<i64>,
    kind: &str,
    input_json: &str,
) -> Result<Job, ConvertError> {
    let now = now_seconds();
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO jobs (user_id, api_key_id, kind, status, input_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
    )
    .bind(user.id)
    .bind(api_key_id)
    .bind(kind)
    .bind(STATUS_QUEUED)
    .bind(input_json)
    .bind(now)
    .fetch_one(pool)
    .await?;
    get_raw(pool, row.0).await
}

pub async fn list(pool: &SqlitePool, user: &User) -> Result<Vec<JobView>, ConvertError> {
    let rows: Vec<Job> = sqlx::query_as(
        "SELECT id, user_id, api_key_id, kind, status, input_json, result_json, \
                error_message, created_at, started_at, finished_at \
         FROM jobs WHERE user_id = ?1 ORDER BY created_at DESC LIMIT 100",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(JobView::from).collect())
}

pub async fn get(pool: &SqlitePool, user: &User, id: i64) -> Result<JobView, ConvertError> {
    let row: Option<Job> = sqlx::query_as(
        "SELECT id, user_id, api_key_id, kind, status, input_json, result_json, \
                error_message, created_at, started_at, finished_at \
         FROM jobs WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(pool)
    .await?;
    row.map(|j| JobView::from(&j)).ok_or(ConvertError::NotFound)
}

/// Whether a status is a sink that the SSE stream should close on.
pub fn is_terminal(status: &str) -> bool {
    matches!(status, STATUS_DONE | STATUS_FAILED | STATUS_CANCELED)
}

pub async fn cancel(pool: &SqlitePool, user: &User, id: i64) -> Result<(), ConvertError> {
    let res = sqlx::query(
        "UPDATE jobs SET status = ?1, finished_at = ?2 \
         WHERE id = ?3 AND user_id = ?4 AND status = ?5",
    )
    .bind(STATUS_CANCELED)
    .bind(now_seconds())
    .bind(id)
    .bind(user.id)
    .bind(STATUS_QUEUED)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ConvertError::Conflict(
            "job is no longer queued and can't be canceled".into(),
        ));
    }
    Ok(())
}

async fn get_raw(pool: &SqlitePool, id: i64) -> Result<Job, ConvertError> {
    let row: Option<Job> = sqlx::query_as(
        "SELECT id, user_id, api_key_id, kind, status, input_json, result_json, \
                error_message, created_at, started_at, finished_at \
         FROM jobs WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ConvertError::NotFound)
}

/// Background loop: pick the oldest queued job, run it, write the result.
/// Sleeps between polls when the queue is empty. Designed to be spawned
/// once at startup; cooperates with shutdown via `tokio::select!` callers
/// or simply by termination of the runtime.
pub async fn run_worker(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        match claim_next(&state.pool).await {
            Ok(Some(job)) => {
                if let Err(e) = process(&state, &job).await {
                    let _ = mark_failed(&state.pool, job.id, &e.to_string()).await;
                }
            }
            Ok(None) => {} // queue empty, wait for next tick
            Err(e) => {
                tracing::error!(?e, "job worker: claim error");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

async fn claim_next(pool: &SqlitePool) -> Result<Option<Job>, ConvertError> {
    let mut tx = pool.begin().await?;
    let row: Option<Job> = sqlx::query_as(
        "SELECT id, user_id, api_key_id, kind, status, input_json, result_json, \
                error_message, created_at, started_at, finished_at \
         FROM jobs WHERE status = ?1 ORDER BY id ASC LIMIT 1",
    )
    .bind(STATUS_QUEUED)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(job) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    let now = now_seconds();
    sqlx::query("UPDATE jobs SET status = ?1, started_at = ?2 WHERE id = ?3 AND status = ?4")
        .bind(STATUS_RUNNING)
        .bind(now)
        .bind(job.id)
        .bind(STATUS_QUEUED)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(Job {
        status: STATUS_RUNNING.into(),
        started_at: Some(now),
        ..job
    }))
}

async fn process(state: &AppState, job: &Job) -> Result<(), ConvertError> {
    let user = load_user(&state.pool, job.user_id).await?;
    match job.kind.as_str() {
        "convert" => {
            let req: ConvertRequest = serde_json::from_str(&job.input_json)
                .map_err(|e| ConvertError::Internal(format!("decode job input: {e}")))?;
            let res: ConvertResponse = convert::run(&req, state, Some(&user)).await?;
            let result_json = serde_json::to_string(&res)
                .map_err(|e| ConvertError::Internal(format!("encode result: {e}")))?;
            mark_done(&state.pool, job.id, &result_json).await
        }
        "batch" => {
            let req: BatchRequest = serde_json::from_str(&job.input_json)
                .map_err(|e| ConvertError::Internal(format!("decode job input: {e}")))?;
            batch::validate(&req)?;
            let res: BatchResponse = batch::run(&req, state, &user).await?;
            let result_json = serde_json::to_string(&res)
                .map_err(|e| ConvertError::Internal(format!("encode result: {e}")))?;
            mark_done(&state.pool, job.id, &result_json).await
        }
        other => Err(ConvertError::Internal(format!("unknown job kind: {other}"))),
    }
}

async fn load_user(pool: &SqlitePool, id: i64) -> Result<User, ConvertError> {
    let user: Option<User> = sqlx::query_as(
        "SELECT id, email, password_hash, password_salt, role, created_at \
         FROM users WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    user.ok_or(ConvertError::NotFound)
}

async fn mark_done(pool: &SqlitePool, id: i64, result_json: &str) -> Result<(), ConvertError> {
    sqlx::query(
        "UPDATE jobs SET status = ?1, result_json = ?2, finished_at = ?3 WHERE id = ?4",
    )
    .bind(STATUS_DONE)
    .bind(result_json)
    .bind(now_seconds())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_failed(pool: &SqlitePool, id: i64, error: &str) -> Result<(), ConvertError> {
    sqlx::query(
        "UPDATE jobs SET status = ?1, error_message = ?2, finished_at = ?3 WHERE id = ?4",
    )
    .bind(STATUS_FAILED)
    .bind(error)
    .bind(now_seconds())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
