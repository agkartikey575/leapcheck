# Scope: Upgrade readiness checker (working name "UpgradeSafe")

## Promise
"Before you upgrade your language version, know exactly what will break, including code hidden inside your dependencies."

## Upcoming end-of-support dates (endoflife.date, checked 2026-11-02)
| Runtime | Version | End of support |
|---|---|---|
| PHP | 8.2 | 31 Dec 2026 |
| Python | 3.10 | 1 Oct 2026 (already ended) |
| Node.js | 22 LTS | 30 Apr 2027 |
| Python | 3.11 | 31 Oct 2027 |
| PHP | 8.3 | 31 Dec 2027 |

## "Hidden" things we detect (feasible)
1. **Bundled libraries:** copies of other libraries shipped inside a plugin or package (e.g. an old PHPMailer, Guzzle or jQuery in a plugin folder). Detected by file fingerprints, version strings and package manifests.
2. **Minified JavaScript libraries** inside themes and plugins: version detection, checked against known breakage and known vulnerabilities.
3. **Required server extensions** (e.g. GD or Imagick for image processing, mbstring, intl). Images themselves don't break; the code and extensions that process them do.
4. **Copied code snippets** from known libraries (fingerprint matching), in a later phase.
5. **Deprecated or removed functions** used directly or through dependencies.

## Honest limits
- Static analysis can't see everything (code built at runtime, `eval`, dynamic includes). Reports show confidence levels and never promise 100%.
- Encrypted or encoded code (e.g. ionCube) can't be scanned. We report it as "unknown".

## Phases
| Phase | When | Scope | Go/no-go gate |
|---|---|---|---|
| **1** | Now → 31 Dec 2026 | WordPress/PHP: pre-scanned DB of public plugins/themes; free plugin; bundled-library + minified-JS detection; required extensions; public "Is X compatible with PHP 8.4?" pages | ≥ 2,000 installs or ≥ 300 active weekly users by end of Jan |
| **2** | Jan–Mar 2027 | Paid agency dashboard + hosting-company API; CLI + GitHub Action for any PHP project (Laravel, Composer) | ≥ 20 paying accounts by end of Mar |
| **3** | Mar–Apr 2027 (before Node 22 EOL) | Node.js analyzer: CLI + GitHub Action + public "Is <npm package> ready for Node 24?" pages | Traction in phase 2 |
| **4** | Jul–Oct 2027 (before Python 3.11 EOL) | Python analyzer, same model | Traction in phase 3 |

## Architecture rule
Build the **core once, language-neutral**: job queue, fingerprint database, report format, website, billing. Each language is a **plug-in analyzer**. Phase 1 implements only the PHP analyzer, so adding Node.js and Python later is a matter of adding analyzers, not a rebuild.

## Competition notes
- PHP/WordPress: the incumbent is abandoned; other tools are small (≤ 9k installs).
- Node.js/Python: stronger free tools exist (Dependabot, Renovate, vermin, pyupgrade, npm `engines`). Our edge there: one cross-language "upgrade readiness" report, including bundled and hidden code. To be verified with a live check before phase 3.

## Money
Free for phase 1 (CEO/CFO decide when Pro opens, based on the gates). Target running cost < €40/month until revenue.
