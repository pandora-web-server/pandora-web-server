# Rewrite module for Pandora Web Server

The Rewrite module provides URI rewriting capabilities. Once rewritten, the URI is either passed on to other modules (internal redirect), or a redirect response is produced that makes the client emit a new request (external redirect). A number of rules can be defined in the configuration file, for example:

```yaml
rewrite_rules:
- from: /old.txt
  query_regex: "!noredirect"
  to: /file.txt
  type: permanent
- from: /view.php
  query_regex: "^file=large\\.txt$"
  to: /large.txt
- from: /images/*
  from_regex: "\\.jpg$"
  to: https://example.com${tail}
  type: redirect
```

## Rule precedence

If multiple rules potentially apply to a particular request, the rule with the longer path in the `from` field is applied. If multiple rules with the same path in `from` exist, exact matches are preferred over prefix matches.

## Variable interpolation

The redirect target defined in the `to` setting can contain variables that depending on the request will be replaced by different values. The supported variables are:

* `${tail}`: The part of the original path matched by `/*` in `from`
* `${query}`: The original query string including `?` if a query string is present
* `${http_<header>}`: The value of an HTTP request header, e.g. `${http_host}` will be replaced by the value of the `Host` header

## Configuration settings

| Configuration setting   | Type                  | Description |
|-------------------------|-----------------------|-------------|
| `rewrite_rules`         | list of [rewrite rules](#rewrite-rules) | A list of rules to apply to incoming requests |

### Rewrite rules

| Configuration setting   | Type               | Default value | Description |
|-------------------------|--------------------|---------------|-------------|
| `from`                  | string             | `/*`          | Restricts the rule to a specific path or path prefix (if the value ends with `/*`). |
| `from_regex`            | [regular expression](#regular-expressions) |               | Additional path-based restriction. Using `from` is preferred, it is more efficient. |
| `query_regex`           | [regular expression](#regular-expressions) |               | Restricts the rule to requests where the query string matches the regular expression. |
| `to`                    | URL                | `/`           | Redirect target, possibly containing [variables](#variable-interpolation) |
| `type`                  | `internal`, `redirect`, `permanent` | `internal` | Redirect type: either internal, `308 Permanent Redirect` response or `307 Temporary Redirect` response |

### Regular expressions

The [regular expression](https://en.wikipedia.org/wiki/Regular_expression) syntax implemented by the [regex crate](https://crates.io/crates/regex) is similar to other regular expression engines. Some features like lookahead and lookbehind are omitted for performance reasons.

Regular expressions are specified as strings in YAML. Prefixing the regular expression with `!` will negate its effect, only paths/query strings will be accepted then that *don’t* match the regular expression.
