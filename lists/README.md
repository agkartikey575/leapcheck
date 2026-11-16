# Slug lists

The nightly scan (`.github/workflows/scan.yml`) reads these JSON files. By default it uses
`lists/top-plugins.json` and, if the file exists, `lists/top-themes.json`. A manual run can pass other
lists (space-separated paths, input `list`); a list that does not exist is skipped with a notice.

## Format

A JSON array. Each entry is either a plain string (a WordPress.org **plugin** slug) or an object:

```json
[
  "akismet",
  {"slug": "contact-form-7", "kind": "plugin"},
  {"slug": "contact-form-7", "version": "5.1"},
  {"slug": "twentytwentyfive", "kind": "theme"}
]
```

| Field | Required | Meaning |
|---|---|---|
| `slug` | yes | WordPress.org slug: `^[a-z0-9][a-z0-9-]{0,199}$` |
| `kind` | no | `plugin` (default) or `theme`. Theme entries must set `"kind": "theme"`; plain strings are always plugins. |
| `version` | no | Pin one version (`^[A-Za-z0-9._-]{1,50}$`). Without it, the latest version on WordPress.org is scanned. |

Other fields (for example `active_installs`) are ignored by the scan and can be kept for reference.
Invalid entries are skipped with a warning; duplicates are scanned once.

## What gets scanned

Each run scans only what is not published yet: an entry is skipped when
`/v1/wordpress-{plugin|theme}/{slug}/{version}.json` already exists on the site. Entries that are closed or
removed on WordPress.org are not scanned. After an engine or rules change, or with the `force_full` input,
everything in the lists is rescanned. Details: `docs/ops/runbook.md`, section 2.

## Sources

Lists contain only public WordPress.org slugs, taken from the official WordPress.org API
(`api.wordpress.org/plugins/info/1.2/` and `api.wordpress.org/themes/info/1.2/`, browse "popular").
No personal data.
