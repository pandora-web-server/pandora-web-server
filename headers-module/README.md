# Headers Module for Pandora Web Server

This crate allows defining additional HTTP headers to be sent with responses. It should be
called before other handlers such as `static-files-module` or `virtual-hosts-module` to add
headers to their responses. In order to add headers to upstream responses as well, the
handler’s `call_response_filter` method needs to be called during Pingora’s
`upstream_response_filter` or `response_filter` phase. `DefaultApp` in `startup-module` handles
that automatically.

Each set of header rules is paired with rules determining which host names and paths it applies
to. This is similar to how `virtual-hosts-module` works. This module is meant to be called
outside virtual hosts configuration and use its own rules however, to help set up a consistent
set of HTTP headers across the entire webspace.

A configuration could look like this:

```yaml
response_headers:
    cache_control:
    -
        no-storage: true
        include: example.com/caching_forbidden/*
    -
        max-age: 604800
        include: [example.com, example.net]
        exclude: example.com/caching_forbidden/*
    -
        max-age: 3600
        include: example.com/short_lived/*
    content_security_policy:
    -
        script-src: "'self'"
        frame-src:
        - "'self'"
        - https://example.com
        - https://example.info
    -
        script-src: https://cdn.example.com
        include: example.com/app/*
        exclude: example.com/app/admin/*
    custom:
        X-Custom-Header: "something"
        include: [example.com, example.net]
        exclude: example.com/exception.txt
```

This defines six sets of header rules, each applying to different sections of `example.com`
and `example.net` websites. The `cache_control` section allows composing `Cache-Control` header
in a structured way, the `content_security_policy` section composes the
`Content-Security-Policy` header, and the `custom` section defines arbitrary header name and
value combinations as they should be sent to the client.

Note how two sets of rules define the `max-age` parameter for the `Cache-Control` header and
both apply within the `example.com/short_lived` subdirectory. In such cases the more specific
rule is respected, here it is the one applying to the specific subdirectory. This means that
the shorter caching interval will be used.

## Include/exclude rule format

The include and exclude rules can contain either a single rule (a string) or a list with
multiple rules. The individual rules have the following format:

* `""` (empty string): This rule applies to everything. Putting this into the `include` list is
  equivalent to omitting it, applying to everything is the default behavior.
* `/path`: This rule applies only to the specified path on all hosts. Note that `/path` and
  `/path/` are considered equivalent.
* `/path/*`: This rule applies to the specified path on all hosts and everything contained
  within it such as `/path/subdir/file.txt`.
* `host`: This rule applies to all paths on the specified host. It is equivalent to `host/*`.
* `host/path`: This rule applies only to the specified host/path combination. Note that `/path`
  and `/path/` are considered equivalent.
* `host/path/*`: This rule applies to the specified host/path combination and everything
  contained within it such as `host/path/subdir/file.txt`.

## Rule specificity

Rule specificity becomes relevant whenever more than one rule applies to a particular host/path
combination. That’s for example the case when both an `include` and an `exclude` rule match a
location. The other relevant scenario is when the same HTTP header setting is configured
multiple times with different values.

In such cases the more specific rule wins. A rule is considered more specific if:

1. It is bound to a specific host whereas the other rule is generic.
2. Hosts are identical but the rule is bound to a longer path.
3. Hosts and paths are identical but the rule applies to an exact path whereas the other rule
   matches everything within the path as well.
4. Everything is identical but the rule is an `exclude` rule whereas the other is an `include`
   rule.

## `cache_control` section

The `cache_control` section can contain the following boolean values: `no-cache`, `no-storage`,
`no-transform`, `must-revalidate`, `proxy-revalidate`, `must-understand`, `private`, `public`,
`immutable`. These should be set to `true` to be added to the `Cache-Control` header.

The following numeric settings are supported: `max-age`, `s-maxage`, `stale-while-revalidate`,
`stale-if-error`. These will be added to the `Cache-Control` header with the value configured.

## `content_security_policy`

The `content_security_policy` section contains settings corresponding to various
[`Content-Security-Policy` header directives](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Security-Policy).
Most of these are lists like `script-src`. These lists will be combined if different rules
apply to the same location and define different sources.

Reporting directives like `report-to` take a single value. If multiple values are possible, the
one from the closest rule takes precedence.

Finally, `upgrade-insecure-requests` directive is a boolean value. It should be set to `true`
to enable this directive in the output. Setting it to `false` has no effect.

## `custom` section

The `custom` section maps header names to header values. These headers will be sent to the
client verbatim.

In the unlikely scenario that you might need to send a header named `include` or `exclude`, you
can add the header as `Include` or `Exclude` to the configuration file. Unlike the rule
settings, header names are case-insensitive.

## A note on duplicate header values

This module does not support duplicate values for the same header name. Existing headers with
the same name produced by previous handlers (e.g. received from an upstream server) will be
overwritten. Rule processing within the `custom` section also makes sure that only the most
specific rule producing a particular header applies.

If multiple sections produce the same header name (e.g. `cache_control` section present and
`custom` section also defining a `Cache-Control` header), the values are combined as defined in
[RFC 7230 section 3.2.2](https://datatracker.ietf.org/doc/html/rfc7230#section-3.2.2).

The only header where this limitation might become problematic is `Set-Cookie`, and this module
isn’t the right tool for handling cookies.

## Code example

You would normally combine the handler of this module with the handlers of other modules. The
`pandora-module-utils` and `startup-module` crates provide helpers to simplify merging of
configuration and the command-line options of various handlers as well as creating a server
instance from the configuration.

`HeaderHandler` handles both the `request_filter` phase (compile the headers to be added and
add them to handler responses if any) and the `upstream_response_filter` or `response_filter`
phase (apply the headers to upstream responses). When `DefaultApp` is used, it will run
`handler.call_response_filter()` during the `upstream_response_filter` phase.

```rust
use compression_module::{CompressionHandler, CompressionOpt};
use headers_module::HeadersHandler;
use pandora_module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use startup_module::{DefaultApp, StartupConf, StartupOpt};
use upstream_module::UpstreamHandler;
use structopt::StructOpt;

#[derive(Debug, RequestFilter)]
struct Handler {
    compression: CompressionHandler,
    headers: HeadersHandler,
    upstream: UpstreamHandler,
}

#[merge_conf]
struct Conf {
    startup: StartupConf,
    handler: <Handler as RequestFilter>::Conf,
}

#[merge_opt]
struct Opt {
    startup: StartupOpt,
    compression: CompressionOpt,
}

let opt = Opt::from_args();
let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
conf.handler.compression.merge_with_opt(opt.compression);

let app = DefaultApp::<Handler>::from_conf(conf.handler).unwrap();
let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();

// Do something with the server here, e.g. call server.run_forever()
```
