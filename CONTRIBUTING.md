# Contributing to LeapCheck

Thanks for helping. LeapCheck tells people what will break before they upgrade PHP, so **accuracy is the product**. The most valuable contributions are reports of wrong results and better detection rules.

By taking part you agree to follow our [Code of Conduct](CODE_OF_CONDUCT.md).

> **Licence status:** the project's licences are being finalised (see `docs/marketing/license-recommendation.md`). **We won't merge outside code contributions until a `LICENSE` file is in the repository.** Issues, result reports and discussion are welcome now.

## Ways to contribute

| You want to… | Do this |
|---|---|
| Report a **wrong result** (false "breaks", missed problem, wrong library version) | Open an issue with the **plugin/theme slug, version, PHP target, the finding (rule, file, line)** and why you think it's wrong. If you have it, include the `run_id` from the result. |
| Report a bug in the WordPress plugin | Open an issue with your WordPress version, PHP version, what you did, what you expected, and what happened. **Never post passwords, logins or private site data.** |
| Report a **security issue** | **Don't open a public issue.** Use GitHub's private vulnerability reporting for this repository ("Security" tab → "Report a vulnerability"), if enabled; otherwise contact the repository owner privately through GitHub. |
| Add or fix a **hidden-library signature** or extension pattern | Edit the JSON rule files in `engine/rules/` (format below) and add a small fixture test. |
| Improve the engine, plugin or docs | Open an issue first for anything bigger than a small fix, so we can agree on the approach. |
| Plugin author asking for a re-scan or correction | Open an issue titled `Re-scan: {slug} {version}`. We aim to reply within 72 hours (target, not a guarantee). |

## Repository layout
- `engine/`: Rust core engine (scan, hidden-library detection, run tracing, static JSON export). See `docs/06-architecture.md` and `docs/07-interfaces.md`.
- `engine/rules/`: detection rules **as data** (`php-signatures.json`, `php-extensions.json`, RetireJS data).
- `tools/php/`: PHP_CodeSniffer + PHPCompatibility, installed with Composer, run by the engine as a separate process.
- `plugin/leapcheck/`: the free WordPress plugin (PHP, GPLv2 or later).
- `docs/`: scope, architecture, interfaces, product, QA and ops docs.

## Development setup

Requirements: Rust (stable), PHP 8.x CLI, Composer.

```sh
# PHP analyzer tools
cd tools/php && composer install && cd ../..

# Engine
cd engine
cargo build
cargo test
cd ..

# Plugin logic test (no WordPress needed; exit code 0 = pass)
php plugin/tests/test-functions.php
php -l plugin/leapcheck/leapcheck.php
```

Try a scan (downloads one plugin from WordPress.org):
```sh
./engine/target/debug/leapcheck scan-wp --slug akismet --target 8.4
./engine/target/debug/leapcheck runs           # see the trace of what happened
./engine/target/debug/leapcheck export --out public
```
All commands and options are in `docs/ops/runbook.md`.

## Rule files (data, not code)

`engine/rules/php-signatures.json`: one entry per bundled library:
```json
{
  "name": "phpmailer",
  "match_file_regex": "(^|/)(class\\.phpmailer\\.php|PHPMailer\\.php)$",
  "version_regex": "(?:\\$Version|const\\s+VERSION)\\s*=\\s*['\"](\\d[0-9A-Za-z.\\-]*)['\"]",
  "notes": "Why this pattern, and what it must NOT match.",
  "verified_in": ["some-plugin@1.2.3 (path/to/file.php, 5.2.7)"]
}
```
- `version_regex` must have **exactly one capture group** (the version).
- `verified_in` lists real plugin versions where you checked the match by hand. **A new signature needs at least one.**
- Prefer patterns that avoid false matches (see the `notes` of existing entries for examples).

`engine/rules/php-extensions.json`: `{"extension": "gd", "patterns": ["\\bimagecreate\\w*\\s*\\("]}`.

For each rule change, add a tiny fixture under `engine/tests/fixtures/` (a few lines of **your own** code that triggers the rule; never copy large chunks of third-party plugins) and make sure `cargo test` passes. See `docs/qa/test-plan.md`.

## Pull request checklist
- [ ] One topic per PR; the description says what changed and why.
- [ ] `cargo test` passes; `cargo fmt` and `cargo clippy` show no new warnings.
- [ ] Plugin changes: `php -l` passes; inputs sanitised, outputs escaped, nonces + capability checks on forms; **no outbound HTTP request before the site owner opts in**; no new external assets (CSS/JS/fonts must be local).
- [ ] No new third-party dependency without a short justification in the PR (licence, maintenance, exit plan). The project prefers its own code and permissively licensed open source.
- [ ] Docs updated if behaviour or the JSON format changed (`docs/07-interfaces.md`).
- [ ] Commits are signed off (`git commit -s`) to certify the [Developer Certificate of Origin](https://developercertificate.org/), i.e. you wrote the change or have the right to submit it under the project's licence.

## Honesty rules for anything user-facing
- Never claim results are guaranteed or 100% accurate.
- Every finding must be explainable: file, line, rule, and the engine and rules version that produced it.
- Never describe a third-party plugin as "insecure" or "abandoned". Report only what the scan found, with its date.

## Questions
Open a GitHub Discussion (if enabled) or an issue labelled `question`.
