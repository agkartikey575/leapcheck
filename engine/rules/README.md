# engine/rules: hidden-library and requirement rules

These files are data, not code. The engine (`engine/src/detect.rs`) loads them at start-up
(`--rules engine/rules` by default). Changing a rule needs no rebuild. Results store the rules
version, so bump it when a rule changes and re-scan the affected packages.

| File | What it does | Owner | Source |
|---|---|---|---|
| `php-signatures.json` | Finds PHP libraries copied into plugins/themes, and their versions | LeapCheck | our own, checked against real plugins |
| `php-extensions.json` | Finds PHP extensions the code calls directly (gd, intl, ...) | LeapCheck | our own, checked against real plugins |
| `retire-jsrepository.json` | Finds JS libraries (often minified) and known vulnerabilities | upstream RetireJS | Apache-2.0 open data (see below) |
| `suppressions.json` | Known false positives and rules that do not break on an upgrade (remove, or lower to warning / info) | LeapCheck | our own, checked against real plugins |
| `confirmed-breaks.json` | Errors in bundled library code that were checked by hand and are reached by the package, so they keep the "breaks" verdict | LeapCheck | our own, each entry checked in the real package |
| `not-reached.json` | Errors and warnings in one exact package version that were checked by hand and never run, so they become info | LeapCheck | our own, each entry checked in the real package |
| `wp-polyfills.json` | New PHP functions WordPress core defines itself on older PHP (`wp-includes/compat.php`) | LeapCheck | WordPress core source (GPL), function names and versions only |

Current contents (v1, 2026-11-21): **41 PHP signatures, 25 extensions, 76 RetireJS library entries (798 vulnerability records)**.

---

## 1. `php-signatures.json`

```json
[{
  "name": "phpmailer",
  "match_file_regex": "(^|/)(class\\.phpmailer\\.php|PHPMailer\\.php)$",
  "version_regex": "(?:\\$Version|const\\s+VERSION)\\s*=\\s*['\"](\\d[0-9A-Za-z.\\-]*)['\"]",
  "notes": "free text: what it is, PHP 8 risk, known gaps",
  "verified_in": ["wysija-newsletters@2.7.5 (inc/phpmailer/class.phpmailer.php, 2.0.4)"]
}]
```

| Field | Required | Meaning |
|---|---|---|
| `name` | yes | Component name written to results (`components[].name`). Lower-case, stable: public pages link to it. |
| `match_file_regex` | yes | Rust `regex` crate syntax, matched against the file path **relative to the plugin/theme root**, with `/` separators (e.g. `vendor/tecnickcom/tcpdf/tcpdf.php`). The engine only tries files ending in `.php` or `.inc`. Anchor the file name with `(^|/)...$`. |
| `version_regex` | yes (string) | Run on the **first 64 KB** of the matched file. Must have **exactly one** capture group: the version. If it does not match, the component is still reported with `version: null`. |
| `notes` | no | Ignored by the engine. Explains the rule and its known gaps. |
| `content_regex` | no | The first 64 KB of the matched file must also match this regex. Use it when the file name is generic (`ftp.php`, `Version.php`). |
| `root_up` | no | Number of folders above the matched file's folder to use as the component path (default 0), for version files that sit below the library root (`libraries/classes/Version.php` in phpMyAdmin). |
| `verified_in` | no | Ignored by the engine. Where the rule was checked: `slug@version (path, version found)`. |

Conventions:
- **Presence-only rules.** Some libraries have no version in their source (PhpSpreadsheet, Carbon,
  phpseclib, dompdf's PHP files). The engine needs a string, so these use the never-matching
  pattern `(\b\B)` and report `version: null`. Do not use JSON `null`: the engine's loader then fails to parse
  the whole file and the scan stops with an error.
- Rust `regex` has no look-around and no backreferences. Use `(?i)` only when folder case really
  varies between copies (it causes false matches otherwise, e.g. Kirki's `src/Kirki.php` class file).
- The component's `path` in results is the matched file's folder. One component per
  `(name, folder)`, so two copies of one library in one plugin are both reported.
- A rule matching a plugin's own main file (e.g. `kirki.php` in the Kirki plugin itself) reports
  the plugin as a component of itself. This is harmless but must be ignored in public copy.

### Libraries covered (all verified against real downloaded code)

phpmailer, tcpdf, fpdf, mpdf, dompdf*, guzzle (6+), guzzle3, twig, simplepie, phpexcel, phpspreadsheet*,
monolog (major only), carbon*, freemius, action-scheduler, cmb2, redux-framework (4.x), reduxcore (3.x),
titan-framework, tgm-plugin-activation, kirki, carbon-fields, stripe-php, paypal-rest-sdk,
google-api-client, aws-sdk-php (v3: version usually unknown, see below; v1.x via sdk.class.php), requests, htmlpurifier, phpseclib*,
parsedown, mobile-detect, getid3, scssphp, scssphp-legacy, lessphp, less.php, pemftp*, html_quickform2*, xhprof*, phpmyadmin.
`*` = presence-only (no version in source).

**Not included because they could not be verified** in the packages downloaded (no WordPress.org copy found
in this round): Smarty, Symfony components (HttpKernel `Kernel::VERSION`), Plugin Update Checker
(wordpress.org guidelines forbid third-party updaters, so free copies are rare), PHPMailer 6 bundled
inside a plugin (the 6.x pattern was checked against WordPress core's copy only), HTMLPurifier
standalone build, Mobile-Detect 3.x+, Twig < 1.38, mPDF < 7, dompdf < 0.8. Add them only after a
real copy has been checked.

## 2. `php-extensions.json`

```json
[{
  "extension": "gd",
  "patterns": ["...regex...", "..."],
  "exclude_path_regex": "(^|/)symfony/polyfill-mbstring/",
  "notes": "free text"
}]
```

| Field | Required | Meaning |
|---|---|---|
| `extension` | yes | PHP extension name as `php -m` prints it (lower case). |
| `patterns` | yes | Rust regexes run **line by line** over every `.php` file. Lines starting with `//`, `*` or `#` are skipped. The first matching line becomes the evidence (`requirements[].evidence_file/line`); one piece of evidence per extension per package. **A pattern that does not compile is silently dropped** by engine 0.1.0, so test every change (method below). |
| `exclude_path_regex` | no | **Proposed, not yet read by engine 0.1.0.** Files whose relative path matches should be skipped for this extension (polyfills such as `symfony/polyfill-mbstring`, `paragonie/sodium_compat`). |
| `notes` | no | Ignored by the engine. Caveats, e.g. "always on in PHP 8" for `json`. |

How the patterns avoid false positives:
- Function calls must be preceded by the start of the line, an operator or punctuation, a keyword
  (`return`, `echo`, ...) or a leading `\`. This excludes `$obj->curl_init()`, `Foo::mb_strtolower()`,
  `wp_json_encode()`, quoted names in `function_exists('curl_init')` checks, and
  `function mb_strtolower(` definitions in polyfills.
- Classes are matched as `new \?Class` or `\?Class::...`, never as bare words, so text such as
  "Redis" or "Imagick" in prose does not count.
- `mb_substr` and `mb_strlen` are left out because WordPress core polyfills them.
- APCu skips Composer's `apcu_fetch($this->apcuPrefix...)`, which ships in almost every `vendor/`.

Extensions: gd, imagick, mbstring, intl, curl, zip, dom, simplexml, xml, xmlreader, sodium, bcmath,
gmp, exif, openssl, soap, mysqli, iconv, fileinfo, sockets, ftp, ldap, apcu, redis, memcached. (`json` was
removed: it is always enabled since PHP 8.0.)

## 3. `retire-jsrepository.json` (third-party open data)

- **Source:** https://raw.githubusercontent.com/RetireJS/retire.js/master/repository/jsrepository.json
  (repository https://github.com/RetireJS/retire.js, path `repository/jsrepository.json`; the path has not moved).
- **Licence:** Apache License 2.0 (repository `LICENSE.md`). Keep this attribution and the licence
  name when the file is redistributed; we use it unmodified.
- **Downloaded:** 2026-11-04, 609,700 bytes, SHA-256
  `1ba9dff7c8e0767bec64aa2da255e8cf08a5dc1d43c279ab46c0c2059f2f6f23`. 76 entries (incl. `retire-example`), 798 vulnerability records.
- **Engine use:** only the `filename` and `filecontent` extractors and `below`/`atOrAbove` ranges are used.
  `filecontentreplace`, `hashes`, `uri` and `func` are ignored. 250 of 252 filename/filecontent
  patterns compile in Rust `regex`; 2 fail because they use JavaScript backreferences/look-behind
  (one jQuery pattern, one lodash pattern) and are silently skipped. Other jQuery/lodash patterns still work.

**Refresh (monthly, and before every public data release):**
```sh
cd engine/rules
curl -fsS -o retire-jsrepository.json.new \
  https://raw.githubusercontent.com/RetireJS/retire.js/master/repository/jsrepository.json
python3 -I -c "import json; d=json.load(open('retire-jsrepository.json.new')); assert 'jquery' in d; print(len(d))"
mv retire-jsrepository.json.new retire-jsrepository.json
sha256sum retire-jsrepository.json   # then update the date, size and hash above
```
If the URL returns 404, check the RetireJS repository for where `jsrepository.json` now lives
(the CLI downloads it from the same repository) and update this README.

---

## 4. `suppressions.json`

```json
[{"rule_id": "PHPCompatibility.IniDirectives.RemovedIniDirectives.*", "path_regex": ".*", "action": "warning", "reason": "..."}]
```

| Field | Required | Meaning |
|---|---|---|
| `rule_id` | yes | Exact analyzer rule ID, or a prefix ending in `*`. |
| `path_regex` | yes | Rust regex on the file path relative to the package root. |
| `action` | no | `remove` (default): known false positive, dropped. `warning`: errors become warnings (real change, code still runs). `info`: errors and warnings become info (not a change caused by the upgrade; shown, never counted in the verdict). |
| `reason` | yes in practice | Appended to the finding message in brackets. Written for site visitors. |
| `message_regex` | no | The finding's message must also match this regex (e.g. `removed since PHP ([0-7]\\.\|8\\.[0-2]\\b)` to treat only directives removed before PHP 8.3). |

`rule_id` patterns: `*` stands for any text, anywhere in the pattern (`A.B.*`, `A.B.*Removed`).

The first entry that matches a finding decides, so put narrow entries before broad ones.

## 5. `wp-polyfills.json`

```json
[{"function": "array_first", "wp_since": "6.9", "php_since": "8.5"}]
```

`function` and `wp_since` are read by the engine; `php_since` is documentation. A `NewFunctions` finding
for a listed function becomes `info` when the package's `Requires at least:` (plugin header, theme
`style.css`, or readme) is at least `wp_since`, otherwise `warning` ("needs WordPress X+ on this PHP").
Source: `wp-includes/compat.php` in WordPress core (array_first/array_last: changeset 60672, ticket #63853).
Only versions already released are listed.

## 6. `confirmed-breaks.json`

```json
[{"package": "w3-total-cache", "rule_id": "PHPCompatibility.ParameterValues.RemovedImplodeFlexibleParamOrder.Removed",
  "path_regex": "(^|/)lib/Google/Http/REST\\.php$", "reason": "...", "verified_in": ["w3-total-cache@2.11.0 (...)"]}]
```

When every error of a target lies inside a bundled third-party library (the finding has a
`component`), the verdict is `issues`, not `breaks`, and those errors get the message prefix
"In bundled library code (may only break if this code runs): ". Bundled code is often never
called, and a static scan cannot prove that it is. An error in the package's own code keeps `breaks`.
An entry here marks a bundled-library error that was checked by hand and *is* reached, so it keeps
`breaks`. `package` is the slug, `rule_id` an exact rule ID or a prefix ending in `*`, `path_regex` is matched
against the file path relative to the package root. `reason` and `verified_in` are documentation. Add an
entry only after reading the code path that calls the library.

## 7. `not-reached.json`

```json
[{"slug": "wp-db-backup", "version": "2.5.3", "file": "wp-db-backup.php",
  "rule_id": "PHPCompatibility.FunctionUse.RemovedFunctions.eachDeprecatedRemoved",
  "reason": "each() is only in the is_array() branch of backquote(), and all 11 callers pass a string",
  "reviewed_by_date": "2026-11-19"}]
```

The mirror of `confirmed-breaks.json`: an error or warning that was checked by hand and is never
reached (a function nothing calls, a branch no caller can enter). A static scan cannot prove this, so each
entry is pinned to the **exact** package `slug`, `version`, `file` (relative to the package root)
and `rule_id` (exact, no patterns). A matching error or warning becomes info with the message prefix
"Checked by hand, this code is not reached (<reason>): ". A new package version does not match, so
it is checked again automatically. `reason` is shown to site visitors; `reviewed_by_date` is
documentation. Add an entry only after reading every caller of the code in that version.

## Verification method (how every rule was checked)

1. **Real code only.** Plugins/themes were downloaded from WordPress.org
   (`https://downloads.wordpress.org/plugin/SLUG.zip`, `SLUG.VERSION.zip` for old versions,
   `https://downloads.wordpress.org/theme/SLUG.zip`), each into its own new empty folder under
   `/tmp/claude-compat/`. Nothing downloaded was executed; files were only read with `find`/`grep`.
   75 packages were used (65 plugins incl. 7 old versions, 10 themes) plus WordPress core 6.8.3 for two form checks.
2. **Read the file.** For each library, the real file name, folder layout and version line were
   read in at least one copy (the `verified_in` field records which), including byte offset when
   it might fall past 64 KB.
3. **Compile with the engine's regex crate.** A small checker (Rust, `regex` 1.x, same as the engine)
   compiles every rule, asserts exactly one capture group per `version_regex`, then repeats the
   engine's logic (relative path, `.php`/`.inc` only, 64 KB head, line-by-line extension scan, comment
   lines skipped) over all 75 packages. Every hit was reviewed by hand for wrong matches (e.g.
   PhpSpreadsheet's `Writer/Pdf/Mpdf.php`, `Calculation/Engine/Logger.php`, psr `ClientInterface.php`,
   Kirki's `src/Kirki.php`), and rules were tightened until only real copies matched.
4. **End-to-end with the built engine.** `leapcheck scan-dir` + `export` on wp-user-manager,
   wedding-party-rsvp, leaflet-maps-marker and ad-inserter; the exported JSON `components` and
   `requirements` matched the checker (e.g. wp-user-manager: carbon, carbon-fields 3.6.9, dompdf,
   stripe-php 10.5.0; leaflet-maps-marker: select2 3.5.4 via RetireJS, 1 known vulnerability).

To re-verify after editing a rule: run the engine (`scan-dir` on a folder that contains the library,
then `export`) and check the `components`/`requirements` in the JSON; also confirm the start-up log line
`signatures=N extensions=M` shows the expected counts, because invalid rules are dropped silently.
Known limits and false-positive risks: `docs/quality/accuracy-notes.md`.
