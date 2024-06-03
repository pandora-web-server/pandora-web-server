# Headers Module for Pingora

This crate allows defining additional HTTP headers to be sent for requests. It should be called
as request filter before other handlers such as `static-files-module` or `virtual-hosts-module`
to add headers to their responses. In order to add headers to upstream responses as well, the
handler’s `handle_response` method needs to be called during Pingora’s
`upstream_response_filter` or `response_filter` phase.

Each set of HTTP headers is paired with rules determining which host names and paths it applies
to. This is similar to how `virtual-hosts-module` works. This module is meant to be called
outside virtual hosts configuration and use its own rules however, to help set up a consistent
set of HTTP headers across the entire webspace.

A configuration could look like this:

```yaml
custom_headers:
- headers:
    Cache-Control: "max-age=604800"
    X-Custom-Header: "something"
  include: [example.com, example.net]
  exclude: [example.com/exception.txt]
- headers:
    Cache-Control: "max-age=3600"
    X-Another-Header: "something else"
  include: [example.com/dir/*]
```

This defines two sets of headers, the first applying to all of `example.com` and `example.net`
with the exception of the `example.com/exception.txt` file. The second set of headers applies
only to a single subdirectory.

Note that both sets include the `Cache-Control` header and both apply within the
`example.com/dir` subdirectory. In such cases the more specific rule is respected, here it is
the one applying to the specific subdirectory. This means that the shorter caching interval
will be used.

## Include/exclude rule format

The include and exclude rules can have the following format:

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
location. The other relevant scenario is when the same HTTP header is configured multiple times
with different values.

In such cases the more specific rule wins. A rule is considered more specific if:

1. It is bound to a specific host whereas the other rule is generic.
2. Hosts are identical but the rule is bound to a longer path.
3. Hosts and paths are identical but the rule applies to an exact path whereas the other rule
   matches everything within the path as well.
4. Everything is identical but the rule is an `exclude` rule whereas the other is an `include`
   rule.

## Code example

You would normally combine the handler of this module with the handlers of other modules. The
resulting configuration can then be merged with Pingora’s usual configuration:

```rust
use compression_module::CompressionHandler;
use headers_module::HeadersHandler;
use module_utils::{merge_conf, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
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
    server: ServerConf,
    handler: <Handler as RequestFilter>::Conf,
}

let opt = ServerOpt::from_args();
let conf = opt
    .conf
    .as_ref()
    .and_then(|path| Some(Conf::load_from_yaml(path).unwrap()))
    .unwrap_or_default();

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
        UpstreamHandler::upstream_peer(session, &mut ctx.upstream).await
    }

    fn upstream_response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) {
        self.handler.handle_response(session, upstream_response, ctx)
    }
}
```

Note that the handler needs to be called both during the `request_filter` phase (compile the
headers to be added and add them to handler responses if any) and during the
`upstream_response_filter` or `response_filter` phase (apply the headers to upstream
responses).

For complete code see [single-static-root example](https://github.com/palant/pingora-utils/tree/main/examples/single-static-root) and [virtual-hosts](https://github.com/palant/pingora-utils/tree/main/examples/virtual-hosts) examples in the repository.
