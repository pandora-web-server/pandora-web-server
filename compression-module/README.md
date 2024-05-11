# Compression Module for Pingora

This crate helps configure Pingora’s built-in compression mechanism. It provides two
configuration options:

* `compression_level` (`--compression-level` as command-line option): If present, will enable
  dynamic downstream compression and use the specified compression level (same level for all
  compression algorithms, see
  [Pingora issue #228](https://github.com/cloudflare/pingora/issues/228)).
* `decompress_upstream` (`--decompress-upstream` as command-line flag): If `true`,
  decompression of upstream responses will be enabled.

## Code example

You will usually want to merge Pingora’s command-line options and configuration settings with
the ones provided by this crate:

```rust
use compression_module::{CompressionConf, CompressionHandler, CompressionOpt};
use module_utils::{merge_conf, merge_opt, FromYaml};
use pingora_core::server::Server;
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use structopt::StructOpt;

#[merge_opt]
struct Opt {
    server: ServerOpt,
    compression: CompressionOpt,
}

#[merge_conf]
struct Conf {
    server: ServerConf,
    compression: CompressionConf,
}

let opt = Opt::from_args();
let mut conf = opt
    .server
    .conf
    .as_ref()
    .and_then(|path| Conf::load_from_yaml(path).ok())
    .unwrap_or_else(Conf::default);
conf.compression.merge_with_opt(opt.compression);

let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
server.bootstrap();

let compression_handler: CompressionHandler = conf.compression.try_into().unwrap();
```

You can then use that handler in your server implementation:

```rust
use async_trait::async_trait;
use compression_module::CompressionHandler;
use module_utils::RequestFilter;
use pingora_core::Error;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_proxy::{ProxyHttp, Session};

pub struct MyServer {
    compression_handler: CompressionHandler,
}

#[async_trait]
impl ProxyHttp for MyServer {
    type CTX = <CompressionHandler as RequestFilter>::CTX;
    fn new_ctx(&self) -> Self::CTX {
        CompressionHandler::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        // Enable compression according to settings
        self.compression_handler.handle(session, ctx).await
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        Ok(Box::new(HttpPeer::new(
            "example.com:443",
            true,
            "example.com".to_owned(),
        )))
    }
}
```
