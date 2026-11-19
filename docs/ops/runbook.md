# LeapCheck operations runbook

Status: v3 (2026-11-19)
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

Run a nightly-style scan in GitHub: Actions → "Nightly scan" → "Run workflow" (inputs: shard count 1–20, default 10 like the nightly; space-separated slug lists; `force_full` yes/no; deploy yes/no). See section 2 for how a run decides what to scan.

## 2. Nightly scan: incremental and full

The nightly workflow (`.github/workflows/scan.yml`) has four jobs: **plan → build → scan (N shards) → publish**. The published site lives on the `site` branch (one generated commit, force-pushed each run); the host deploys that branch. No job uses any secret.

### How incremental works
1. **plan** reads the lists (`lists/top-plugins.json` and, if present, `lists/top-themes.json`; format in `lists/README.md`), fetches the `site` branch read-only and lists the files under `v1/`.
2. For every entry without a pinned version it asks the official WordPress.org API for the latest version: batches of 250 from `query_plugins` / `query_themes` (browse = popular, fields trimmed to what we need), then one trimmed `plugin_information` / `theme_information` call per slug not found in the batches. A 404 (closed or removed) means the entry is not scanned (counted as `unavailable`).
   - **Release zips.** The plan also keeps the API's `download_link`. About 12% of popular plugins (583 of the 5,000 on 2026-11-19) are not published as `{slug}.{version}.zip`: their stable tag is `trunk` (link `{slug}.zip`) or the tag is named differently from the version (`wp-health.v2.28.1.zip`, `top-level-cats.release-1.0.1.zip`). `scan-wp` builds `{slug}.{version}.zip` itself and gets a 404 for those. Such entries get a `zip` field in `todo.json`; the scan job downloads that link (HTTPS, `downloads.wordpress.org/{kind}/{slug}…zip` only, 80 MB cap), unpacks it with the same rules as the engine (no absolute paths, no `..`, nothing outside the folder, symlinks skipped, at most 50,000 entries and 400 MB) and runs `leapcheck scan-dir --ecosystem wordpress-{kind}` with the same name and version, so the output path is identical. Counted as `planned_release_zip` in `_meta.json` and `release_zip` in the shard stats. Known gap: these results have no `display_name` / `active_installs` (scan-dir does not call the API), so their pages show the slug as the name. The proper fix is in the engine (Next, item 3); then this fallback can be removed.
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
| `lists`, `counts` | Lists used; `list_entries`, `invalid`, `unavailable`, `unresolved_deferred`, `skipped_unchanged`, `planned`, `planned_release_zip` (planned entries scanned from their release zip, section 2), `scanned_ok`, `scan_failed`, `deferred`, and the totals on the site (`plugins`, `themes`, `versions`). |

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
| Many `scan_failed` with `GET https://downloads.wordpress.org/...{slug}.{version}.zip: status code 404` | the release has no versioned zip (stable tag `trunk` or a differently named tag) and the plan did not get a `download_link` for it (API outage, pinned version) | Fixed for resolved entries by the release-zip path (section 2). A pinned version that 404s does not exist as a zip: remove or correct the pin. |
| `publish` red at "Publish to the site branch": "over the host limit" | the site has more files than `MAX_SITE_FILES` (15,000) | The site branch was still pushed (scan state is kept), but the host may reject the deployment and keep serving the previous one: check the Vercel deployment. Fix: bundle JSON per package (Next, item 2). |

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

### Measured scan time and sizing (2026-11-19)
Evidence (real GitHub-hosted runners, from the `traces-shard-*` SQLite of the full rescan run 44266514978 on 2026-11-18, 999 entries of the top 1,000 in 20 shards): 959 successful scans, mean **13.6 s** per plugin for all three targets (median 4.5 s, p90 33.5 s, p99 143 s, max 432 s for `surecart`), 3.6 runner-hours of scanning in total. Shards of ~50 entries took 8–17 min wall time including ~1 min of setup. Adding the 1 s pause and per-entry overhead gives **~15 s per entry** as the sizing figure. A local cross-check of 15 corpus plugins (same binary, 4 vCPU machine) gave the same picture: small plugins 2–5 s, large ones 30–150 s (table in the change log entry for v3). Themes were not measured in CI; two local theme scans took 3.6 s and 8.3 s (UNVERIFIED as a theme average). The top 1,000 are on average larger than plugins ranked 1,001–5,000, so 15 s is conservative for the new entries.

Sizing for top 5,000 plugins + top 500 themes (5,500 entries):

| Run | Entries | Runner-hours (×15 s) | Per shard at 10 shards | Fits one night? |
|---|---|---|---|---|
| First run after adding the lists (incremental: the ~950 already published are skipped) | ~4,550 | ~19 h | ~1.9 h | yes, budget is 290 min per shard |
| Full rescan (engine change or `force_full`) | 5,500 | ~23 h | ~2.3 h | yes |
| Typical incremental night (new releases, ~5% per night seen on the top 1,000) | ~275 | ~1.2 h | ~7 min | yes |

So the first full coverage of 5,000 + 500 takes **one nightly run** at the default 10 shards (capacity 10 × 290 min ≈ 48 runner-hours per run, about twice what is needed). If a run is slower than measured, the time budget defers the rest and the next night finishes it (at most 2 runs unless scans are 2× slower than measured). 20 shards would halve wall time but leave no Free-plan job slots for CI during the run; not needed at this size. Shards are assigned by slug index modulo N, so they are balanced by count (455–456 entries each in the planned first run); one shard can still draw several of the slowest plugins (max 7 min each), which is far inside the budget.

60k goal (UNVERIFIED extrapolation): 60,000 × 15 s ≈ 250 runner-hours for a full pass, i.e. ~5 nights at 10 shards or ~2.6 nights at 20; incremental nights after that stay small. The host file limit (below) is the binding constraint long before runner time.

The version lookup costs about one API request per 250 entries plus one per entry not in the popular pages (the local dry run for 5,500 entries made 22 page requests, 0 per-slug lookups, in 31 s).

### Vercel (Hobby plan)
- **Hobby is for personal, non-commercial use only.** LeapCheck plans paid tiers, so this needs an owner decision: stay on Hobby only while the project is non-commercial, move to a Pro plan (paid), or use a free alternative (section 7). Because the output is plain static files on a git branch, switching hosts needs no workflow change.
- Bandwidth: 100 GB/month on Hobby (UNVERIFIED, recheck).
- Deployments: 100 per day; the nightly uses 1 (one push to `site`) (UNVERIFIED, recheck).
- **Files per deployment: UNVERIFIED.** The official docs could not be opened from our environment on 2026-11-19 (vercel.com not reachable from the build sandbox). What search results show: a Vercel limits page states "the maximum number of files that can be uploaded when creating a CLI Deployment is 15,000 for source files"; an older copy listed 10,000 source files and 16,000 build output files; a Vercel changelog entry "CLI deployment limits removed" (dated June 2026 by a mirror) may have lifted CLI-specific limits, but it is not clear whether that covers file counts or Git deployments. Our deployment is a Git deployment with no build step, so every file on the `site` branch is a source file. Until someone confirms the figure in the Vercel dashboard or docs, we plan for **15,000** (`MAX_SITE_FILES` in `scan.yml`; the publish step warns above 80% and turns red above it, after pushing).

File count (measured on the `site` branch of run 44504861180: 3,065 files for 960 plugins / 1,100 versions = 972 HTML + 2,061 JSON + a few static files):

| Coverage | HTML | JSON (index + one per version) | Total | Under 15,000? |
|---|---|---|---|---|
| Today (960 plugins) | 972 | 2,061 | 3,065 | yes |
| 5,000 plugins + 500 themes, first run (1.15 versions per package as today; theme pages not rendered yet) | ~5,010 | ~11,800 | **~16,800** | **no** |
| Same, with theme pages (Next, item 1) | ~5,510 | ~11,800 | ~17,300 | no |
| One month later (~5% new versions per night ≈ +275 JSON per night) | ~5,510 | ~20,000 | ~25,500 | no (also over Cloudflare Pages' 20,000) |

So 5,500 packages with one JSON file per version do **not** fit, and per-version files grow every night. Plan (Next, item 2): bundle JSON per package, which makes the count 1 HTML + 1 JSON per package (~11,000 files for 5,500, independent of version history). Until that lands, expect the first runs after the list expansion to exceed the limit; the `site` branch keeps the full results either way, and the live site stays on the last accepted deployment.

## 7. Fallback host: Cloudflare Pages (steps only, not set up)

Use this if the Vercel Hobby non-commercial rule (or a limit) forces a move. Needs the owner's decision and a Cloudflare account (free plan); nothing is set up today. Limits **UNVERIFIED** (from public docs as of 2026-11; recheck): free plan 500 builds per month (one per push, nightly uses ~31), 20,000 files per site, 25 MiB per file. The 20,000-file limit is the binding one: check `find public -type f | wc -l` (or `counts` in `v1/_meta.json`) before moving.

1. Engine (one-time change in `leapcheck site`): also write a Cloudflare `_headers` file next to `vercel.json` with the same headers (`/v1/*`: `Cache-Control: public, max-age=3600`, `Access-Control-Allow-Origin: *`, `X-Content-Type-Options: nosniff`; `/*`: `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`). Pages serves `/plugins/x/index.html` at `/plugins/x/` without extra config, which matches the trailing-slash URLs. No workflow change is needed for it: the publish validation runs before `leapcheck site`, so files written by the engine are not rejected.
2. Cloudflare dashboard → Workers & Pages → Create → Pages → Connect to Git → authorise the GitHub app for this repository only (read access).
3. Project settings: production branch `site`; framework preset None; build command empty (or `exit 0` if a command is required); build output directory `/`. Disable preview deployments for other branches (Settings → Builds → Branch control: production branch only), so source branches are never deployed.
4. Run "Nightly scan" manually (deploy = yes) or wait for the next run; the push to `site` triggers the Cloudflare build. Check `https://<project>.pages.dev/v1/_meta.json` and a plugin page.
5. Pass `--base-url https://<new-domain>` to `leapcheck site` in `scan.yml` (canonical links and sitemap), and update the URL the WordPress plugin fetches from.
6. Point the custom domain (if any) to Pages, then remove the Vercel project. Exit plan: the same `site` branch works on GitHub Pages too (Settings → Pages → Deploy from branch `site`, root), with no custom headers.

## 8. Next

### 1. Theme pages in `leapcheck site` (spec for the engine; `engine/src/site.rs` is not changed by ops)
Today `site::load()` reads only `v1/wordpress-plugin/`, so the 500 themes get JSON under `v1/wordpress-theme/` but no HTML page, no directory entry and no sitemap entry (checked 2026-11-19: a site build from plugin + theme JSON wrote pages only under `plugins/`). Required behaviour:

1. **Load both ecosystems.** `load()` iterates `v1/wordpress-plugin/` and `v1/wordpress-theme/`; `Pkg` gets a `kind` (`plugin` | `theme`). Same parsing, same "latest = last version in sorted `index.json`", same history.
2. **URLs, separate namespace** (a plugin and a theme may share a slug): theme page `themes/{slug}/index.html` → `/themes/{slug}/`; directory `themes/index.html` → `/themes/`. Plugin URLs do not change. Never write a theme page under `plugins/`.
3. **Page content** = the plugin page template with the kind-specific strings:
   - title: `Is {name} theme compatible with PHP 8.4? …` (same four verdict variants), H1 `Is the {name} theme compatible with PHP 8.4?`; description says "theme";
   - breadcrumb `Home › Themes › {name}` (and the JSON-LD `BreadcrumbList` item 2 = `Themes`, `{base}/themes/`);
   - raw data link `/v1/wordpress-theme/{slug}/{version}.json`;
   - JSON-LD `about.url` = `https://wordpress.org/themes/{slug}/`;
   - "What to do next": "Update the {name} theme…", support forum `https://wordpress.org/support/theme/{slug}/`; the "unknown" text says "theme";
   - not-affiliated line: "Not affiliated with the author of the {name} theme."
   - all theme-derived text HTML-escaped exactly like plugin text (`esc()`), JSON-LD `</` escaped.
4. **Indexing gate:** same rule and same `LEAPCHECK_INDEX_PLUGINS=1` switch as plugin pages (noindex until the accuracy gate passes); sitemap gets `/themes/` and indexable theme pages only when the gate is open.
5. **Directory page** `/themes/`: same table as `/plugins/` (Theme, Version, PHP 8.4), sorted by active installs. Header nav gets a "Themes" link after "Plugins".
6. **Libraries page:** a bundling theme links to `/themes/{slug}/`, not `/plugins/{slug}/`.
7. **Home page:** counts read "{p} plugins and {t} themes scanned for PHP 8.4"; the search box may stay plugin-only for now.
8. **Size:** `/plugins/index.html` is 172 KB for 964 rows, so ~0.9 MB at 5,000. Either paginate (500 rows per page, `plugins/page/2/`, … each counts as one file) or list the top 500 and link the rest from `/libraries/` and search. Either way add the page count to the file-count budget (section 6).
9. **Tests:** a fixture with one plugin and one theme of the same slug produces two pages at different URLs; a theme page contains `/v1/wordpress-theme/` and `wordpress.org/themes/`; nothing theme-related is written under `plugins/`; escaping test with a malicious theme name.
No workflow change is needed: publish already merges `v1/wordpress-theme/` and runs `leapcheck site` last.

### 2. Fit the host file limit: one JSON file per package (engine + WordPress plugin + interfaces doc)
Needed before (or right after) the first 5,000 + 500 run, see the file-count table in section 6.
- `leapcheck export` writes `v1/wordpress-{kind}/{slug}.json` = today's `index.json` fields plus `results: {version: <today's per-version document>}` for every version, and stops writing `{slug}/index.json` and `{slug}/{version}.json`. Count: 1 JSON + 1 HTML per package = ~11,000 files for 5,500, growing with packages only, not with versions. Measured per-version JSON today: mean 15.6 KB, median 1.5 KB, max 437 KB, so bundles stay small; if a bundle passes ~1 MB, keep full findings only for the latest 3 versions and verdict + counts for older ones.
- Publish merge (`scan.yml`, ops): replace the `index.json` union with a per-package merge of `results` (new shard wins per version), then the same validation.
- WordPress plugin: fetch `/v1/wordpress-{kind}/{slug}.json` and pick the installed version locally (also one request per package instead of two). Keep serving the old paths during a transition only if the file budget allows; otherwise switch in one release.
- Site generator: read the bundles.
- Contract change: update `docs/07-interfaces.md` first.
- Beyond ~7,000 packages even this passes 15,000 files (2 per package); for the 60k goal the options are a prefix-bundled JSON API (`v1/wordpress-plugin/_by-prefix/{aa}.json`) plus HTML pages only for packages above an install threshold, or a host without a per-deployment file cap (owner decision; GitHub Pages' limits are UNVERIFIED).

### 3. Engine: download from the API's `download_link`
`scan-wp` builds `https://downloads.wordpress.org/{kind}/{slug}.{version}.zip`, which 404s for ~12% of popular plugins (section 2). Fix in `engine/src/main.rs`: read `download_link` in `wp_info()` and use it when the requested version equals the API's version (validate host `downloads.wordpress.org` and path `/{kind}/{slug}…zip`); keep the constructed URL for pinned older versions. Then remove the release-zip path from `scan.yml` (the scan-dir results also lack the display name and install count). An engine change triggers a full rescan (~23 runner-hours, one night).

### 4. Observations to follow up
- Scheduled runs start hours late: cron is 02:17 UTC, but the last runs started at 10:10–10:43 UTC (GitHub delays scheduled workflows under load; cause UNVERIFIED). Harmless for a nightly, but do not schedule anything that assumes the scan finished by a fixed time.
- `actionlint` was not run on v3 of `scan.yml` (YAML parsed and every `run:` script passed `bash -n`); run it in CI.

## 9. Change log
- 2026-11-02: v1: CI, nightly sharded scan, Vercel deploy, security notes.
- 2026-11-04: aligned with the real CLI (`--db` on every subcommand, comma-separated `--target`); one shard simulated locally end to end (akismet 5.7.2 compatible, tablepress 1.9.2 breaks: 5 errors, 8 warnings).
- 2026-11-16: v2: incremental nightly scans (skip versions already on the `site` branch; publish overlays the previous `v1/`), `force_full` input and automatic full rescan after engine changes (`v1/_meta.json`), optional theme list, per-shard time budget with deferral, publish survives failed shards, no host secrets (Git deploy of `site`), Cloudflare Pages fallback steps.
- 2026-11-19: v3: lists grown to the top 5,000 plugins and top 500 themes (WordPress.org API, browse popular). Last run's 40 `scan_failed` were all download 404s of `{slug}.{version}.zip` for releases without a versioned zip (none were closed plugins, timeouts, oversized zips or phpcs errors); the plan now keeps `download_link` and the scan uses it (release-zip path). Measured scan time from CI traces (mean 13.6 s per plugin, 3 targets) and sized the run: one night at 10 shards. Manual-run shard default 4 → 10. File-count guard before deploy (`MAX_SITE_FILES`). Local timings (s, 3 targets): akismet 5.7.2 3.0, classic-editor 1.7.0 1.7, duplicate-post 4.7 2.5, redirection 4.0 2.2, contact-form-7 5.1 2.5, limit-login-attempts-reloaded 3.4.0 4.9, wordpress-seo 28.8 33.7, elementor 4.3.6 148.9, woocommerce 3.5.0 49.4, jetpack 6.5.2 47.9, updraftplus 1.16.0 30.4, mailpoet 5.44.0 54.4, wordfence 7.1.17 12.8, tablepress 1.9.2 7.5, gmail-smtp 1.1.9 11.3 (mean 27.5 s, median 11.3 s; the corpus is weighted towards large plugins). Theme pages, per-package JSON bundles and the engine download fix are specified under Next.
