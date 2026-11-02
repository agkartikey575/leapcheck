//! Run tracing: every workflow execution gets a run id; every step is recorded
//! in the database AND written as a structured JSON log line (stderr) carrying the run id.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::json;
use std::time::Instant;

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Structured log line. Format kept OpenTelemetry-friendly (timestamp, severity, body, attributes).
pub fn log(run_id: &str, level: &str, step: &str, msg: &str, extra: serde_json::Value) {
    let line = json!({
        "ts": now(), "level": level, "run_id": run_id, "step": step, "msg": msg, "attrs": extra
    });
    eprintln!("{line}");
}

pub struct Run<'a> {
    pub id: String,
    conn: &'a Connection,
    started: Instant,
}

impl<'a> Run<'a> {
    pub fn start(conn: &'a Connection, kind: &str, input: serde_json::Value) -> Result<Self> {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO run (id, kind, input, engine_version, status, started_at) VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
            params![id, kind, input.to_string(), ENGINE_VERSION, now()],
        )?;
        log(&id, "info", "run", "run started", json!({"kind": kind, "input": input}));
        Ok(Run { id, conn, started: Instant::now() })
    }

    /// Run one named step, recording its timing, status and error.
    pub fn step<T>(&self, name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let started_at = now();
        let t = Instant::now();
        let res = f();
        let ms = t.elapsed().as_millis() as i64;
        let (status, message) = match &res {
            Ok(_) => ("ok", None),
            Err(e) => ("failed", Some(format!("{e:#}"))),
        };
        self.conn.execute(
            "INSERT INTO run_event (run_id, step, status, message, started_at, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![self.id, name, status, message, started_at, ms],
        )?;
        let level = if status == "ok" { "info" } else { "error" };
        log(&self.id, level, name, status, json!({"duration_ms": ms, "error": message}));
        res
    }

    /// Attach a note to the run's trace without timing anything.
    pub fn note(&self, step: &str, message: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO run_event (run_id, step, status, message, started_at, duration_ms) VALUES (?1, ?2, 'ok', ?3, ?4, 0)",
            params![self.id, step, message, now()],
        )?;
        log(&self.id, "info", step, message, json!({}));
        Ok(())
    }

    pub fn set_meta(&self, input_hash: Option<&str>, analyzer_version: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE run SET input_hash = COALESCE(?2, input_hash), analyzer_version = COALESCE(?3, analyzer_version) WHERE id = ?1",
            params![self.id, input_hash, analyzer_version],
        )?;
        Ok(())
    }

    pub fn finish(self, result: &Result<()>) -> Result<()> {
        let ms = self.started.elapsed().as_millis() as i64;
        let (status, err) = match result {
            Ok(_) => ("ok", None),
            Err(e) => ("failed", Some(format!("{e:#}"))),
        };
        self.conn.execute(
            "UPDATE run SET status = ?2, error = ?3, finished_at = ?4, duration_ms = ?5 WHERE id = ?1",
            params![self.id, status, err, now(), ms],
        )?;
        log(&self.id, if status == "ok" { "info" } else { "error" }, "run", "run finished",
            json!({"status": status, "duration_ms": ms, "error": err}));
        Ok(())
    }
}
