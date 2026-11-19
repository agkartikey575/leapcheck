---
title: How to check which PHP version your WordPress site runs
description: Find your site's PHP version in Site Health or your hosting panel, what the three numbers mean, and whether your version still gets security fixes.
h1: How to check which PHP version your WordPress site runs
updated: 2026-11-19
---

**Short answer:** in your WordPress dashboard, go to **Tools → Site Health**, open the **Info** tab and expand the **Server** section. The line **PHP version** shows the version your site runs on right now. Your hosting control panel usually shows it too, and lets you change it.

This guide shows both places, explains what a number like 8.2.x means, and how to tell whether your version still gets security fixes.

## Way 1: Site Health in your dashboard

You need to be logged in as an administrator.

1. In the left menu, go to **Tools → Site Health**.
2. Click the **Info** tab at the top.
3. Click **Server** to expand it.
4. Read the **PHP version** line.

Next to the number you may see a note such as "(Supports 64bit values)". That is about how the server was built, not about which version you run, and you can ignore it here.

The WordPress documentation describes the whole screen on its [Site Health screen](https://wordpress.org/documentation/article/site-health-screen/) page. The **Status** tab on the same screen runs checks, and it may recommend updating PHP if your version is older than WordPress recommends.

**Tip:** the **Copy site info to clipboard** button on the Info tab copies the PHP version together with your WordPress version, active plugins and theme. It is the quickest way to give a developer or support team everything they need. Read through it before you paste it anywhere public, because it also includes details about your server.

**Why Site Health is the number to trust:** it is read by WordPress itself while your site is running, so it is the version your site really uses. Other places can show a different number. For example, a host may run a different PHP version on the command line than for your website, or allow a different version per domain or folder.

## Way 2: Your hosting control panel

Every host labels this differently. Look for a menu called something like "PHP version", "PHP settings", "Select PHP version", "PHP manager" or "Software" in your hosting account. If you manage several sites, check the setting for the right domain or folder: some panels let each one run a different version.

The panel is also where you would change the version later. Don't change it yet: check your plugins and theme first, as described in [how to upgrade PHP on WordPress safely](/guides/upgrade-php-wordpress-safely/).

If you can't find the setting, ask your host's support team: "Which PHP version does my site use, and which versions can I choose?"

## What the numbers mean

A PHP version has three parts, for example **8.4.x**:

- **8** is the major version. A new major version (like the move from PHP 7 to PHP 8) removes old features and is the biggest step.
- **4** is the minor version. Together, **8.4** is what the PHP project calls a *branch*. Support dates are set per branch, so this is the number that matters for "is my PHP still supported?".
- **The last number** is the patch release within that branch. Patch releases fix bugs and security problems without changing how the language works. While a branch is supported, a higher last number means more fixes. Your host normally installs these for you.

So when people say "PHP 8.2 reaches end of life", they mean every 8.2.x release, whatever the last number is.

## Is your version still supported?

The PHP project publishes the dates for every branch on its [supported versions page](https://www.php.net/supported-versions.php). Each branch first gets about two years of bug and security fixes ("active support"), then security fixes only, then nothing. Branches without any support are listed on the [unsupported branches page](https://www.php.net/eol.php).

As of 19 November 2026, the security support dates are:

- **PHP 8.1 or older**: no longer supported. PHP 8.1 security support ended on 31 December 2025.
- **PHP 8.2**: security fixes only, until **31 December 2026**. See [PHP 8.2 end of life](/guides/php-8-2-end-of-life/).
- **PHP 8.3**: security fixes only, until 31 December 2027.
- **PHP 8.4**: security support until 31 December 2028.
- **PHP 8.5**: security support until 31 December 2029.

The PHP project's [PHP 8.6 schedule](https://wiki.php.net/todo/php86) planned the first 8.6 release for 19 November 2026. Check the supported versions page for its status; plugins and themes usually need some time to catch up with a brand-new branch.

WordPress itself also publishes a recommended PHP version on its [requirements page](https://wordpress.org/about/requirements/), and per-release support on its [PHP compatibility and WordPress versions page](https://make.wordpress.org/core/handbook/references/php-compatibility-and-wordpress-versions/).

## What to do with the number

- **8.1 or older:** your site runs on PHP with no security fixes. Plan an upgrade now, and expect a bigger jump. Checking your plugins first matters even more.
- **8.2:** you have until 31 December 2026 before security fixes stop. Ask your host what they will do after that date (see [what hosts do when PHP versions reach end of life](/guides/php-8-2-end-of-life-hosting/)), and plan your move to 8.3, 8.4 or 8.5.
- **8.3 or newer:** you are on a supported branch. Note its end date so the next upgrade doesn't surprise you.

Before you switch, find out which of your plugins and theme are ready for the version you want. You can look up public plugins in our [plugin results](/plugins/), or [get the free LeapCheck plugin](/get/) to check everything installed on your site at once. If a plugin turns out not to be ready, see [what to do when a plugin isn't compatible with PHP 8.4](/guides/php-8-4-wordpress-plugins-not-compatible/).

## Frequently asked questions

### I can't see Tools → Site Health. Why?

Site Health is only shown to administrators. If you are an administrator and still can't see it, a plugin or your host may have hidden it. Your hosting panel is then the next place to look.

### Site Health and my hosting panel show different versions. Which is right?

Trust Site Health for what your site runs on right now. A different number in the panel can mean the change hasn't been applied yet, or that the panel shows the setting for another domain, folder or for the command line. Ask your host if it isn't clear.

### Does updating WordPress update PHP?

No. WordPress and PHP are updated separately. WordPress updates come from WordPress; the PHP version is set by your host or in your hosting panel.
