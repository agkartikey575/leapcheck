---
title: Hidden libraries in plugins that block PHP upgrades
description: Plugins often ship their own copies of libraries like PHPMailer or PHPExcel. Why old bundled copies break on new PHP, and why two copies can clash.
h1: Hidden libraries: why old code bundled inside plugins blocks your PHP upgrade
updated: 2026-11-16
---

When a PHP upgrade breaks a site, the error often doesn't point at the plugin's own code. It points into a folder like `vendor/`, `lib/` or `includes/libraries/` inside the plugin. That is a **bundled library**: a copy of someone else's code that the plugin ships with. This guide explains why these copies are a common, hidden blocker for PHP upgrades, and what you can do about them.

## What a bundled library is

Plugins rarely write everything themselves. To send email, build spreadsheets, make HTTP requests or handle encryption, they use existing open-source libraries such as [PHPMailer](https://github.com/PHPMailer/PHPMailer), [PhpSpreadsheet](https://github.com/PHPOffice/PhpSpreadsheet) or [phpseclib](https://github.com/phpseclib/phpseclib). WordPress has no shared package manager for plugins, so each plugin includes its own copy of the library files inside its folder.

That copy is frozen at the version the author included. When the library's own developers fix a bug or a PHP compatibility problem, the copy inside the plugin doesn't change until the plugin author updates it and releases a new plugin version.

## Why old copies block PHP upgrades

### They were written for older PHP

A library copied into a plugin years ago was written for the PHP of its time. Libraries keep moving: their maintainers fix PHP 8 problems in new releases. The frozen copy doesn't get those fixes.

Two real examples, from the projects' own pages:

- **PHPExcel** was a popular library for reading and writing spreadsheets. Its [repository](https://github.com/PHPOffice/PHPExcel) says the project was officially deprecated in 2017 and permanently archived in 2019, and tells users to migrate to its successor, PhpSpreadsheet. PHPExcel's code uses curly-brace string offsets such as `$value{0}`, which PHP 8.0 [removed](https://www.php.net/manual/en/migration80.incompatible.php). On any PHP 8 version, loading those files ends in a fatal error.
- **PHPMailer 5.2**: the [PHPMailer project](https://github.com/PHPMailer/PHPMailer) states that version 5.2 "is no longer supported, even for security updates". Current versions are maintained and support current PHP versions.

### They only fail when used

A bundled library is often loaded only for one feature: an export button, a scheduled report, an email sent in the background. So the site can look perfectly fine after a PHP upgrade, and fail days later when someone clicks "Export to Excel" or a scheduled job runs. A quick click-through of your home page won't catch it.

### Deprecations pile up in them too

Even libraries that still work collect deprecation warnings with each PHP release, for example the [PHP 8.4 deprecation of implicitly nullable parameters](/guides/php-8-4-changes-wordpress/). In an old copy those warnings will never be fixed, and they become errors when PHP eventually removes the feature.

### Old JavaScript libraries carry known vulnerabilities

The same applies to JavaScript bundled in plugins, such as old copies of jQuery plugins. That doesn't affect your PHP upgrade, but old versions can have publicly known vulnerabilities. The open [RetireJS](https://github.com/RetireJS/retire.js) project tracks which versions of popular JavaScript libraries have known vulnerabilities.

## The conflict problem: two plugins, two versions

PHP has one important rule here: **a class name can only be defined once per request.** If two plugins each bundle the same library, both copies use the same class names. Only one copy can be loaded.

What happens then depends on how the plugins load the library:

- **No guard:** the second plugin tries to define a class that already exists, and PHP stops with a fatal error like *Cannot declare class ..., because the name is already in use*.
- **With a guard or an autoloader** (the usual case, for example plugins using Composer's autoloader or checking `class_exists()` first): no error, but **whichever copy loads first wins**, and *both* plugins use that version. Plugin B may get plugin A's old version, or a newer version with changes it doesn't expect.

This is why a PHP upgrade can break a plugin that "is compatible". The plugin's own copy of the library is fine, but on your site another plugin loads an older copy first, and that older copy is the one that fails on the new PHP version. It also means the problem can come and go depending on which plugins are active and in what order they load.

Plugin authors can avoid this by giving their bundled copy unique class names, usually by prefixing its namespace with tools such as [PHP-Scoper](https://github.com/humbug/php-scoper) or [Strauss](https://github.com/BrianHenryIE/strauss).

## WordPress's own rule for libraries it already ships

WordPress itself ships some libraries, including PHPMailer, jQuery and SimplePie. The WordPress.org plugin directory's [guideline 13](https://github.com/WordPress/wporg-plugin-guidelines/blob/trunk/guideline-13.md) says plugins "must use the versions of those libraries packaged with WordPress". So a plugin in the directory should not ship its own PHPMailer; it should use the copy that comes with WordPress, which is updated when you update WordPress. The rule covers only the libraries WordPress already includes. For everything else, plugins bundle their own copies.

## How LeapCheck finds hidden libraries

Each [plugin results page](/plugins/) has a "hidden libraries" section. We look for:

- **package manifests** inside the plugin, such as Composer's `vendor/composer/installed.json` or the `package.json` of bundled npm packages,
- **file signatures**: known file names and version strings of popular PHP libraries,
- **JavaScript files** matched against the RetireJS open data, which also gives the known-vulnerability count for that version.

The limits: we can only report libraries we have a signature for, a heavily modified copy may not be recognised, and a version string in a file can be wrong if the author edited the library. The method is described on [how we scan](/about/how-we-scan/).

## What to do

### As a site owner

1. Check your plugins on the [results pages](/plugins/), or check your whole site at once with the [free LeapCheck plugin](https://github.com/agkartikey575/leapcheck). Look at the hidden libraries section, not only the verdict.
2. Update the plugin. Authors often update bundled libraries in new releases.
3. If the latest version still bundles a library that fails on your target PHP version, tell the author in the plugin's support forum. Include the library name, version and path. That makes it easy to act on.
4. When an error message points into a library folder, note which plugin's folder it is in. If two plugins bundle the same library, try deactivating one of them on a staging copy to see whether the error changes.
5. Test the features that use the library (exports, email, imports) after upgrading PHP, not just the home page. See [how to upgrade PHP safely](/guides/upgrade-php-wordpress-safely/).

### As a plugin author

- Update bundled libraries to a release that supports the PHP versions you claim to support. Replace abandoned ones (PHPExcel with PhpSpreadsheet, for example).
- Use the libraries WordPress already ships instead of your own copy, as guideline 13 requires.
- Prefix your bundled libraries' namespaces so your copy can't clash with another plugin's.
- Remove libraries you no longer use. Unused files add risk and still show up in scans.
