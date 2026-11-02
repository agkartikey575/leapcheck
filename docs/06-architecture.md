# Architecture v1: generic core, language plug-ins, everything traced

## Principle
One generic core. Each language or ecosystem is a small **analyzer plug-in** that follows one contract:
`input: a folder of code + target runtime version → output: findings JSON in our standard format`.
Adding Node.js or Python later means adding one analyzer, with no changes to the core.

## Components (all on one small server, Docker Compose)
| Part | What it does | Tech |
|---|---|---|
| **API** | Receives "check my site" requests; serves results | TypeScript (Node.js) |
| **Worker** | Runs scan jobs: download → unpack → find hidden libraries → run analyzer → save | TypeScript, calls analyzers as subprocesses |
| **Analyzers** | PHP first (PHPCompatibility + our rules); Node.js and Python later | Each in its own language, run in containers |
| **Database** | Everything: packages, versions, findings, hidden components, runs, logs | PostgreSQL (open source) |
| **Website** | Public "Is X compatible with PHP 8.4?" pages + admin "Runs" page | Static pages generated from the DB |
| **WordPress plugin** | The free tool for site owners | PHP (required by WordPress) |

Hosting: one VPS (~€5–10/month), with nightly database backups to object storage. No other paid services.

## Generic data model (core tables)
- `ecosystem` (php, npm, pypi, …)
- `package`, `package_version`
- `runtime_target` (php 8.4, node 24, python 3.13, …)
- `component`: hidden/bundled libraries found inside a package version (name, version, path, detection method)
- `finding`: rule_id, severity, confidence, file, line, evidence snippet, fix hint
- `run`: one execution of a workflow (id, type, input hash, analyzer version, rules version, status, timings)
- `run_event`: each step of a run (step name, start, end, status, message, error details)

## Tracing and logs: how every run is recorded
1. Every workflow execution (scan a package, scan a site, nightly refresh) gets a **run_id**.
2. Each step writes a **run_event** row: `fetch`, `unpack`, `detect_components`, `analyze`, `aggregate`, `publish`, each with start/end time, status and error.
3. All log lines are **structured JSON with run_id**, so one id finds everything about a run.
4. Each run stores **analyzer version + rules version + input hash**, so any result can be re-run and explained ("why do you say this breaks?" → file, line, rule, evidence).
5. Run and event records are kept 90 days. Findings are kept permanently, since they are the product.
6. The log format follows the OpenTelemetry standard, so a dashboard tool (e.g. Grafana) can be added later without code changes. Not needed for v1.

## Analysis of runs
- An **admin "Runs" page** lists runs and their status, slowest steps, top errors, and lets you open any run to see its full trace.
- A **daily health report**, auto-generated from the DB: runs, failure rate, errors grouped by cause, slowest packages, new findings. The QA and DevOps agents read it and open fixes.
- **Alerts** when the failure rate spikes or the nightly refresh doesn't finish. Sent by email or a chat message; channel to be decided.

## Privacy
Plugin data is opt-in and contains only plugin slugs and versions, no site content. Site identifiers are hashed. 30-day retention for site checks.
