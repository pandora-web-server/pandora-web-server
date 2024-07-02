# Headers module for Pandora Web Server

The Headers module allows adding HTTP headers to Pandora Web Server responses. It currently supports `Cache-Control` and `Content-Security-Policy` headers to be constructed in a structured way, other headers can be specified with a value to be sent verbatim.

Each set of header rules is paired with `include` and `exclude` settings determining which host names and paths it applies to. This is similar to how Virtual Hosts module works. This module is meant to be called outside virtual hosts configuration however, to help set up a consistent set of HTTP headers across the entire webspace.

A configuration could look like this:

```yaml
response_headers:
  cache_control:
  - no-storage: true
    include: example.com/caching_forbidden/*
  - max-age: 604800
    include: [example.com, example.net]
    exclude: example.com/caching_forbidden/*
  - max-age: 3600
    include: example.com/short_lived/*
  content_security_policy:
  - script-src: "'self'"
    frame-src:
    - "'self'"
    - https://example.com
    - https://example.info
  - script-src: https://cdn.example.com
    include: example.com/app/*
    exclude: example.com/app/admin/*
  custom:
    X-Custom-Header: "something"
    include: [example.com, example.net]
    exclude: example.com/exception.txt
```

This defines six sets of header rules, each applying to different sections of `example.com` and `example.net` websites.

## Conflict resolution

The headers defined by this module generally take precedence over existing headers. If for example an upstream response already contains a `Cache-Control` header, it will be replaced by the header value configured for this module.

Sometimes a configuration section for this module has different potentially applying values for a particular setting. In the example above this is the `max-age` setting with two different values applying to the `example.com/short_lived` subdirectory. In such cases, the rule with the higher [rule specificity](#rule-specificity) is chosen. Here it is the rule applying specifically to the subdirectory, so the shorter caching interval will be used.

Different configuration sections can potentially specify different values for the same module. For example, the `Cache-Control` header can be specified both via `cache_control` and `custom` settings. The values are then the combined as defined in [RFC 7230 section 3.2.2](https://datatracker.ietf.org/doc/html/rfc7230#section-3.2.2).

This module does *not* support multiple headers with the same name. This limitation should only be problematic for the `Set-Cookie` header, and this module isn’t the right tool for handling cookies.

## Rule specificity

Rule specificity becomes relevant whenever more than one rule applies to a particular host/path combination. That’s for example the case when both an `include` and an `exclude` rule match a location. The other relevant scenario is when the same HTTP header setting is configured
multiple times with different values.

In such cases the more specific rule wins. A rule is considered more specific if:

1. It is bound to a specific host whereas the other rule is generic.
2. Hosts are identical but the rule is bound to a longer path.
3. Hosts and paths are identical but the rule applies to an exact path whereas the other rule matches everything within the path as well.
4. Everything is identical but the rule is an `exclude` rule whereas the other is an `include` rule.

## Configuration settings

| Configuration setting   | Type                                                              |
|-------------------------|-------------------------------------------------------------------|
| `response_headers`      | [Response headers configuration](#response-headers-configuration) |

### Response headers configuration

| Configuration setting     | Type                                                                    |
|---------------------------|-------------------------------------------------------------------------|
| `cache_control`           | list of [Cache-Control rules](#cache-control-rules)                     |
| `content_security_policy` | list of [Content-Security-Policy rules](#content-security-policy-rules) |
| `custom`                  | list of [custom headers rules](#custom-headers-rules)                     |

### Cache-Control rules

These rules determine the value of the [Cache-Control HTTP header](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Cache-Control). They can contain the usual optional [`include` and `exclude` settings](#includeexclude-settings-format). In addition, the following settings will set the corresponding caching directives:

| Configuration setting     | Type    |
|---------------------------|---------|
| `max-age`                 | integer |
| `s-maxage`                | integer |
| `no-cache`                | boolean |
| `no-storage`              | boolean |
| `no-transform`            | boolean |
| `must-revalidate`         | boolean |
| `proxy-revalidate`        | boolean |
| `must-understand`         | boolean |
| `private`                 | boolean |
| `public`                  | boolean |
| `immutable`               | boolean |
| `stale-while-revalidate`  | integer |
| `stale-if-error`          | integer |

Note that only setting the boolean values to `true` will have an effect. Setting them to `false` will be ignored. If the intention is to omit a directive for a location, you should use the `exclude` setting.

### Content-Security-Policy rules

These rules determine the value of the [Content-Security-Policy HTTP header](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Security-Policy). They can contain the usual optional [`include` and `exclude` settings](#includeexclude-settings-format). In addition, the following settings will set the corresponding content policy directives:

| Configuration setting     | Type            |
|---------------------------|-----------------|
| `connect-src`             | list of strings |
| `default-src`             | list of strings |
| `fenced-frame-src`        | list of strings |
| `font-src`                | list of strings |
| `frame-src`               | list of strings |
| `img-src`                 | list of strings |
| `manifest-src`            | list of strings |
| `media-src`               | list of strings |
| `object-src`              | list of strings |
| `prefetch-src`            | list of strings |
| `script-src`              | list of strings |
| `script-src-elem`         | list of strings |
| `script-src-attr`         | list of strings |
| `style-src`               | list of strings |
| `style-src-elem`          | list of strings |
| `style-src-attr`          | list of strings |
| `worker-src`              | list of strings |
| `base-uri`                | list of strings |
| `sandbox`                 | list of strings |
| `form-action`             | list of strings |
| `frame-ancestors`         | list of strings |
| `report-uri`              | string          |
| `report-to`               | string          |
| `require-trusted-types-for` | list of strings |
| `trusted-types`           | list of strings |
| `upgrade-insecure-requests` | list of strings |

### Custom headers rules

These rules allow setting arbitrary HTTP response headers. They can contain the usual optional [`include` and `exclude` settings](#includeexclude-settings-format). All other settings present will be interpreted as a header name and its corresponding value.

In the unlikely scenario that you might need a response header named `include` or `exclude`, you can add the header as `Include` or `Exclude` to the configuration. Unlike setting names, HTTP header names are case-insensitive.

### Include/exclude settings format

The include and exclude settings can contain either a single value (a string) or a list with multiple values. The individual values have the following format:

* `""` (empty string): This matches everything. Putting this into the `include` list is equivalent to omitting it, applying to everything is the default behavior.
* `/path`: This matches only the specified path on all hosts. Note that `/path` and `/path/` are considered equivalent.
* `/path/*`: This matches the specified path on all hosts and everything contained within it such as `/path/subdir/file.txt`.
* `host`: This matches all paths on the specified host. It is equivalent to `host/*`.
* `host/path`: This matches only to the specified host/path combination. Note that `host/path` and `host/path/` are considered equivalent.
* `host/path/*`: This matches the specified host/path combination and everything contained within it such as `host/path/subdir/file.txt`.
