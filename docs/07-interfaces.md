# Interfaces: the contracts every team builds against

## Engine CLI (Rust binary `leapcheck`)
```
leapcheck scan-wp   --slug <plugin-slug> [--version <v>] [--kind plugin|theme] --target <php-version> [--db data/leapcheck.sqlite]
leapcheck scan-dir  --path <dir> --name <name> --version <v> --ecosystem <eco> --target <php-version>
leapcheck export    --out <dir>               # writes static JSON (below) for every stored result
leapcheck runs      [--id <run-id>] [--failed] # trace viewer: list runs or show one run's steps
leapcheck health                               # daily health report (JSON): runs, failure rate, top errors, slowest steps
```
Logs: one JSON object per line on stderr: `{"ts","level","run_id","step","msg","attrs"}`.

## Static result files (the "API"), served from Vercel
Path: `/v1/{ecosystem}/{name}/{version}.json`, e.g. `/v1/wordpress-plugin/contact-form-7/5.1.json`
Index: `/v1/{ecosystem}/{name}/index.json` lists the scanned versions and the latest one.

```json
{
  "schema": 1,
  "ecosystem": "wordpress-plugin",
  "name": "contact-form-7",
  "version": "5.1",
  "scanned_at": "2026-11-02T12:00:00Z",
  "engine_version": "0.1.0",
  "targets": {
    "php:8.4": {
      "verdict": "compatible | issues | breaks | unknown",
      "errors": 0,
      "warnings": 2,
      "findings": [
        {"rule_id": "...", "severity": "error|warning", "confidence": "high|medium|low",
         "message": "...", "file": "includes/mail.php", "line": 161, "component": null,
         "analyzer": "php:phpcompatibility"}
      ]
    }
  },
  "components": [
    {"name": "phpmailer", "version": "5.2.7", "kind": "php", "path": "lib/PHPMailer",
     "detection": "signature", "known_vulnerabilities": 0}
  ],
  "requirements": [
    {"name": "gd", "evidence_file": "inc/image.php", "evidence_line": 40}
  ]
}
```
Verdict rules: `breaks` = ≥1 error · `issues` = only warnings (deprecations; still runs) · `compatible` = none · `unknown` = could not scan (e.g. encoded files).
Targets in phase 1: `php:8.3`, `php:8.4`, `php:8.5`.

## Hidden-library rule files (data, not code): `engine/rules/`
- `php-signatures.json`: `[{"name","match_file_regex","version_regex","notes"}]`. `version_regex` has one capture group for the version.
- `php-extensions.json`: `[{"extension","patterns":[regex…]}]`, e.g. gd → `\bimagecreate\w*\s*\(`.
- `retire-jsrepository.json`: open data from RetireJS (Apache-2.0) for minified JS library detection and known vulnerabilities.

## WordPress plugin → result files
1. The plugin reads installed plugins and themes (slug + version) and the current PHP version.
2. **Only after the site owner opts in**, it fetches each `/v1/.../{version}.json` (plain GET, no site data sent).
3. Missing file → the plugin shows "not in database yet" (custom/premium code) and offers a local scan later.
4. It shows the verdict per target PHP version, using the UX copy in `docs/product/`.
