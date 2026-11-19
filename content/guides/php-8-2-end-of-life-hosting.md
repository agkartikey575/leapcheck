---
title: PHP 8.2 end of life: what your web host may do
description: When PHP 8.2 reaches end of life on 31 Dec 2026, hosts may upgrade sites, charge for old versions or keep them. What to expect and what to ask your host.
h1: PHP 8.2 end of life and your hosting: what happens, and what to ask
updated: 2026-11-19
---

**Short answer:** the PHP project stops releasing security fixes for PHP 8.2 on **31 December 2026** ([supported versions](https://www.php.net/supported-versions.php)). Nothing switches off on that date. What happens to your site next is decided by whoever runs your server: your host, or you. Hosts handle end-of-life PHP in different ways, and their own announcements are the only reliable source for what they will do. This guide explains the common approaches, so you know what to look for and what to ask.

For the dates and what "end of life" means in general, start with [PHP 8.2 end of life](/guides/php-8-2-end-of-life/).

## Who decides your PHP version?

It depends on how your site is hosted.

- **Shared hosting with a control panel:** the host installs the PHP versions; you usually choose one of them for your site in the panel. The host decides which versions stay on the list.
- **Managed WordPress hosting:** the host looks after the server for you. Hosts of this kind often set or change the PHP version themselves, sometimes with a choice for you in their dashboard.
- **A server you or your developer manage (VPS, dedicated, cloud):** you are responsible. PHP usually comes from the operating system's packages or from an extra package repository, and nobody will upgrade it for you.
- **Containers and platform services:** the PHP version is set in your configuration or base image. It changes when someone changes that setting.

Not sure which one you have? If you never log in to a server and only use a hosting dashboard, it is one of the first two.

## What hosts can do when a PHP version reaches end of life

There is no single industry rule. These are the approaches you may meet, often combined:

- **Keep offering it for a while.** The old version stays selectable after the PHP project's date, sometimes with a warning in the dashboard.
- **Move sites to a newer version automatically.** Usually announced in advance by email or on the host's status or blog pages, sometimes with a date per server. This is the case where unprepared sites break: the switch happens on the host's schedule, not yours.
- **Charge for continued access.** Some hosts offer the old version as a paid option, so customers who aren't ready can stay on it longer.
- **Rely on patches from someone else.** The PHP project stops, but some operating system vendors keep patching the PHP version they ship for the life of their own release. For example, Debian runs a [long-term support programme](https://wiki.debian.org/LTS) for its releases. A host using such packages may keep a "PHP 8.2" that still receives fixes, though only for that vendor's packages and under that vendor's policy.
- **Remove it from new sites first.** New sites can no longer pick the old version, while existing sites keep it until a later date.

Whatever your host does, the same two facts apply: your plugins and theme decide whether the newer version works, and checking before the switch puts you in control of the timing.

## Questions to ask your host

Look for an announcement first: an email from your host, a notice in your dashboard, or a help page about PHP versions. If you can't find one, ask support. These questions get you a clear answer:

1. "Until when will you offer PHP 8.2 on my plan?"
2. "Will you move my site to a newer version automatically? On what date, and to which version?"
3. "Can I choose the new version myself before that date, and switch back if something breaks?"
4. "Is there a staging copy I can test the new version on first?"
5. "If I need more time, is there an option to stay on 8.2, and does it cost extra?"
6. "If PHP 8.2 stays available after 31 December 2026, where do its security fixes come from?"
7. If you use features that need extra PHP extensions (for example a plugin that reads a mailbox over IMAP): "Do you provide the IMAP extension for PHP 8.4?" PHP 8.4 moved IMAP out of the standard build; see [what changed in PHP 8.4](/guides/php-8-4-changes-wordpress/).

Write down the answers and the date you got them. Plans and policies change, so re-check them before you rely on them.

## If you manage the server yourself

- Find out where your PHP comes from: the operating system's packages or an extra repository. Each source has its own support policy, which may differ from the dates on php.net.
- Check that source's policy for your exact operating system release and PHP version, and note when it ends.
- Plan the upgrade like any other change: test on a copy of the site, keep the old PHP version installed until you are sure, and know how to switch back.
- Remember the extensions. Each PHP version has its own set of installed extensions, so install the same ones for the new version that your site uses today. After switching, the **Status** tab in **Tools → Site Health** checks whether modules WordPress needs or recommends are missing.

## A timeline that works for most sites

- **Now:** [check which PHP version you run](/guides/how-to-check-php-version-wordpress/) and read your host's announcements.
- **Next:** check your plugins and theme against the version you want to move to. Look them up in our [plugin results](/plugins/), or [get the free LeapCheck plugin](/get/) to check everything installed on your site at once.
- **Then:** update what needs updating, and deal with any plugin that isn't ready (see [what to do when a plugin isn't compatible](/guides/php-8-4-wordpress-plugins-not-compatible/)).
- **Before your host's date:** switch to the new version yourself, on your schedule, with a backup and a way back. The full walkthrough is in [how to upgrade PHP on WordPress safely](/guides/upgrade-php-wordpress-safely/).

## If your host already upgraded and something broke

1. If your panel still lets you choose the old version, switch back. That gives you time to fix things without pressure.
2. If you can't switch back, use WordPress [recovery mode](https://wordpress.org/documentation/article/recovery-mode/): WordPress emails the site's admin address a link that lets you log in with the failing plugin or theme paused.
3. Find the plugin or theme from the error message, update it, and if it is still failing, follow [what to do when a plugin isn't compatible](/guides/php-8-4-wordpress-plugins-not-compatible/).
4. Contact your host's support team: tell them when the problem started and paste the error message.

## Frequently asked questions

### Is my site unsafe on 1 January 2027 if my host keeps PHP 8.2?

Not suddenly. The risk is that a security problem found in PHP 8.2 after 31 December 2026 gets no official fix from the PHP project. Unless your host gets fixes from another source, that risk grows over time. Ask your host (question 6 above).

### My host offers paid extended support for PHP 8.2. Is it worth it?

It can buy time if a plugin you depend on isn't ready yet. It doesn't fix the plugin. Use the time to update, replace or fix it, and set yourself a date to move on.

### Which version will my host move me to?

That is up to your host, and they should say so in their announcement. If you have a choice, pick a version you have checked your plugins against. [PHP 8.2 end of life](/guides/php-8-2-end-of-life/) compares PHP 8.3, 8.4 and 8.5.
