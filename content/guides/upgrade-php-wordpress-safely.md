---
title: How to upgrade PHP on WordPress safely, step by step
description: Back up, check plugins, test on staging, switch PHP, and know how to roll back. A practical checklist for upgrading PHP on a WordPress site.
h1: How to upgrade PHP on a WordPress site without breaking it
updated: 2026-11-19
---

Upgrading PHP is usually a single setting in your hosting panel. The risk is not the switch itself but the code that runs on top of it: plugins, themes and the libraries bundled inside them. This guide walks through the upgrade in the order that keeps your site safe: back up, check, update, test on a copy, switch, and keep a way back.

If you are doing this because of the PHP 8.2 deadline, read [PHP 8.2 end of life](/guides/php-8-2-end-of-life/) first for the dates and which version to choose.

## Step 1: Back up and take notes

1. **Make a full backup** of files and database, and make sure you can restore it. A backup you have never restored is a hope, not a plan. Many hosts include backups; a backup plugin works too.
2. **Find your current PHP version.** In your dashboard, go to **Tools → Site Health → Info** and open the **Server** section ([more detail](/guides/how-to-check-php-version-wordpress/)). Write the version down: it is the version you roll back to if needed.
3. **List your plugins and theme** with their version numbers. The Site Health Info screen has a "Copy site info to clipboard" button that includes them.

## Step 2: Check compatibility before you switch

You want to know, per plugin and theme, whether it will run on the new PHP version. There are three ways to find out, and they work best together.

- **Look up the results.** Our [plugin results](/plugins/) show each public plugin's result for PHP 8.3, 8.4 and 8.5, with the file, line and message for every finding. The [free LeapCheck plugin](/get/) does the same for everything installed on your site in one go (it is waiting for review in the WordPress.org directory, so for now you download it from our site). It only downloads public results after you agree, and each request names just one plugin or theme and its version.
- **Read the plugin's own information.** On a plugin's WordPress.org page, "Requires PHP" is the *minimum* version it needs. It does not tell you the newest version it works on. The changelog often mentions PHP 8.x fixes.
- **Ask the author.** If you are unsure, ask in the plugin's support forum which PHP versions they test on.

What the results mean:

- **Compatible**: nothing found for that PHP version.
- **Issues (warnings)**: the plugin uses features PHP has deprecated. It still runs, but your logs may fill with notices, and those features may be removed in a later version.
- **Breaks (errors)**: the plugin uses something removed or changed in that PHP version. Expect a fatal error when that code runs.

Automated checks read the code without running it, so they cannot see everything (for example, code built at runtime). Treat a clean result as a strong signal, not a guarantee, and still test.

## Step 3: Update everything first

1. Update WordPress core.
2. Update every plugin and theme, including inactive ones you might reactivate. Better still, delete plugins you don't use.
3. Check again after updating. Many problems are fixed in the latest version.
4. For anything still showing errors: ask the author, or find a maintained replacement. Plugins that have been closed on WordPress.org or not updated for years are unlikely to get PHP fixes. The options are covered in [what to do when a plugin isn't ready](/guides/php-8-4-wordpress-plugins-not-compatible/).
5. Premium or custom plugins are not in public databases. Ask their vendor or developer directly.

## Step 4: Test on a staging copy

A staging site is a private copy of your site where you can break things safely. Many hosts offer one-click staging. If yours does:

1. Create a fresh staging copy.
2. Switch the staging site to the new PHP version.
3. Run the test checklist in step 6.

If your host has no staging, you can still upgrade the live site carefully. Pick a quiet time, have your backup ready, and keep the hosting panel open so you can switch back within a minute.

## Step 5: Switch the PHP version

Where the setting lives depends on your host. Look for a menu called something like "PHP version", "PHP settings", "Select PHP version" or "Software" in your hosting control panel. Some hosts let you set a version per site or per folder. If you can't find it, your host's help pages or support team can tell you.

Switch to the version you checked in step 2, not a different one.

## Step 6: Test what matters

Right after switching, check:

- **The front end**: home page, a few posts and pages, your menu, search.
- **Anything that takes input**: contact forms, sign-ups, comments, logins.
- **Shops**: add to cart, checkout with a test payment, order emails.
- **The admin**: dashboard, editing and saving a post, the settings pages of your main plugins.
- **Scheduled tasks**: backups, emails and imports that run in the background (watch them over the next day).
- **The error log.** To log PHP messages in WordPress, set `WP_DEBUG` and `WP_DEBUG_LOG` to `true` in `wp-config.php` and keep `WP_DEBUG_DISPLAY` set to `false` so visitors don't see them. Messages go to `wp-content/debug.log` by default. See the official guide to [debugging in WordPress](https://developer.wordpress.org/advanced-administration/debug/debug-wordpress/), and turn debugging off again when you are done.

A few "Deprecated" lines in the log are normal and don't break anything. "Fatal error" lines need action.

## If something breaks

**"There has been a critical error on this website."** WordPress catches many fatal errors and emails the site's admin address a link to log in using [recovery mode](https://wordpress.org/documentation/article/recovery-mode/), with the plugin or theme that caused it paused. From there you can deactivate the plugin and keep the site running.

**Fastest fix: switch PHP back.** Go back to the version you wrote down in step 1. Your site returns to how it was, and you can investigate without pressure.

**Can't log in at all?** Connect with SFTP or your host's file manager, go to `wp-content/plugins/` and rename the folder of the plugin that causes the error (for example add `-off` to the name). WordPress deactivates a plugin whose folder it can't find. Rename it back later to restore it.

**Reading the error message.** The message names a file and line. The path tells you which plugin or theme it is: `wp-content/plugins/{plugin-folder}/...`. If the path points into a folder like `vendor/` or `lib/` inside a plugin, the problem may be an old library the plugin bundles: see [hidden libraries and PHP upgrades](/guides/hidden-libraries-php-upgrades/). For what common messages mean, see [what changed in PHP 8.3, 8.4 and 8.5](/guides/php-8-4-changes-wordpress/).

## If your host is upgrading PHP for you

Hosts sometimes announce that they will move sites to a newer PHP version on a set date. Don't wait for that date: run steps 1 to 6 yourself before it, on your own schedule. Ask your host whether you can choose the new version earlier and switch back if needed. [PHP 8.2 end of life and your hosting](/guides/php-8-2-end-of-life-hosting/) lists the questions worth asking.

## Managing many sites

If you look after several sites, do the check for all of them first and sort them into three groups: ready to upgrade, needs plugin updates, and blocked by a plugin with errors. Upgrade the ready group first, and spend your time on the blocked ones. Keep a simple spreadsheet per site: current PHP version, target version, blocking plugins, date upgraded.
