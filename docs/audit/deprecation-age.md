# Audit Deprecation Age

Homeboy audit flags `@deprecated X.Y.Z` docblock tags that are significantly older than the
component's current version. This surfaces ancient deprecations that are overdue for removal.

## How it works

1. Resolve the component's current version from either the WordPress plugin header (`Version: X.Y.Z`
   in a `*.php` file at the component root) or the `composer.json` `version` field.
2. Scan every file fingerprint for docblock tags matching `@deprecated X.Y.Z` (including the
   `@deprecated since X.Y.Z` variant and trailing prose).
3. Compare each tag to the current version. Flag when the deprecation is older than the threshold:
   - Current major strictly greater than the deprecated major, OR
   - Same major and `current.minor - deprecated.minor > 2`.
4. Count remaining references to the nearest following symbol (function, method, class, trait,
   interface) across `internal_calls` and `call_sites` from every fingerprint. Include that count
   in the finding description and suggestion.

Malformed `@deprecated` tags without a semver token are ignored. Tags within the threshold are also
ignored.

## Finding output

- `convention`: `deprecation_age`
- `kind`: `deprecation_age`
- `severity`: `info` (the paired fixer is plan-only; removal requires human review)

Each finding reports the line number, the tagged version, the current version, and the remaining
call-site count so reviewers can judge whether the deprecated symbol is safe to delete.
