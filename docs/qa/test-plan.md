# LeapCheck test plan (phase 1: WordPress / PHP)

Owner: QA & Test Automation Lead · Status: draft v1 (2026-11-02) · Scope: the `leapcheck` engine, its static JSON output and the release process.

Mission: nothing wrong reaches a user. A wrong "compatible" verdict is the worst defect we can ship: a site owner upgrades PHP and their site breaks. A wrong "breaks" verdict costs trust and support time. Both count as escaped defects.

## 1. Test corpus: `engine/tests/corpus.json`

40 real WordPress.org plugin versions, format `[{"slug","version","kind","why"}]`:

| Group | Count | Purpose |
|---|---|---|
| Popular, latest version | 20 | Baseline: the plugins most users have. Expect mostly `compatible`/`issues`; any `breaks` gets a manual review before release. |
| Old versions (2018–2020 era) | 10 | Likely to break on PHP 8 (removed functions, old syntax). Several pair with the latest release of the same plugin (contact-form-7, woocommerce, jetpack, wordfence, updraftplus, redirection), so the golden diff shows real change over time. |
| Bundled libraries | 10 | Hidden-library detection: PHPMailer (gmail-smtp 1.1.9), TCPDF (tickera), PHPExcel vs PhpSpreadsheet (woo-order-export-lite 2.1.0 / 4.1.4), Freemius (post-smtp, wp-sheet-editor), Action Scheduler (mailpoet, email-subscribers), dompdf incl. Strauss-prefixed copies (two invoice plugins). |

Verification done on 2026-11-02:
- Every `slug`/`version` pair was checked against `https://api.wordpress.org/plugins/info/1.2/?action=plugin_information&request[slug]=SLUG&request[fields][versions]=1`: all 40 are present in the `versions` map. No zips were downloaded.
- The bundled-library claims marked "verified in SVN" were checked by listing directories on `plugins.svn.wordpress.org` (not by downloading code).
- Release years of old versions and "likely breaks on PHP 8" are **UNVERIFIED** until the first engine run plus manual review (section 3). The corpus states this in each `why`.

Corpus rules:
- Pin exact versions, never "latest", so golden files stay stable. Refresh the "popular" versions on purpose (quarterly), as a reviewed change.
- Add a corpus entry for every escaped defect (regression test), with the bug reference in `why`.
- Grow toward ~100 entries in phase 1, adding themes (`"kind":"theme"`), ionCube-encoded plugins (expected `unknown`), and plugins with minified JS libraries.
- Plugin code is GPL and downloaded from WordPress.org at test time. We store only our findings (golden JSON), never the plugin source, in the repo.

## 2. Golden-file tests

Idea: every engine change is checked against known answers from real plugin versions.

### Layout
```
engine/tests/corpus.json                                   # inputs (pinned slug + version)
engine/tests/golden/{ecosystem}/{name}/{version}.json      # expected output, same schema as /v1/... files
```
A golden file is the `export` output for that version, normalised:
- Remove volatile fields: `scanned_at`, `engine_version`.
- Sort `findings` by (`file`, `line`, `rule_id`), `components` by (`path`, `name`), `requirements` by (`name`, `evidence_file`, `evidence_line`).
- Keep `targets` for `php:8.3`, `php:8.4` and `php:8.5`.

### How a test runs
1. `leapcheck scan-wp --db <temp> --slug S --version V --target 8.3,8.4,8.5` for each corpus entry (network needed; nightly or on demand, not on every push).
2. `leapcheck export --db <temp> --out <temp>/public`, normalise, then compare with `engine/tests/golden/...`.
3. Fail on any difference. Report it as a readable diff grouped by: verdict changed, findings added, findings removed, components added/removed, requirement changes.

### Updating goldens (blessing)
- An intended change (new rule, fixed false positive) is "blessed" by regenerating the goldens with an explicit flag, e.g. `LEAPCHECK_BLESS=1 cargo test --test golden -- --ignored` (exact switch to be agreed with the engine team).
- The golden diff is part of the pull request. A reviewer reads it; any verdict change on a corpus entry needs a one-line reason in the PR.
- Never bless a verdict change from `breaks` to `compatible` without a manual check of the affected findings (section 3).

### Fast tier (every push, offline)
The CI workflow (`.github/workflows/ci.yml`) runs `cargo test` on every push and PR, plus a PHPCompatibility smoke test that must report `create_function()` as an error for PHP 8.4. Small, hand-written PHP fixtures (a few lines each, our own code, committed under `engine/tests/fixtures/`) cover every rule, signature in `engine/rules/php-signatures.json` and extension pattern in `engine/rules/php-extensions.json` without network access.

### Slow tier (nightly / before release, online)
The full corpus golden comparison. Until the engine has a golden test harness, the nightly scan workflow (`.github/workflows/scan.yml`) exercises the corpus end to end, and its traces (`leapcheck runs --failed`, `leapcheck health`) are reviewed each morning.

## 3. Accuracy review (manual check of a sample)

Purpose: measure how often our findings are right, and catch systematic false positives/negatives that goldens alone would freeze in.

### Sample
- Per release candidate: 30 findings, drawn at random from the corpus results, stratified: 10 `error`, 10 `warning`, 5 component detections, 5 extension requirements.
- Plus every verdict change versus the previous release (all of them, not sampled).
- Plus 3 corpus entries reported `compatible`, read for false negatives (search the code for known PHP 8 removals: `create_function`, `each(`, curly-brace string offsets, `${var}` interpolation, removed `mysql_*`, etc.).

### Procedure per finding
1. Open the file and line in the plugin version (download it locally to an isolated folder; never execute it).
2. Decide: **TP** (real problem on that PHP target), **FP** (not a problem), or **Unsure** (dynamic code; escalate to the Compatibility Expert).
3. Check severity (error vs warning) and confidence level are right, and that the message is understandable.
4. For components: confirm name, version and path against the library's own version string.
5. Record in `docs/qa/accuracy/<release>.md`: plugin, version, target, rule_id, file:line, verdict (TP/FP/Unsure), note.

### Metrics and gates
- Precision = TP / (TP + FP) over the sample. Release gate: precision >= 95% for `error` findings, >= 90% overall. **UNVERIFIED target**, to be confirmed after the first baseline review.
- Every FP becomes a fixture or corpus entry plus a rule fix or a confidence downgrade.
- Every false negative found becomes a corpus entry.
- Trend tracked per release; "escaped defects" = user-reported wrong verdicts after release.

## 4. Release checklist

Before tagging an engine release or deploying a rules change to the public site:

- [ ] CI green on the release commit: `cargo build --release`, `cargo test`, PHP tools install and smoke test, `php -l` on `plugin/`.
- [ ] Golden comparison run on the full corpus; every diff explained in the PR and blessed by a reviewer.
- [ ] Accuracy review done (section 3) and recorded; precision gate met.
- [ ] All 40 corpus entries scanned for `php:8.3`, `php:8.4`, `php:8.5`; none failed unexpectedly (`leapcheck runs --failed` empty or each failure explained).
- [ ] `leapcheck health` failure rate below 5% on the latest nightly. **UNVERIFIED threshold**, revisit after the first month.
- [ ] Encoded plugins return `unknown`, never `compatible`.
- [ ] Exported JSON validates against the schema in `docs/07-interfaces.md` (`schema: 1`, required fields, verdict rules: `breaks` = >= 1 error, `issues` = only warnings, `compatible` = none).
- [ ] `engine_version` bumped; rules version recorded in each run.
- [ ] Spot-check 5 result URLs on the preview deployment (`/v1/wordpress-plugin/{name}/{version}.json` and `index.json`).
- [ ] WordPress plugin: `php -l` on PHP 7.4 and 8.4, manual smoke test on a clean WordPress install with opt-in off (no outbound requests) and on.
- [ ] Release notes list rule changes and verdict changes on popular plugins.
- [ ] Rollback plan: previous Vercel deployment can be promoted again (see `docs/ops/runbook.md`).

## 5. Ownership
- QA owns the corpus, goldens policy, accuracy review and release checklist.
- DevOps owns CI/scan workflows and deploy.
- Engine team owns the golden test harness implementation in `engine/tests/`.
