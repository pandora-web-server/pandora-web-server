# Upstream Module for Pingora

This crate helps configure Pingora’s upstream functionality. It is most useful in combination
with the `virtual-hosts-module` crate that allows applying multiple upstream configurations
conditionally.

Currently only one configuration option is provided: `upstream` (`--upstream` as command line
option). The value should be a URL like `http://127.0.0.1:8081` or `https://example.com`.

Supported URL schemes are `http://` and `https://`. Other than the scheme, only host name and
port are considered. Other parts of the URL are ignored if present.

The `UpstreamHandler` type has to be called in both `request_filter` and `upstream_peer`
Pingora Proxy phases. The former selects an upstream peer and modifies the request by adding
the appropriate `Host` header. The latter retrieves the previously selected upstream peer.

```rust
use async_trait::async_trait;
use upstream_module::UpstreamHandler;
use module_utils::RequestFilter;
use pingora_core::Error;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_proxy::{ProxyHttp, Session};

pub struct MyServer {
    upstream_handler: UpstreamHandler,
}

#[async_trait]
impl ProxyHttp for MyServer {
    type CTX = <UpstreamHandler as RequestFilter>::CTX;
    fn new_ctx(&self) -> Self::CTX {
        UpstreamHandler::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        // Select upstream peer according to configuration. This could be called based on some
        // conditions.
        self.upstream_handler.handle(session, ctx).await
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        // Return previously selected peer if any
        UpstreamHandler::upstream_peer(session, ctx).await
    }
}
```

To create a handler, you will typically read its configuration from a configuration file,
optionally combined with command line options. The following code will extend Pingora's usual
configuration file and command line options accordingly.

```rust
use upstream_module::{UpstreamConf, UpstreamHandler, UpstreamOpt};
use module_utils::{merge_conf, merge_opt, FromYaml};
use pingora_core::server::Server;
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use structopt::StructOpt;

#[merge_opt]
struct Opt {
    server: ServerOpt,
    upstream: UpstreamOpt,
}

#[merge_conf]
struct Conf {
    server: ServerConf,
    upstream: UpstreamConf,
}

let opt = Opt::from_args();
let mut conf = opt
    .server
    .conf
    .as_ref()
    .and_then(|path| Conf::load_from_yaml(path).ok())
    .unwrap_or_default();
conf.upstream.merge_with_opt(opt.upstream);

let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
server.bootstrap();

let upstream_handler: UpstreamHandler = conf.upstream.try_into().unwrap();
```

For complete and more realistic code see `virtual-hosts` example in the repository.
