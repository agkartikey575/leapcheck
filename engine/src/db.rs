//! SQLite storage: product data (packages, findings, components) and run tracing.

use anyhow::Result;
use rusqlite::Connection;

pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS run (
            id               TEXT PRIMARY KEY,
            kind             TEXT NOT NULL,          -- scan_package | scan_dir | export
            input            TEXT NOT NULL,          -- what was scanned (json)
            input_hash       TEXT,
            engine_version   TEXT NOT NULL,
            analyzer_version TEXT,
            status           TEXT NOT NULL,          -- running | ok | failed
            error            TEXT,
            started_at       TEXT NOT NULL,
            finished_at      TEXT,
            duration_ms      INTEGER
        );

        CREATE TABLE IF NOT EXISTS run_event (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id      TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
            step        TEXT NOT NULL,
            status      TEXT NOT NULL,               -- ok | failed
            message     TEXT,
            started_at  TEXT NOT NULL,
            duration_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS run_event_run ON run_event(run_id);

        CREATE TABLE IF NOT EXISTS package_version (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            ecosystem  TEXT NOT NULL,                -- wordpress-plugin, wordpress-theme, composer, npm, pypi
            name       TEXT NOT NULL,
            version    TEXT NOT NULL,
            UNIQUE (ecosystem, name, version)
        );

        CREATE TABLE IF NOT EXISTS result (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            package_version_id INTEGER NOT NULL REFERENCES package_version(id) ON DELETE CASCADE,
            target             TEXT NOT NULL,        -- e.g. php:8.4
            run_id             TEXT NOT NULL REFERENCES run(id),
            verdict            TEXT NOT NULL,        -- compatible | issues | breaks | unknown
            errors             INTEGER NOT NULL,
            warnings           INTEGER NOT NULL,
            scanned_at         TEXT NOT NULL,
            UNIQUE (package_version_id, target)
        );

        CREATE TABLE IF NOT EXISTS finding (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            result_id  INTEGER NOT NULL REFERENCES result(id) ON DELETE CASCADE,
            rule_id    TEXT NOT NULL,
            severity   TEXT NOT NULL,
            confidence TEXT NOT NULL,
            message    TEXT NOT NULL,
            file       TEXT NOT NULL,
            line       INTEGER NOT NULL,
            component  TEXT,
            analyzer   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS finding_result ON finding(result_id);

        CREATE TABLE IF NOT EXISTS component (
            id                    INTEGER PRIMARY KEY AUTOINCREMENT,
            package_version_id    INTEGER NOT NULL REFERENCES package_version(id) ON DELETE CASCADE,
            name                  TEXT NOT NULL,
            version               TEXT,
            kind                  TEXT NOT NULL,
            path                  TEXT NOT NULL,
            detection             TEXT NOT NULL,
            known_vulnerabilities INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS requirement (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            package_version_id INTEGER NOT NULL REFERENCES package_version(id) ON DELETE CASCADE,
            name               TEXT NOT NULL,
            evidence_file      TEXT NOT NULL,
            evidence_line      INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(conn)
}
