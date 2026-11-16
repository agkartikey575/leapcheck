# engine/rules: hidden-library and requirement rules

These files are data, not code. The engine (`engine/src/detect.rs`) loads them at start-up
(`--rules engine/rules` by default). Changing a rule needs no rebuild. Results store the rules
version, so bump it when a rule changes and re-scan the affected packages.

| File | What it does | Owner | Source |
|---|---|---|---|
| `php-signatures.json` | Finds PHP libraries copied into plugins/themes, and their versions | compatibility-expert | our own, checked against real plugins |
| `php-extensions.json` | Finds PHP extensions the code calls directly (gd, intl, ...) | compatibility-expert | our own, checked against real plugins |
| `retire-jsrepository.json` | Finds JS libraries (often minified) and known vulnerabilities | upstream RetireJS | Apache-2.0 open data (see below) |

Current contents (v1, 2026-11-04): **35 PHP signatures, 26 extensions, 76 RetireJS library entries (798 vulnerability records)**.

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
google-api-client, aws-sdk-php (version usually unknown, see below), htmlpurifier, phpseclib*,
parsedown, mobile-detect, getid3, scssphp, scssphp-legacy, lessphp, less.php.
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
gmp, exif, openssl, soap, mysqli, iconv, fileinfo, json, sockets, ftp, ldap, apcu, redis, memcached.

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
