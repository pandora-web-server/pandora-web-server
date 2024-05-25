# Rewrite Module for Pingora

This crate adds URI rewriting capabilities. A modified URI can be passed on to further
processors, or a redirect response can make the client emit a new request. A number of rules
can be defined in the configuration file, for example:

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

## Rewrite rules

The following parameters can be defined for a rule:

* `from` restricts the rule to a specific path or a path prefix (if the value ends with `/*`).
* `from_regex` allows further refining the path restriction via a regular expression. Putting
  `!` before the regular expression makes the rule apply to paths *not* matched by the regular
  expression.
* `query_regex` restricts the rule to particular query strings only. Putting `!` before the
  regular expression makes the rule apply to query strings *not* matched by the regular
  expression.
* `to` is the new path and query string to be used if the rule is applied. Some variables will
  are replaced here:
  * `${tail}`: The part of the original path matched by `/*` in `from`
  * `${query}`: The original query string
  * `${http_<header>}`: The value of an HTTP header, e.g. `${http_host}` will be replaced by
    the value of the `Host` header
* `type` is the rewrite type, one of `internal` (default, internal redirect), `redirect`
  (temporary redirect) or `permanent` (permanent redirect)

If multiple rules potentially apply to a particular request, the rule with the longer path in
the `from` field is applied. If multiple rules with the same path in `from` exist, exact
matches are preferred over prefix matches.

## Code example

You would normally combine the handler of this module with the handlers of other modules such
as `static-files-module`. The resulting configuration can then be merged with Pingora’s usual
configuration:

```rust
use module_utils::{merge_conf, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use rewrite_module::RewriteHandler;
use static_files_module::StaticFilesHandler;
use structopt::StructOpt;

#[derive(Debug, RequestFilter)]
struct Handler {
    rewrite: RewriteHandler,
    static_files: StaticFilesHandler,
}

#[merge_conf]
struct Conf {
    server: ServerConf,
    handler: <Handler as RequestFilter>::Conf,
}

let opt = ServerOpt::from_args();
let conf = opt
    .conf
    .as_ref()
    .and_then(|path| Some(Conf::load_from_yaml(path).unwrap()))
    .unwrap_or_else(Conf::default);

let mut server = Server::new_with_opt_and_conf(opt, conf.server);
server.bootstrap();

let handler = Handler::new(conf.handler);
```

You can then use that handler in your server implementation:

```rust
use async_trait::async_trait;
use module_utils::RequestFilter;
use pingora_core::Error;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};

struct MyServer {
    handler: Handler,
}

#[async_trait]
impl ProxyHttp for MyServer {
    type CTX = <Handler as RequestFilter>::CTX;
    fn new_ctx(&self) -> Self::CTX {
        Handler::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        self.handler.handle(session, ctx).await
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        panic!("Upstream phase should not be reached");
    }
}
```

For complete code see [single-static-root example](https://github.com/palant/pingora-utils/tree/main/examples/single-static-root) and [virtual-hosts](https://github.com/palant/pingora-utils/tree/main/examples/virtual-hosts) examples in the repository.
