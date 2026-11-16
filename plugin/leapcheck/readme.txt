=== LeapCheck – PHP Upgrade Checker ===
Contributors: leapcheck
Tags: php, php 8, compatibility, upgrade, php version
Requires at least: 5.8
Tested up to: 7.1
Requires PHP: 7.4
Stable tag: 0.2.0
License: GPLv2 or later
License URI: https://www.gnu.org/licenses/gpl-2.0.html

Check before you leap: see which plugins and themes are ready for PHP 8.3, 8.4 and 8.5, including old libraries hidden inside them.

== Description ==

PHP 8.2 security support ends on 31 December 2026, and many hosts upgrade PHP for you. LeapCheck tells you, in seconds, whether your plugins and themes will keep working on PHP 8.3, 8.4 and 8.5.

* **Instant results, no timeouts.** Public plugin and theme code is scanned in advance on our servers with the open-source PHPCompatibility rules. Your site only downloads the ready-made result files, so nothing heavy runs on your hosting.
* **A clear answer:** "Safe to upgrade to PHP 8.4?" with a verdict per plugin and theme: compatible, issues (warnings), breaks, or not in the database yet.
* **Details when you need them:** file, line and message for each finding, plus the libraries bundled inside each plugin (for example an old copy of PHPMailer) and their versions.
* **PHP extensions your plugins need** (for example gd or intl), so you can make sure your host enables them on the new PHP version.
* **Library conflict warning:** if two active plugins bundle different versions of the same library, LeapCheck tells you, because PHP loads only one copy and your site may end up using the older one.
* **Old code that newer PHP skips** (guarded by a version or "function exists" check) is listed apart and never counted against a plugin.
* **Closest version:** if your exact version has not been scanned yet, LeapCheck shows the result for the closest scanned version, clearly labelled.
* **Opt-in and private by design.** Nothing is downloaded until you click the consent button. See the Privacy section.

Find it under **Tools → LeapCheck**.

Results come from static analysis. It cannot see everything (for example code built at runtime), so always test on a staging copy before upgrading. Custom and premium plugins that are not on WordPress.org show as "not in database yet".

== Installation ==

1. Install and activate the plugin.
2. Go to **Tools → LeapCheck**. You will see your plugins, themes and current PHP version.
3. Read the explanation and click **Yes, check my plugins and themes** to load the results.

== Frequently Asked Questions ==

= Does this plugin send data about my site? =

No. After you opt in, the plugin downloads one public file per plugin and theme. The file address contains only the plugin or theme folder name and its version number. No site address, user data, content or settings are sent. The plugin replaces the default WordPress user agent (which contains your site address) with a neutral one.

= Why does a plugin show "not in database yet"? =

It is probably a custom or premium plugin that is not on WordPress.org. If we scanned other versions of it, LeapCheck shows the result for the closest one instead, labelled "Result for version X (closest we have)".

= Why do I see a "Check remaining" button? =

To keep the page fast, LeapCheck downloads at most 25 result files per page load. Click the button to continue; results already downloaded are kept.

= How do I stop it? =

Click **Turn off checking and delete cached results** on the LeapCheck page, or deactivate the plugin. Deleting the plugin removes all its stored data.

== Privacy ==

LeapCheck does not track you and does not contact any server until an administrator opts in on the Tools → LeapCheck page.

After opt-in, the plugin makes plain HTTPS GET requests to the LeapCheck results service, one per installed plugin and per active (and parent) theme, for example:
`https://leapcheck.vercel.app/v1/wordpress-plugin/contact-form-7/5.1.json`

If that exact version is not in the database, it also requests the list of scanned versions for that plugin or theme (`.../contact-form-7/index.json`) and the result file of the closest scanned version. At most 25 requests are made per page load.

* What is sent: the plugin or theme slug and its version number, as part of the file address. Like any web request, the server sees the IP address of your web server.
* What is not sent: your site address, user data, content, settings or any identifier.
* What is stored on your site: your consent (date and the ID of the administrator who gave it) and cached results for 12 hours.
* You can withdraw consent at any time; this deletes the cached results. Uninstalling removes all LeapCheck data.

== External services ==

This plugin connects to the LeapCheck results service (static JSON files hosted at https://leapcheck.vercel.app) to show PHP compatibility results. It does so only after an administrator opts in, and only when the LeapCheck admin page is opened and no cached result exists. The data sent is described in the Privacy section above.

LeapCheck privacy policy: https://leapcheck.vercel.app/privacy/ . The service is hosted on Vercel; Vercel privacy policy: https://vercel.com/legal/privacy-policy

== Upgrade Notice ==

= 0.2.0 =
Shows needed PHP extensions, library conflicts between plugins, and closest-version results.

== Changelog ==

= 0.2.0 =
* New: PHP extensions needed by each plugin and theme, with a site-wide list to check with your host before upgrading.
* New: warning when two or more active plugins bundle different versions of the same PHP library.
* New: if your exact version was not scanned, show the result for the closest scanned version, clearly labelled.
* New: guarded legacy code (only runs on old PHP) is shown apart as "safe to ignore" and never counted.
* New: at most 25 downloads per page load, with a "Check remaining" button for large sites.
* Changed: the summary now says "Can't fully tell" when an item in use could not be checked exactly, even if others show warnings.

= 0.1.0 =
* First release: opt-in check of installed plugins and themes against the LeapCheck database for PHP 8.3, 8.4 and 8.5.
