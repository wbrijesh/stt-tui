use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;

use crate::config::Config;

// gpt-4o-mini-transcribe: $0.003 per minute of audio
const COST_PER_MINUTE_USD: f64 = 0.003;

#[derive(Debug, Clone)]
pub struct Transcript {
    pub id: i64,
    pub text: String,
    pub duration_ms: u64,
    pub created_at: i64,
    pub cost_usd: f64,
}

impl Transcript {
    pub fn new(text: String, duration_ms: u64) -> Self {
        let cost_usd = (duration_ms as f64 / 60_000.0) * COST_PER_MINUTE_USD;
        Self {
            id: 0,
            text,
            duration_ms,
            created_at: chrono::Utc::now().timestamp(),
            cost_usd,
        }
    }

    pub fn duration_display(&self) -> String {
        let secs = self.duration_ms / 1000;
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {}s", secs / 60, secs % 60)
        }
    }

    pub fn relative_time(&self) -> String {
        let now = chrono::Utc::now().timestamp();
        let diff = now - self.created_at;

        if diff < 0 {
            return "just now".to_string();
        }

        let diff = diff as u64;
        match diff {
            0..=59 => "just now".to_string(),
            60..=119 => "1 minute ago".to_string(),
            120..=3599 => format!("{} minutes ago", diff / 60),
            3600..=7199 => "1 hour ago".to_string(),
            7200..=86399 => format!("{} hours ago", diff / 3600),
            86400..=172799 => "yesterday".to_string(),
            _ => format!("{} days ago", diff / 86400),
        }
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open() -> Result<Self> {
        let dir = Config::config_dir()?;
        fs::create_dir_all(&dir)?;
        let db_path = dir.join("stt-tui.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transcripts (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                text        TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                created_at  INTEGER NOT NULL,
                cost_usd    REAL NOT NULL,
                deleted     INTEGER NOT NULL DEFAULT 0
            );",
        )
        .context("Failed to create transcripts table")?;

        // Migration: add deleted column if missing (existing databases)
        let has_deleted: bool = conn
            .prepare("SELECT deleted FROM transcripts LIMIT 0")
            .is_ok();
        if !has_deleted {
            conn.execute_batch("ALTER TABLE transcripts ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;")
                .ok();
        }

        Ok(Self { conn })
    }

    pub fn insert(&self, transcript: &Transcript) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO transcripts (text, duration_ms, created_at, cost_usd, deleted) VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![
                transcript.text,
                transcript.duration_ms as i64,
                transcript.created_at,
                transcript.cost_usd,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Returns only non-deleted transcripts for display
    pub fn active_transcripts(&self) -> Result<Vec<Transcript>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, text, duration_ms, created_at, cost_usd FROM transcripts WHERE deleted = 0 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Transcript {
                id: row.get(0)?,
                text: row.get(1)?,
                duration_ms: row.get::<_, i64>(2)? as u64,
                created_at: row.get(3)?,
                cost_usd: row.get(4)?,
            })
        })?;
        let mut transcripts = Vec::new();
        for row in rows {
            transcripts.push(row?);
        }
        Ok(transcripts)
    }

    pub fn soft_delete(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE transcripts SET deleted = 1 WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub fn soft_delete_all(&self) -> Result<()> {
        self.conn
            .execute("UPDATE transcripts SET deleted = 1 WHERE deleted = 0", [])?;
        Ok(())
    }

    pub fn total_cost(&self) -> Result<f64> {
        let cost: f64 = self
            .conn
            .query_row("SELECT COALESCE(SUM(cost_usd), 0.0) FROM transcripts", [], |row| {
                row.get(0)
            })?;
        Ok(cost)
    }

    /// Stats count ALL transcripts (including soft-deleted) for accurate usage tracking
    pub fn stats_since(&self, since: i64) -> Result<PeriodStats> {
        let row = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(duration_ms), 0), COALESCE(SUM(cost_usd), 0.0)
             FROM transcripts WHERE created_at >= ?1",
            rusqlite::params![since],
            |row| {
                Ok(PeriodStats {
                    count: row.get::<_, i64>(0)? as u64,
                    duration_ms: row.get::<_, i64>(1)? as u64,
                    cost_usd: row.get(2)?,
                })
            },
        )?;
        Ok(row)
    }

    pub fn stats_all(&self) -> Result<PeriodStats> {
        let row = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(duration_ms), 0), COALESCE(SUM(cost_usd), 0.0)
             FROM transcripts",
            [],
            |row| {
                Ok(PeriodStats {
                    count: row.get::<_, i64>(0)? as u64,
                    duration_ms: row.get::<_, i64>(1)? as u64,
                    cost_usd: row.get(2)?,
                })
            },
        )?;
        Ok(row)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PeriodStats {
    pub count: u64,
    pub duration_ms: u64,
    pub cost_usd: f64,
}

impl PeriodStats {
    pub fn duration_display(&self) -> String {
        let secs = self.duration_ms / 1000;
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UsageStats {
    pub today: PeriodStats,
    pub this_week: PeriodStats,
    pub this_month: PeriodStats,
    pub all_time: PeriodStats,
}

pub fn fetch_usage_stats(db: &Database) -> Result<UsageStats> {
    use chrono::{Datelike, Local, TimeZone};

    let now = Local::now();
    let today_start = Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .unwrap()
        .timestamp();

    let weekday_offset = now.weekday().num_days_from_monday() as i64;
    let week_start = today_start - weekday_offset * 86400;

    let month_start = Local
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .unwrap()
        .timestamp();

    Ok(UsageStats {
        today: db.stats_since(today_start)?,
        this_week: db.stats_since(week_start)?,
        this_month: db.stats_since(month_start)?,
        all_time: db.stats_all()?,
    })
}
