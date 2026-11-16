# LeapCheck operations runbook

Owner: DevOps & Security Engineer · Status: v1 (2026-11-02)
Related: `docs/07-interfaces.md` (CLI and JSON contract), `.github/workflows/ci.yml`, `.github/workflows/scan.yml`, `docs/qa/test-plan.md`.

## 1. Run a scan locally

Prerequisites: Rust (stable), PHP 8.x CLI with Composer 2, `jq`.

```bash
# 1. Build the engine
cd engine && cargo build --release && cd ..

# 2. Install the PHP analysis tools (PHP_CodeSniffer + PHPCompatibility)
cd tools/php
composer install --no-plugins          # --no-plugins: never run dependency plugins/scripts
vendor/bin/phpcs --config-set installed_paths ../../phpcompatibility/php-compatibility,../../phpcsstandards/phpcsutils
vendor/bin/phpcs -i                    # must list PHPCompatibility and PHPCSUtils
cd ../..

# 3. Scan one plugin version for all phase-1 targets (run from the repo root:
#    the defaults --db data/leapcheck.sqlite, --rules engine/rules, --php-tools tools/php are relative to it)
engine/target/release/leapcheck scan-wp --slug contact-form-7 --version 5.1 --target 8.3,8.4,8.5

# 4. Export the static JSON "API"
engine/target/release/leapcheck export --out public
jq . public/v1/wordpress-plugin/contact-form-7/5.1.json
```

Scan the whole test corpus the way one CI shard does:

```bash
jq -c '.[]' engine/tests/corpus.json | while read -r e; do
  s=$(jq -r .slug <<<"$e"); v=$(jq -r .version <<<"$e")
  engine/target/release/leapcheck scan-wp --db data/local.sqlite --slug "$s" --version "$v" \
    --target 8.3,8.4,8.5 2>>logs.jsonl
done
engine/target/release/leapcheck export --db data/local.sqlite --out public
```

Every subcommand accepts `--db <file>` (default `data/leapcheck.sqlite`), `--rules <dir>` (default `engine/rules`) and `--php-tools <dir>` (default `tools/php`). `--target` is repeatable or comma separated (default `8.4`). `--version` defaults to the latest release; `--kind` defaults to `plugin`. Delete the DB file to start from scratch. `export` prints `{"exported_files":N,"out":...}` and writes nothing when N is 0.

Run a nightly-style scan in GitHub: Actions → "Nightly scan" → "Run workflow" (inputs: shard count 1–20, slug list path, deploy yes/no).

## 2. Read traces

Every scan is a **run** with a `run_id`; each step (`fetch`, `unpack`, `detect_components`, `analyze`, `aggregate`, `publish`) is a `run_event` in SQLite.

```bash
leapcheck runs                     # list the 20 most recent runs (--limit N for more)
leapcheck runs --failed            # only failed runs, with the error
leapcheck runs --id <run-id>       # one run's steps: timings, status, error details
leapcheck health                   # JSON report for the last 24 h: runs, failed, failure_rate, top errors, slowest_steps
leapcheck health --hours 168       # same, for the last week
# all of them take --db <file>
```

Logs are JSON lines on stderr: `{"ts","level","run_id","step","msg","attrs"}`. Find everything about one run:

```bash
jq -c 'select(.run_id == "<run-id>")' logs.jsonl
jq -r 'select(.level == "error") | .msg' logs.jsonl | sort | uniq -c | sort -rn | head   # top errors
```

### Traces from a CI scan
Each shard uploads an artifact `traces-shard-N` (kept 14 days) containing:
- `data/shard-N.sqlite`: that shard's own database (inspect it with `leapcheck runs --db data/shard-N.sqlite`),
- `logs/shard-N.jsonl`: stderr logs,
- `logs/health-shard-N.json`: `leapcheck health` output,
- `logs/failed-runs-shard-N.txt`: `leapcheck runs --failed` output.

```bash
gh run download <run-id> -p 'traces-shard-*' -D traces/
leapcheck runs --db traces/traces-shard-0/data/shard-0.sqlite --failed
leapcheck health --db traces/traces-shard-0/data/shard-0.sqlite
```

Morning check (QA + DevOps): open the latest "Nightly scan" run, read each shard's `health-shard-N.json`; any failure rate above 5% (UNVERIFIED threshold) gets an issue.

### When the nightly fails
| Symptom | Likely cause | Action |
|---|---|---|
| `build` failed | engine does not compile | Fix on main; CI should have caught it. |
| A shard failed at "Fail shard if most scans failed" | network/WordPress.org outage or an engine bug | Read `logs/shard-N.jsonl`; re-run failed jobs. The site is not deployed, so the previous version stays live. |
| A shard timed out (5.5 h) | shard too large | Raise the shard count (max 20). |
| `publish` failed at "Merge" with duplicate files | two shards wrote the same path | Engine export wrote a file outside `/v1/{ecosystem}/{name}/`; fix export or the merge rule. |
| `publish` failed at "Validate output" | invalid JSON or non-JSON files in `public/` | Engine export bug; nothing was deployed. |
| Deploy skipped with "Vercel secrets not set" | secrets missing | Add them (section 3). |

Rollback: in the Vercel dashboard, promote the previous production deployment (or `vercel rollback` with the token). The scan never deletes the previous deployment.

## 3. Secrets

Set in GitHub: repository → Settings → Secrets and variables → Actions → New repository secret. They are used **only** by the `publish` job of `scan.yml`; CI and scan shards get no secrets.

| Secret | What | Where to get it |
|---|---|---|
| `VERCEL_TOKEN` | Vercel access token used by the CLI | Vercel → Account Settings → Tokens. Scope it to the LeapCheck team only; set an expiry (e.g. 90 days) and a calendar reminder to rotate. |
| `VERCEL_ORG_ID` | Vercel team (or personal account) id | Run `vercel link` once locally in an empty folder; read `orgId` from `.vercel/project.json`. |
| `VERCEL_PROJECT_ID` | Vercel project id | Same file, `projectId`. |

Vercel project settings for a static upload: Framework preset "Other", no build command, no install command. The workflow runs `vercel deploy public --prod --yes` with the CLI pinned to major version 65.

Never print secrets, never pass them to steps that touch plugin code, never add `pull_request_target` triggers to these workflows.

## 4. Security notes: plugin code is untrusted

Every plugin and theme we scan is third-party code we have not reviewed. Rules:

1. **Never executed, only parsed.** The engine reads files as bytes/text: regex signatures, tree-sitter parsing, and PHP_CodeSniffer/PHPCompatibility, which tokenises PHP without running it. No `php` on plugin files except `phpcs`; never `include`, `require`, `eval`, `composer install` or `npm install` inside a plugin folder; never run a plugin's own scripts, tests or build tools.
2. **Zip-slip protection.** When unpacking, reject any entry whose normalised path is absolute, contains `..`, or resolves outside the extraction folder; never follow or create symlinks from archive entries (skip them). Use the `zip` crate's `enclosed_name()` (or equivalent) and check the result against the target directory.
3. **Size and count limits** (zip-bomb protection). Proposed defaults, to be implemented in the engine and tuned from real data (UNVERIFIED values): max download 100 MB, max total uncompressed size 500 MB, max single file 50 MB, max 50,000 entries, max compression ratio 100:1 per entry. Stop and record the run as `unknown` with the reason when a limit is hit.
4. **Downloads only from WordPress.org over HTTPS** (`downloads.wordpress.org`, `api.wordpress.org`). Slugs and versions are validated (`^[a-z0-9][a-z0-9-]*$`, `^[A-Za-z0-9._-]+$`) before they reach a URL or a shell command; the scan workflow already does this for the slug list.
5. **Timeouts.** Each `scan-wp` call is wrapped in `timeout 20m` in CI; the engine should also time out the phpcs subprocess per plugin.
6. **Isolated, unprivileged runners.** Scans run on throwaway GitHub-hosted runners with a read-only `GITHUB_TOKEN` and no secrets. Extract into a temp folder and delete it after the run.
7. **Output is data only.** Strings from plugin code (file names, messages, version strings) end up in JSON; the site and WordPress plugin must escape them when rendering. The deploy job refuses to publish any non-JSON file. (When the engine starts generating the static HTML site, update that check in `scan.yml` and make sure all plugin-derived text is HTML-escaped.)
8. **Supply chain.** Workflows use only official or widely trusted actions pinned to major versions (`actions/checkout@v4`, `actions/cache@v4`, `actions/upload-artifact@v4`, `actions/download-artifact@v4`, `shivammathur/setup-php@v2`, `dtolnay/rust-toolchain`). `cargo build --locked` and `composer.lock` pin dependencies. Composer runs with `--no-plugins`.
9. **We store results, not plugin code.** Plugin source is GPL and freely redistributable, but we keep only findings to stay small and clear.

## 5. Free-tier limits

Target cost: €0. Figures below are from public documentation as we understand it; re-check before relying on them (**UNVERIFIED** as of 2026-11-02).

### GitHub Actions (public repository, Free plan)
- Standard GitHub-hosted runners: free, no minute quota for public repos.
- Max job run time 6 h (shards stop at 5.5 h via `timeout-minutes: 330`); max workflow run time 35 days.
- Concurrent jobs on the Free plan: 20, so the scan caps shards at 20.
- Matrix: max 256 jobs per workflow run.
- Artifacts: retention set to 1 day (binary), 3 days (JSON), 14 days (traces) to stay tidy.
- Scheduled workflows in public repos are disabled automatically after 60 days without repository activity; re-enable in the Actions tab.
- The repository must stay **public** for free minutes. A private repo would consume the included minutes quickly.

Capacity estimate for the "60k plugins" goal (UNVERIFIED until we measure real scan times): 20 shards × 5.5 h = 110 runner-hours per night. At ~5 s per plugin per target that is ~26k plugin×target scans per hour of total capacity; one full pass of 60k plugins × 3 targets ≈ 180k scans ≈ 50 runner-hours. Better: scan only plugins whose version changed since the last night (incremental), plus a weekly full rescan.

### Vercel (Hobby plan)
- **Hobby is for personal, non-commercial use only.** LeapCheck plans paid tiers, so this needs an owner decision: stay on Hobby only while the project is non-commercial, move to a Pro plan (paid), or use the free alternatives listed in `docs/06-architecture.md` (GitHub Pages or Cloudflare Pages). Because the output is plain static files, switching hosts is a one-step change to the deploy job.
- Bandwidth: 100 GB/month on Hobby.
- Deployments: 100 per day; the nightly uses 1.
- Files per deployment: there is a limit on the number of files uploaded by the CLI (in the order of 15,000 on Hobby). Tens of thousands of plugin versions will exceed it; options: deploy only the latest N versions per plugin, pack results into fewer files, or move the JSON to a host without that limit.

## 6. Change log
- 2026-11-02: v1: CI, nightly sharded scan, Vercel deploy, security notes.
- 2026-11-04: aligned with the real CLI (`--db` on every subcommand, comma-separated `--target`); one shard simulated locally end to end (akismet 5.7.2 compatible, tablepress 1.9.2 breaks: 5 errors, 8 warnings).
