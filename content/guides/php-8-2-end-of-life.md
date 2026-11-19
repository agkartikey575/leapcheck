---
title: PHP 8.2 end of life (31 Dec 2026): WordPress guide
description: PHP 8.2 security support ends on 31 December 2026. What that means for your WordPress site, which version to move to, and a safe 3-step plan.
h1: PHP 8.2 end of life (31 Dec 2026): what WordPress site owners need to do
updated: 2026-11-19
---

**Short answer:** PHP 8.2 gets its last security fixes until **31 December 2026**. After that date, the PHP project stops releasing fixes for it, even for security holes. Your site will not switch off on 1 January, but it will run on software that no longer gets patched. The safe move is to check your plugins and theme, then upgrade to PHP 8.3, 8.4 or 8.5 with a way to roll back.

The official dates are on the PHP project's [supported versions page](https://www.php.net/supported-versions.php) and its [list of unsupported branches](https://www.php.net/eol.php).

## The dates that matter

Each PHP version gets about two years of bug fixes, then security-only fixes. Since PHP 8.1, security support lasts until the end of the year four years after the release.

- **PHP 8.1**: security support ended on 31 December 2025.
- **PHP 8.2** (released December 2022): bug fixes ended on 31 December 2024; security support ends on **31 December 2026**.
- **PHP 8.3** (released November 2023): security support until 31 December 2027.
- **PHP 8.4** (released 21 November 2024): bug fixes until 31 December 2026; security support until 31 December 2028.
- **PHP 8.5** (released 20 November 2025): security support until 31 December 2029.

Check the [supported versions page](https://www.php.net/supported-versions.php) for the current state; the PHP project publishes any change there first.

## What "end of life" means, and what it doesn't

**It means:** no more official PHP releases for 8.2. If a security problem is found in PHP 8.2 after 31 December 2026, the PHP project will not ship a fix for it.

**It doesn't mean:**

- Your site stops working on 1 January 2027. PHP 8.2 keeps running exactly as before.
- Your site is suddenly unsafe. The risk is that any PHP 8.2 security problem found after that date stays unfixed, so the risk grows over time.

**What usually forces the change is your host.** Hosts decide which PHP versions they keep offering. When a version reaches end of life, a host may move sites to a newer version automatically, keep offering the old one for a while, or offer paid extended support. Read your host's emails and help pages, or ask their support team: "When will you stop offering PHP 8.2, and will you move my site automatically?" More questions to ask are in [PHP 8.2 end of life and your hosting](/guides/php-8-2-end-of-life-hosting/).

If your host upgrades PHP for you and a plugin is not ready, that is when sites break. Checking first puts you in control of the timing.

## Which version should you move to?

- **PHP 8.4** is a good default for most sites. It has security support until the end of 2028, and it has been out since November 2024, so plugin authors have had two years to test against it.
- **PHP 8.3** is the smaller step from 8.2, with fewer changes. But its security support ends on 31 December 2027, so you will do this again next year.
- **PHP 8.5** has the longest support (to the end of 2029), but it is the newest. Some plugins may still show deprecation notices on it.

Whatever you pick, check your plugins against that exact version first. A plugin that works on 8.3 can still show problems on 8.4 or 8.5. See [what changed in PHP 8.3, 8.4 and 8.5](/guides/php-8-4-changes-wordpress/) for the details.

## Is WordPress itself compatible with PHP 8.4 and 8.5?

WordPress core publishes its PHP support per release on its [PHP compatibility and WordPress versions page](https://make.wordpress.org/core/handbook/references/php-compatibility-and-wordpress-versions/). Recent WordPress releases list support for PHP 8.4 and 8.5; check the entry for the WordPress version you run, and update WordPress first if you are behind.

Core support is only part of the picture. It says nothing about your plugins and theme, and they are where most PHP upgrade problems come from.

## The 3-step plan

### 1. Check

Find your current PHP version in your dashboard under **Tools → Site Health → Info → Server** (step by step: [how to check your PHP version](/guides/how-to-check-php-version-wordpress/)). Then make a list of your active plugins and your theme, and check each one against the PHP version you plan to move to.

You can look up any public plugin in our [plugin results](/plugins/): each page shows the result for PHP 8.3, 8.4 and 8.5, with the file and line of every finding. To check everything installed on a site at once, [download the free LeapCheck plugin](/get/). It is waiting for review in the WordPress.org plugin directory, so for now you install it from our site.

### 2. Fix or update

- Update WordPress, every plugin and your theme. Many compatibility problems are already fixed in newer versions.
- For a plugin that still has errors on its latest version, ask the author in its support forum, or look for a maintained replacement. See [what to do when a plugin isn't ready](/guides/php-8-4-wordpress-plugins-not-compatible/).
- For a plugin that has been closed or not updated in years, plan to replace it. Nobody will fix it for a new PHP version.
- Look out for old libraries bundled inside plugins. They are a common hidden blocker: see [hidden libraries and PHP upgrades](/guides/hidden-libraries-php-upgrades/).

### 3. Upgrade with a rollback ready

Back up first. Test on a staging copy if your host offers one. Switch the PHP version, test the pages and forms that matter, and know how to switch back. The full walkthrough is in [how to upgrade PHP on WordPress safely](/guides/upgrade-php-wordpress-safely/).

## Frequently asked questions

### Can I stay on PHP 8.2 after 31 December 2026?

Technically yes, if your host still offers it. Your site will keep working. But it will run on a PHP version with no official security fixes, and your host may remove 8.2 later on its own schedule. Staying is a decision to delay, not a solution.

### Will my theme break?

Themes contain PHP code too, so they can have the same problems as plugins. Check your active theme and, if you use one, its parent theme.

### What is the difference between a warning and an error?

A **deprecation warning** means the code still runs, but uses something PHP plans to remove in a future version. An **error** means the code will fail on that PHP version, which can show "There has been a critical error on this website". See [deprecated vs fatal error](/guides/php-8-4-changes-wordpress/) for examples.

### My site is already on PHP 8.1 or older. What now?

Those versions no longer get security fixes either. The same plan applies, but the jump is bigger, so checking first matters even more.
