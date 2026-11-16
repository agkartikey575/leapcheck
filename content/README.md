# Site content

Hand-written pages for the public website. The site generator (`leapcheck site`) reads these files and renders them as HTML pages with the same layout as the rest of the site.

## Folders

- `guides/`: one Markdown file per guide. The file name (without `.md`) is the URL slug: `guides/php-8-2-end-of-life.md` is published at `/guides/php-8-2-end-of-life/`.

## File format

Each file starts with a front-matter block, then the body.

```
---
title: <title tag, at most 60 characters>
description: <meta description, at most 155 characters>
h1: <page heading>
updated: YYYY-MM-DD
---
<Markdown body>
```

Front matter rules:

- One `key: value` per line. The value is everything after the first `: ` on the line, so values may contain colons. No quotes, no multi-line values. This is not full YAML.
- All four keys are required. `title` is used as-is for the `<title>` tag; `updated` is shown on the page and used as the sitemap `lastmod` and the `dateModified` in structured data.

Body rules (the generator supports only this subset of Markdown):

- `##` and `###` headings (the `h1` comes from front matter; don't use `#` in the body).
- Paragraphs, separated by a blank line.
- Bullet lists (`- `) and numbered lists (`1. `), one level only.
- `**bold**`, `*italic*`, `` `inline code` `` and `[links](url)`.
- No HTML, images, tables or code blocks.

Links:

- Internal links are root-relative and end with a slash: `/plugins/`, `/plugins/{slug}/`, `/guides/{slug}/`, `/about/how-we-scan/`.
- External facts are cited inline with a link to the primary source (php.net, the PHP source repository, WordPress.org, the library's own project page).

## Writing rules

- Every factual claim must be checkable from the linked source. No invented numbers. Statistics from our own scans state the number of plugins scanned and the date.
- Plain, calm language. No hype and no fear. Say what is a warning and what is an error.
- Don't name a plugin in a negative way. Plugin names are used only descriptively.
- "WordPress" may be used descriptively in text and titles, never as part of our name or domain.
- Update the `updated` date whenever facts change, and re-check PHP dates against https://www.php.net/supported-versions.php.
