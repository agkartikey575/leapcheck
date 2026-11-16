---
title: PHP 8.3, 8.4 and 8.5 changes that affect WordPress plugins
description: What changed in PHP 8.3, 8.4 and 8.5 that affects plugins and themes: deprecations vs real breakage, with examples and fixes from PHP's official notes.
h1: PHP 8.3, 8.4 and 8.5 for WordPress: what changed and what breaks
updated: 2026-11-16
---

Each PHP release changes a few things that old code relies on. Most of these changes are **deprecations**: the code still runs, but PHP warns that the feature will go away. A few are **real breakage**: the code stops working. This guide explains the difference, then lists the changes in PHP 8.3, 8.4 and 8.5 most likely to show up in WordPress plugins and themes, with fixes.

Everything here comes from the PHP project's own migration guides: [8.2](https://www.php.net/manual/en/migration82.php), [8.3](https://www.php.net/manual/en/migration83.php), [8.4](https://www.php.net/manual/en/migration84.php) and [8.5](https://www.php.net/manual/en/migration85.php). The complete technical lists are in the `UPGRADING` file of each release branch in the [PHP source repository](https://github.com/php/php-src). This page covers the changes most relevant to plugin code, not every change.

## Deprecated vs fatal error, in plain words

- **Deprecated** (`E_DEPRECATED`, "Deprecated: ..." in your log): PHP still runs the code exactly as before. It is a warning that this feature is planned for removal in a future version. Your site keeps working. If PHP messages are shown on screen, visitors may see the notices, which is why live sites should log them instead of displaying them.
- **Fatal error** ("Fatal error: Uncaught Error: ...", or "There has been a critical error on this website"): PHP stops running the request. The page, form or admin screen that hit that code fails.

A deprecation today is a likely error in a later version. Fixing deprecations early means the next upgrade is easy.

One important detail: a fatal error only happens when the broken code actually runs. A plugin can look fine on the home page and fail only when someone uses its export button or its settings page. That is why testing the features you use matters, and why reading the code (static analysis) catches problems that a quick click-through misses.

## Still common: PHP 8.2 deprecations

If your site has been on PHP 8.2, you may already see these in your log. They are still deprecations in 8.3, 8.4 and 8.5, not errors.

### Dynamic properties

PHP 8.2 deprecated creating a property on an object when the class doesn't declare it. For example, with an empty class `Settings`, the line `$s->color = 'blue';` logs *Creation of dynamic property Settings::$color is deprecated*.

Fix: declare the property (`public $color;`) in the class. If a class really needs arbitrary properties, mark it with the `#[\AllowDynamicProperties]` attribute. `stdClass` objects are not affected.

### Other PHP 8.2 deprecations you may meet

- `utf8_encode()` and `utf8_decode()` are deprecated. `mb_convert_encoding()` is the usual replacement.
- `"${var}"` and `"${expr}"` string interpolation is deprecated; use `"$var"` or `"{$var}"`.
- Callables like `"self::method"`, `"parent::method"` and `"static::method"` are deprecated; use an array callable such as `[self::class, 'method']`.

Details: [PHP 8.2 deprecated features](https://www.php.net/manual/en/migration82.deprecated.php).

## PHP 8.3

PHP 8.3 is a small step for most plugins. Things that can matter:

- Calling `get_class()` or `get_parent_class()` **without arguments** is deprecated. Use `self::class`, `static::class` or `get_class($this)` instead.
- `assert_options()` and the `ASSERT_*` constants are deprecated.
- `range()` is stricter: it now throws a `TypeError` when given objects, resources or arrays as boundaries, and a `ValueError` for a step of 0 or a negative step on an increasing range. Those are errors, not deprecations, but they only affect unusual code.

Details: [PHP 8.3 deprecated features](https://www.php.net/manual/en/migration83.deprecated.php) and [backward-incompatible changes](https://www.php.net/manual/en/migration83.incompatible.php).

## PHP 8.4

PHP 8.4 has the change you are most likely to see in plugin logs, plus one change that can cause real errors on some hosts.

### Implicitly nullable parameters (deprecated)

Older PHP code often makes a typed parameter optional by giving it a default of `null`, for example `function send_mail( string $to, array $headers = null )`. Before PHP 8.4, the `= null` default silently made the type accept `null` too.

In PHP 8.4 this is deprecated. The message looks like: *send_mail(): Implicitly marking parameter $headers as nullable is deprecated, the explicit nullable type must be used instead.*

Fix: make the type explicitly nullable with `?`, as in `function send_mail( string $to, ?array $headers = null )`.

The `?array` syntax has worked since PHP 7.1, so plugin authors can make this fix without dropping support for older PHP versions. The code keeps running on PHP 8.4 either way; this is a warning, not an error.

### Extensions moved out of PHP (can cause errors)

PHP 8.4 unbundled four extensions and moved them to PECL, the separate extension repository: **IMAP**, **OCI8**, **PDO_OCI** and **pspell**. They are no longer part of a standard PHP build.

What it means for you: if a plugin calls `imap_open()` (for example to read a mailbox for support tickets or email-to-post) or `pspell_*` functions (spell checking), and your host does not install the PECL extension for PHP 8.4, that code fails with *Call to undefined function imap_open()*. Well-written plugins check `function_exists()` first and show a message instead. Ask your host whether they provide the IMAP extension for PHP 8.4 before you upgrade if you rely on such a feature.

### E_STRICT

The `E_STRICT` error level was removed and the `E_STRICT` constant is deprecated. Old code such as `error_reporting( E_ALL & ~E_STRICT )` now triggers a deprecation notice. Fix: remove `E_STRICT` from the expression; it has no effect any more.

### Other PHP 8.4 deprecations

- Passing `E_USER_ERROR` to `trigger_error()` is deprecated; throw an exception or log and exit instead.
- `lcg_value()` is deprecated; plugins should switch to another random number function.
- `mysqli_kill()` and `mysqli::kill()` are deprecated.
- Changing the `session.sid_length` and `session.sid_bits_per_character` settings is deprecated.

Details: [PHP 8.4 deprecated features](https://www.php.net/manual/en/migration84.deprecated.php) and [backward-incompatible changes](https://www.php.net/manual/en/migration84.incompatible.php).

## PHP 8.5

PHP 8.5 (released 20 November 2025) adds a longer list of deprecations. The ones most likely in plugin code:

- **Old cast names**: `(boolean)`, `(integer)`, `(double)` and `(binary)` are deprecated. Use `(bool)`, `(int)`, `(float)` and `(string)`.
- **Backticks**: the backtick operator as a shortcut for `shell_exec()` is deprecated.
- **`null` as an array key**: using `null` as an array offset, or in `array_key_exists()`, is deprecated. Use an empty string `''` if that is what the code means.
- **`case` ending with a semicolon**: `case 1;` instead of `case 1:` is deprecated.
- **Freeing functions that do nothing now**: `curl_close()`, `imagedestroy()`, `finfo_close()` and `xml_parser_free()` are deprecated, because these objects are freed automatically. The fix is simply to delete the call (or guard it with a PHP version check if the plugin supports old PHP versions).
- **`$http_response_header`**: this automatically created variable is deprecated; use `http_get_last_response_headers()`.
- **`chr()` and `ord()`**: passing an integer outside 0 to 255 to `chr()`, or a string that is not exactly one byte to `ord()`, is deprecated.
- **`__sleep()` and `__wakeup()`** are soft-deprecated in favour of `__serialize()` and `__unserialize()`. "Soft" means documented as discouraged; it does not trigger a notice.

PHP 8.5 also removed the `disable_classes` setting, which is a server configuration matter for your host rather than plugin code.

Details: [PHP 8.5 deprecated features](https://www.php.net/manual/en/migration85.deprecated.php) and [backward-incompatible changes](https://www.php.net/manual/en/migration85.incompatible.php).

## What breaks hardest: code that never made it to PHP 8.0

The biggest failures on PHP 8.x usually don't come from the latest release. They come from code written for PHP 7 or earlier that has not been touched since, often inside an old library bundled in a plugin. PHP 8.0 removed, among other things:

- `create_function()` (use an anonymous function instead),
- `each()` (use `foreach`),
- curly-brace offsets like `$string{0}` (use `$string[0]`).

Code using these fails with a fatal error on any PHP 8 version as soon as the file is loaded. See the [PHP 8.0 backward-incompatible changes](https://www.php.net/manual/en/migration80.incompatible.php) and our guide to [hidden libraries in plugins](/guides/hidden-libraries-php-upgrades/).

## What to do with this

- **Site owners:** you don't need to fix code yourself. Check whether your plugins have errors or only warnings on your target version in our [plugin results](/plugins/), update them, and follow the [safe upgrade steps](/guides/upgrade-php-wordpress-safely/). Warnings alone are not a reason to delay an upgrade; errors are.
- **Plugin authors:** each finding on a plugin's results page includes the file, line and rule. Many PHP 8.4 and 8.5 deprecations have mechanical fixes like the ones above. Several of them, such as explicit `?type` parameters and the short cast names, also work on older PHP versions, so one code base can support both.
