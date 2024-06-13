# Upstream Module for Pingora

This crate helps configure Pingora’s upstream functionality. It is most useful in combination
with the `virtual-hosts-module` crate that allows applying multiple upstream configurations
conditionally.

Currently only one configuration option is provided: `upstream` (`--upstream` as command line
option). The value should be a URL like `http://127.0.0.1:8081` or `https://example.com`.

Supported URL schemes are `http://` and `https://`. Other than the scheme, only host name and
port are considered. Other parts of the URL are ignored if present.

## Code example

`UpstreamHandler` has to be called in both `request_filter` and `upstream_peer` phases. The
former selects an upstream peer and modifies the request by adding the appropriate `Host`
header. The latter retrieves the previously selected upstream peer. As such, this handler isn’t
suitable for `DefaultApp` defined in `startup-module` but requires an explicit `ProxyHttp`
implementation.

```rust
use async_trait::async_trait;
use module_utils::pingora::{Error, HttpPeer, ProxyHttp, Session};
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use startup_module::{StartupConf, StartupOpt};
use structopt::StructOpt;
use upstream_module::{UpstreamConf, UpstreamHandler, UpstreamOpt};

// Define configuration structures.

#[merge_conf]
struct Conf {
    startup: StartupConf,
    upstream: UpstreamConf,
}

#[merge_opt]
struct Opt {
    startup: StartupOpt,
    upstream: UpstreamOpt,
}

// Define server application

pub struct MyServer {
    handler: UpstreamHandler,
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
        self.handler.call_request_filter(session, ctx).await
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

// Startup

let opt = Opt::from_args();
let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
conf.upstream.merge_with_opt(opt.upstream);

let handler = UpstreamHandler::try_from(conf.upstream).unwrap();
let server = conf.startup.into_server(MyServer { handler }, Some(opt.startup));

// Do something with the server here, e.g. call server.run_forever()
```

For more realistic code see [`virtual-hosts` example in the repository](https://github.com/palant/pingora-utils/tree/main/examples/virtual-hosts).
