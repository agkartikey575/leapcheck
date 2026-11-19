---
title: Plugin not compatible with PHP 8.4? What to do next
description: A plugin shows errors on PHP 8.4. How to tell if it matters, update or ask the author, find an alternative, and test on staging before you upgrade.
h1: A plugin isn't ready for PHP 8.4: what to do
updated: 2026-11-19
---

You checked your plugins before upgrading PHP, and one of them shows problems on PHP 8.4. That doesn't mean you are stuck. This guide walks through the options in order, from the quickest to the most work: read the result, update, ask the author, pick a different target version, replace the plugin, or get the code fixed. Whatever you choose, test on a staging copy before you change your live site.

The same steps work for PHP 8.3 and 8.5, and for themes.

## Step 1: Read the result before you worry

Not every finding stops a plugin from working. There are two kinds:

- **Warnings (deprecations):** the plugin uses something PHP plans to remove later. On PHP 8.4 the code **still runs**. You may see "Deprecated" lines in your error log, but visitors won't notice anything if your site logs PHP messages instead of displaying them. A plugin with only warnings is not a reason to delay your upgrade.
- **Errors:** the plugin uses something that was removed or changed. When that code runs, PHP stops with a fatal error, and you may see "There has been a critical error on this website".

Then look at **where** the finding is:

- **Which feature?** An error only happens when the broken code actually runs. A problem in an export or import tool affects you only if you use that tool.
- **Which folder?** If the file path points into a folder like `vendor/` or `lib/` inside the plugin, the problem is in a library the plugin bundles. It may only load for one feature, and it may not load at all on your site. See [hidden libraries and PHP upgrades](/guides/hidden-libraries-php-upgrades/).

Automated checks read code without running it, so they can also be wrong in both directions. For example, code that is only used on older PHP versions can still be flagged. Each finding on our [plugin results](/plugins/) shows the file, line and rule, so you or a developer can check it. If you think a result is wrong, please report it from the results page.

For what common messages mean, see [what changed in PHP 8.3, 8.4 and 8.5](/guides/php-8-4-changes-wordpress/).

## Step 2: Update the plugin

Many compatibility problems are already fixed in newer versions. Update the plugin (and WordPress and your theme), then check again.

- **Look at the changelog.** On a plugin's WordPress.org page, the changelog often mentions PHP 8.x fixes.
- **Don't rely on "Requires PHP" alone.** That value on a plugin page is the *minimum* PHP version the plugin needs. It tells you nothing about the newest version it works on.
- **Premium plugins** are not in public databases and often need an active licence to receive updates. Check your account with the vendor, or ask them directly which PHP versions they test on.

## Step 3: Ask the author

If the latest version still shows errors on PHP 8.4, tell the author. For a plugin from the WordPress.org directory, use the **Support** tab on its plugin page; each plugin has its own forum there. For premium or custom plugins, use the vendor's support channel or contact the developer.

A good report gets a faster answer. Include:

- the plugin version you use,
- the PHP version you plan to move to,
- the message, file and line (copy them from the results page or from your error log),
- what you were doing when it happened, if you saw the error on a test site.

Keep it short and friendly. Most plugins are maintained by small teams or volunteers, and a precise report saves them time. Never post passwords, login details or private data in a public forum.

Give the author a reasonable time to reply, and in the meantime look at the next steps.

## Step 4: Pick a target version that buys you time

You don't have to jump to PHP 8.4 in one go. If a plugin you can't do without works on PHP 8.3 but not yet on 8.4, moving to 8.3 first keeps you on a supported version while you wait for a fix. PHP 8.3 gets security fixes until 31 December 2027, a year longer than PHP 8.2 (dates from the [PHP supported versions page](https://www.php.net/supported-versions.php)).

Check the plugin against PHP 8.3 the same way you checked it for 8.4. The trade-off: you will need to do another upgrade before the end of 2027. See [PHP 8.2 end of life](/guides/php-8-2-end-of-life/) for how to choose between 8.3, 8.4 and 8.5.

## Step 5: Consider an alternative

Replacing a plugin is more work, but it is the right call when the plugin is no longer maintained. Signs to look for on its WordPress.org page:

- **Closed:** the page shows a notice that the plugin has been closed and is not available for download. A closed plugin gets no updates through the directory.
- **Not updated for a long time:** check "Last updated" and whether the author still answers in the support forum.
- **"Tested up to"** is far behind the current WordPress version.

None of these prove the plugin is broken, but together they suggest nobody will fix it for a new PHP version.

When you compare replacements:

1. Write down exactly which features you use. You probably need fewer than the old plugin offers.
2. Check each candidate's results for your target PHP version, its recent updates and how its support forum is answered.
3. Find out how to move your data and settings. Some plugins store content in their own format.
4. Try the replacement on a staging copy first, with the old plugin deactivated.

Sometimes the feature can also come from your theme, from WordPress itself, or from a plugin you already have.

## Step 6: Get the code fixed

Plugins in the WordPress.org directory must be licensed under the GPL or a compatible licence ([guideline 1](https://github.com/WordPress/wporg-plugin-guidelines/blob/trunk/guideline-01.md)), so you are allowed to change the code or have a developer change it. Many PHP 8.4 fixes are small and mechanical; [what changed in PHP 8.3, 8.4 and 8.5](/guides/php-8-4-changes-wordpress/) shows several.

Be aware of the catch: **updating a plugin replaces its files**, so your own changes are lost at the next update. If you patch a plugin, send the fix to the author too, so it ends up in an official release. Otherwise you take over looking after that copy.

## Step 7: Test on staging, then upgrade with a way back

Whatever you decided, try it on a copy before your live site:

1. Make a backup you know you can restore.
2. Create a staging copy if your host offers one.
3. Switch the staging copy to your target PHP version.
4. Use the features of the plugin you were worried about, not just the home page.
5. Watch the error log for "Fatal error" lines.

The full checklist, including how to roll back, is in [how to upgrade PHP on WordPress safely](/guides/upgrade-php-wordpress-safely/).

## Check the rest of your site

One plugin with a problem is often not the only one. Check all plugins and your theme against the same target version: look them up in our [plugin results](/plugins/), or [get the free LeapCheck plugin](/get/) to see every installed plugin and theme in one list.

## Frequently asked questions

### My host is upgrading PHP before the plugin is fixed. What can I do?

Ask your host whether you can stay on your current version for a while, or choose PHP 8.3 instead of 8.4. If neither is possible, deactivate or replace the plugin before the date. See [what hosts do when PHP versions reach end of life](/guides/php-8-2-end-of-life-hosting/).

### The plugin only shows warnings. Should I wait?

No need. Warnings mean the code still runs on that version. Upgrade, keep PHP messages logged rather than displayed, and let the author know so the warnings are fixed before a later PHP version turns them into errors.

### Can I just turn off the warnings?

Your live site should not display PHP messages to visitors anyway. The [WordPress debugging guide](https://developer.wordpress.org/advanced-administration/debug/debug-wordpress/) explains the `WP_DEBUG` settings. Hiding warnings doesn't fix anything; it only keeps your screen clean while the author works on it.
