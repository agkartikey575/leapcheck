# Final shortlist (2026-11-01)

Sources checked: TrustMRR (verified revenue, 7 categories), Starter Story ($1M APIs, $100k Plugins), ProblemHunt, Exploding Topics, the WordPress.org plugin directory (3,000 most popular plugins, ratings and 1★ reviews), endoflife.date, WordPress.org PHP stats, and the earlier web and live marketplace checks. Reddit, Product Hunt, Indie Hackers, IdeaBrowser and BigIdeasDB block automated access.

## Key finding
Ideas that make the news (regulations, AI shopping, AI labelling) are flooded with zero-install clones. The best signal came from **a big, unhappy user base with an abandoned incumbent**.

## #1 PHP upgrade compatibility checker for WordPress (recommended)
**Evidence**
- The incumbent "PHP Compatibility Checker" has 200,000 installs, a 54/100 rating and 58% 1–2★ reviews. Dozens of reviews say "abandoned", "no PHP 8", "hangs", "timeouts", "false results".
- **Deadline:** PHP 8.2 security support ends **31 Dec 2026**. WordPress.org stats: 8.2 = 22.9% of WP sites; 7.4/8.1/8.0/older ≈ 31% are already unsupported. That's about **half of all WordPress sites** on unsupported PHP from January, and hosts force upgrades, which break sites.
- **Competition:** Eli's scanner (1k installs), Plugin Compatibility Checker (9k), phpinfo() WP (3k), CompatNav (200, new). All small.

**Product**
- Free plugin: one-click "Can I upgrade to PHP 8.3/8.4/8.5?" report in seconds.
- **The smart part:** public plugin and theme code (GPL) is pre-scanned on our servers with open-source PHPCompatibility. The site sends only plugin slugs and versions (opt-in), and the answer is instant. That fixes the incumbent's timeouts on shared hosting. Custom code gets scanned locally in small batches.
- **Pro (€49–99/yr; agency tier €199+/yr):** many sites, scheduled reports, alerts when an update breaks compatibility, white-label PDF reports for clients.
- **B2B API for hosting companies:** they need customers off old PHP versions.
- **Self-marketing:** WordPress.org directory search, plus up to about 60k honest data pages like "Is [plugin] compatible with PHP 8.4?", built from real scan data.

**Fit:** legal ✅ (GPL code analysis; opt-in data per wp.org guideline 7; no "WordPress" trademark in the name) · low dependency ✅ (our own scanner, open-source rules, public data) · low maintenance ✅ (re-scan once per PHP release) · cost ≈ €20–40/month.
**Main unknown:** willingness to pay. Free installs are likely; revenue depends on agencies and hosts. Validate by shipping free before 31 Dec, then measure Pro conversions.

## #2 Platform add-on that fills a missing feature (backup)
Evidence: NoteForms (Notion forms) $40k/mo solo, built in 6 days; DataFetcher (Airtable API importer) $20k/mo solo; Superlemon (WhatsApp chat for Shopify) $30k/mo. A specific gap still needs to be found; it depends on the platform's API.

## #3 Developer utility API (backup)
Evidence: 1Lookup (validation API) $273k MRR; ScreenshotOne $20k/mo solo (+892%); Bannerbear $50k/mo. Proven money, but crowded. It needs a sharp niche, and the infrastructure takes more upkeep.

## Rejected
E-invoicing on Stripe (10+ apps), AI Act labelling, CRA, EUDR, US age laws, WooCommerce AI-shopping plugins (7+ clones), and WordPress Importer replacement (migration plugins already have 1–5M installs).
