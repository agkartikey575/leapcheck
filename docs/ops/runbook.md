# LeapCheck operations runbook

Status: v2 (2026-11-16)
Related: `docs/07-interfaces.md` (CLI and JSON contract), `.github/workflows/ci.yml`, `.github/workflows/scan.yml`, `lists/README.md` (slug lists), `docs/qa/test-plan.md`.

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

Run a nightly-style scan in GitHub: Actions → "Nightly scan" → "Run workflow" (inputs: shard count 1–20, space-separated slug lists, `force_full` yes/no, deploy yes/no). See section 2 for how a run decides what to scan.

## 2. Nightly scan: incremental and full

The nightly workflow (`.github/workflows/scan.yml`) has four jobs: **plan → build → scan (N shards) → publish**. The published site lives on the `site` branch (one generated commit, force-pushed each run); the host deploys that branch. No job uses any secret.

### How incremental works
1. **plan** reads the lists (`lists/top-plugins.json` and, if present, `lists/top-themes.json`; format in `lists/README.md`), fetches the `site` branch read-only and lists the files under `v1/`.
2. For every entry without a pinned version it asks the official WordPress.org API for the latest version: batches of 250 from `query_plugins` / `query_themes` (browse = popular, fields trimmed to what we need), then one trimmed `plugin_information` / `theme_information` call per slug not found in the batches. A 404 (closed or removed) means the entry is not scanned (counted as `unavailable`).
3. An entry is **skipped** when `v1/wordpress-{plugin|theme}/{slug}/{version}.json` already exists on the `site` branch. Everything else goes into `todo.json` (artifact `scan-plan`) and is split across the shards (never more shards than entries; no shards and no publish when nothing is new).
4. If the API cannot be reached, an entry that already has published results is left for the next run (`unresolved_deferred`); a never-scanned entry is scanned and `scan-wp` resolves the version itself. After 5 failed lookups in a row the plan stops calling the API.
5. **scan** shards stop starting new scans after `SCAN_BUDGET_MIN` (290 min), so export and upload always finish before the 330-minute job limit. Entries not started are counted as `deferred` and are picked up by the next run automatically (they are still missing from the site).
6. **publish** starts from the previous site's `v1/` tree (only `v1/`, never its HTML), overlays the new shard JSON, and merges each package's `index.json` (union of the published and new versions, `latest` = highest version), so earlier results and version history are kept. It writes `v1/_meta.json`, validates that `public/` holds only regular `.json` files under `v1/` (no HTML, scripts or symlinks), and only then runs `leapcheck site` to generate the HTML. Only our engine writes HTML.
7. Publish runs even if some shards failed: a failed shard only means its entries keep their previous results and are retried next run. The workflow run still shows red. If no shard produced any output, nothing is published.

### Full rescan
Everything in the lists is rescanned (published versions are overwritten; older versions stay) when:
- the run is started manually with **`force_full` = true** (Actions → "Nightly scan" → "Run workflow" → tick `force_full`), e.g. after a rules or analyzer change you want reflected right away;
- there is no `site` branch yet, or its `v1/_meta.json` has no `engine_sha`;
- anything under `engine/` (except `engine/tests/` and `*.md`) or `tools/php/composer.lock` changed since the commit in `engine_sha` (git diff in the plan job; the run annotations show the mode and reason, the plan log lists the changed files).

`v1/_meta.json` (public, part of the site):

| Field | Meaning |
|---|---|
| `engine_sha` | Commit whose engine produced the published latest results. A full rescan that did not finish (a shard failed or entries were deferred) keeps the previous value, so the next run is full again. |
| `build_sha`, `run_id`, `generated_at` | The commit and workflow run that built this site, and when (UTC). |
| `mode`, `mode_reason`, `complete` | `full` or `incremental`, why, and whether every shard succeeded with nothing deferred. |
| `lists`, `counts` | Lists used; `list_entries`, `invalid`, `unavailable`, `unresolved_deferred`, `skipped_unchanged`, `planned`, `scanned_ok`, `scan_failed`, `deferred`, and the totals on the site (`plugins`, `themes`, `versions`). |

Check what the live site was built from: `git fetch origin site && git show FETCH_HEAD:v1/_meta.json | jq .`

Notes:
- A forced full rescan that does not finish is not repeated automatically when the engine did not change; start it again.
- Results of versions that are no longer the latest keep the engine version they were scanned with (`engine_version` in each JSON). They are not rescanned.
- Entries removed from a list keep their published results.
- An entry that fails every night is retried every night (it is never published, so never skipped).

## 3. Read traces

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
All artifacts are on the workflow run page (Actions → "Nightly scan" → the run → Artifacts):

| Artifact | Kept | Contents |
|---|---|---|
| `scan-plan` | 14 days | `summary.json` (mode, reason, counts), `todo.json` (what was scanned), `mode.json`, the resolved versions (`found-plugin.tsv`, `found-theme.tsv`), the previous site's file list and `_meta.json` |
| `stats-shard-N` | 14 days | `{ok, failed, deferred}` for shard N |
| `traces-shard-N` | 14 days | SQLite + logs (below) |
| `public-shard-N` | 3 days | the JSON that shard N exported |
| `leapcheck-bin` | 1 day | the binary used by the run |

Each `traces-shard-N` contains:
- `data/shard-N.sqlite`: that shard's own database (inspect it with `leapcheck runs --db data/shard-N.sqlite`),
- `logs/shard-N.jsonl`: stderr logs,
- `logs/health-shard-N.json`: `leapcheck health` output,
- `logs/failed-runs-shard-N.txt`: `leapcheck runs --failed` output.

```bash
gh run download <run-id> -p 'traces-shard-*' -D traces/
leapcheck runs --db traces/traces-shard-0/data/shard-0.sqlite --failed
leapcheck health --db traces/traces-shard-0/data/shard-0.sqlite
gh run download <run-id> -n scan-plan -D plan/ && jq . plan/summary.json
```

Morning check: open the latest "Nightly scan" run, read each shard's `health-shard-N.json`; any failure rate above 5% (UNVERIFIED threshold) gets an issue.

### When the nightly fails
| Symptom | Likely cause | Action |
|---|---|---|
| `plan` failed at "Read the previous site build" | GitHub git unreachable | Re-run. Plan refuses to continue, because without the previous site it could not skip anything. |
| `plan` warns "could not resolve the latest version" | WordPress.org API slow or down | Nothing to do: those entries are retried next run (`unresolved_deferred` in `summary.json`). |
| `build` failed | engine does not compile | Fix on main; CI should have caught it. |
| A shard failed at "Fail shard if most scans failed" | network/WordPress.org outage or an engine bug | Read `logs/shard-N.jsonl`. The other shards are still published; this shard's entries keep their previous results and are retried next run. Re-run failed jobs if it cannot wait. |
| Shard warns "time budget reached" | shard too large | Fine once in a while (the rest is deferred to the next run). If it happens every night, raise the shard count (max 20). |
| A shard timed out (5.5 h) | one scan hung past the budget | Should not happen (budget 290 min + 20 min per scan); check `logs/shard-N.jsonl` for the last slug. |
| `publish` failed at "Start from the previous site's v1/ tree" | `site` branch unreachable | Re-run publish. It refuses to publish without the previous results (that would drop history). |
| `publish` failed at "Overlay" with duplicate files | two shards wrote the same path | Engine export wrote a file outside `/v1/{ecosystem}/{name}/`; fix export or the merge rule. |
| `publish` failed at "Overlay": "no new results from any shard" | every shard failed | See the shard logs; the current site stays live. |
| `publish` failed at "Validate output" | invalid JSON, HTML, symlinks or other files in `public/` before site generation | Engine export bug, or the `site` branch was edited by hand; nothing was published. |
| Plan says `0 of N entries to scan` | nothing new on WordPress.org | Normal; build, scan and publish are skipped. |

Rollback: the `site` branch is force-pushed, so roll back on the host: in the Vercel dashboard open the project → Deployments → pick the previous production deployment → "Instant Rollback" / "Promote to Production". The scan never deletes previous deployments. Then fix the cause; the next run publishes again.

## 4. Secrets

None. The workflows use only the built-in `GITHUB_TOKEN`: read-only everywhere, `contents: write` only in the `publish` job, which pushes the `site` branch. The host deploys that branch through its Git integration (Vercel: production branch `site`, framework preset "Other", no build or install command), so no host token is stored in GitHub.

If a secret is ever added: never print it, never pass it to steps that touch plugin code (plan and scan jobs), never add `pull_request_target` triggers to these workflows.

## 5. Security notes: plugin code is untrusted

Every plugin and theme we scan is third-party code we have not reviewed. Rules:

1. **Never executed, only parsed.** The engine reads files as bytes/text: regex signatures, tree-sitter parsing, and PHP_CodeSniffer/PHPCompatibility, which tokenises PHP without running it. No `php` on plugin files except `phpcs`; never `include`, `require`, `eval`, `composer install` or `npm install` inside a plugin folder; never run a plugin's own scripts, tests or build tools.
2. **Zip-slip protection.** When unpacking, reject any entry whose normalised path is absolute, contains `..`, or resolves outside the extraction folder; never follow or create symlinks from archive entries (skip them). Use the `zip` crate's `enclosed_name()` (or equivalent) and check the result against the target directory.
3. **Size and count limits** (zip-bomb protection). Proposed defaults, to be implemented in the engine and tuned from real data (UNVERIFIED values): max download 100 MB, max total uncompressed size 500 MB, max single file 50 MB, max 50,000 entries, max compression ratio 100:1 per entry. Stop and record the run as `unknown` with the reason when a limit is hit.
4. **Downloads only from WordPress.org over HTTPS** (`downloads.wordpress.org`, `api.wordpress.org`). Slugs and versions are validated (`^[a-z0-9][a-z0-9-]*$`, `^[A-Za-z0-9._-]+$`) before they reach a URL or a shell command; the scan workflow already does this for the slug list.
5. **Timeouts.** Each `scan-wp` call is wrapped in `timeout 20m` in CI; the engine should also time out the phpcs subprocess per plugin.
6. **Isolated, unprivileged runners.** Scans run on throwaway GitHub-hosted runners with a read-only `GITHUB_TOKEN` and no secrets. Workflow scripts never interpolate `${{ }}` expressions; inputs reach the shell only as environment variables, and list paths, slugs and versions are validated before use. Extract into a temp folder and delete it after the run.
7. **Output is data only.** Strings from plugin code (file names, messages, version strings) end up in JSON; the site and WordPress plugin must escape them when rendering. Before generating HTML, the publish job accepts only regular `.json` files under `v1/` (the previous site's `v1/` plus new shard output); anything else, including HTML and symlinks, stops the publish. HTML, the sitemap and host config are then written only by `leapcheck site`, which HTML-escapes plugin-derived text.
8. **Supply chain.** Workflows use only official or widely trusted actions pinned to major versions (`actions/checkout@v4`, `actions/cache@v4`, `actions/upload-artifact@v4`, `actions/download-artifact@v4`, `shivammathur/setup-php@v2`, `dtolnay/rust-toolchain`). `cargo build --locked` and `composer.lock` pin dependencies. Composer runs with `--no-plugins`.
9. **We store results, not plugin code.** Plugin source is GPL and freely redistributable, but we keep only findings to stay small and clear.

## 6. Free-tier limits

Target cost: €0. Figures below are from public documentation as we understand it; re-check before relying on them (**UNVERIFIED** as of 2026-11-02).

### GitHub Actions (public repository, Free plan)
- Standard GitHub-hosted runners: free, no minute quota for public repos.
- Max job run time 6 h (shards stop at 5.5 h via `timeout-minutes: 330`); max workflow run time 35 days.
- Concurrent jobs on the Free plan: 20, so the scan caps shards at 20.
- Matrix: max 256 jobs per workflow run.
- Artifacts: retention set to 1 day (binary), 3 days (JSON), 14 days (traces) to stay tidy.
- Scheduled workflows in public repos are disabled automatically after 60 days without repository activity; re-enable in the Actions tab.
- The repository must stay **public** for free minutes. A private repo would consume the included minutes quickly.

Capacity estimate for the "60k plugins" goal (UNVERIFIED until we measure real scan times): 20 shards × 5.5 h = 110 runner-hours per night. At ~5 s per plugin per target that is ~26k plugin×target scans per hour of total capacity; one full pass of 60k plugins × 3 targets ≈ 180k scans ≈ 50 runner-hours. The nightly scan is now incremental (section 2), so after the first full pass a night only scans versions released since the previous run; a full rescan happens only after an engine change or on request. The version lookup itself costs about one API request per 250 entries plus one per entry not in the popular pages.

### Vercel (Hobby plan)
- **Hobby is for personal, non-commercial use only.** LeapCheck plans paid tiers, so this needs an owner decision: stay on Hobby only while the project is non-commercial, move to a Pro plan (paid), or use a free alternative (section 7). Because the output is plain static files on a git branch, switching hosts needs no workflow change.
- Bandwidth: 100 GB/month on Hobby.
- Deployments: 100 per day; the nightly uses 1 (one push to `site`).
- Files per deployment: Vercel limits the number of files per deployment (in the order of 15,000 on Hobby for CLI uploads; the limit for Git deployments is not confirmed). Tens of thousands of plugin versions will exceed it; options: publish only the latest N versions per plugin, pack results into fewer files, or move the JSON to a host without that limit.

## 7. Fallback host: Cloudflare Pages (steps only, not set up)

Use this if the Vercel Hobby non-commercial rule (or a limit) forces a move. Needs the owner's decision and a Cloudflare account (free plan); nothing is set up today. Limits **UNVERIFIED** (from public docs as of 2026-11; recheck): free plan 500 builds per month (one per push, nightly uses ~31), 20,000 files per site, 25 MiB per file. The 20,000-file limit is the binding one: check `find public -type f | wc -l` (or `counts` in `v1/_meta.json`) before moving.

1. Engine (one-time change in `leapcheck site`): also write a Cloudflare `_headers` file next to `vercel.json` with the same headers (`/v1/*`: `Cache-Control: public, max-age=3600`, `Access-Control-Allow-Origin: *`, `X-Content-Type-Options: nosniff`; `/*`: `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`). Pages serves `/plugins/x/index.html` at `/plugins/x/` without extra config, which matches the trailing-slash URLs. No workflow change is needed for it: the publish validation runs before `leapcheck site`, so files written by the engine are not rejected.
2. Cloudflare dashboard → Workers & Pages → Create → Pages → Connect to Git → authorise the GitHub app for this repository only (read access).
3. Project settings: production branch `site`; framework preset None; build command empty (or `exit 0` if a command is required); build output directory `/`. Disable preview deployments for other branches (Settings → Builds → Branch control: production branch only), so source branches are never deployed.
4. Run "Nightly scan" manually (deploy = yes) or wait for the next run; the push to `site` triggers the Cloudflare build. Check `https://<project>.pages.dev/v1/_meta.json` and a plugin page.
5. Pass `--base-url https://<new-domain>` to `leapcheck site` in `scan.yml` (canonical links and sitemap), and update the URL the WordPress plugin fetches from.
6. Point the custom domain (if any) to Pages, then remove the Vercel project. Exit plan: the same `site` branch works on GitHub Pages too (Settings → Pages → Deploy from branch `site`, root), with no custom headers.

## 8. Change log
- 2026-11-02: v1: CI, nightly sharded scan, Vercel deploy, security notes.
- 2026-11-04: aligned with the real CLI (`--db` on every subcommand, comma-separated `--target`); one shard simulated locally end to end (akismet 5.7.2 compatible, tablepress 1.9.2 breaks: 5 errors, 8 warnings).
- 2026-11-16: v2: incremental nightly scans (skip versions already on the `site` branch; publish overlays the previous `v1/`), `force_full` input and automatic full rescan after engine changes (`v1/_meta.json`), optional theme list, per-shard time budget with deferral, publish survives failed shards, no host secrets (Git deploy of `site`), Cloudflare Pages fallback steps.
