# Static Files Module for Pingora

This crate allows extending [Pingora Proxy](https://github.com/cloudflare/pingora) with the
capability to serve static files from a directory.

## Supported functionality

* `GET` and `HEAD` requests
* Configurable directory index files (`index.html` by default)
* Page configurable to display on 404 Not Found errors instead of the standard error page
* Conditional requests via `If-Modified-Since`, `If-Unmodified-Since`, `If-Match`, `If-None`
  match HTTP headers
* Byte range requests via `Range` and `If-Range` HTTP headers
* Compression support: serving pre-compressed versions of the files (gzip, zlib deflate,
  compress, Brotli, Zstandard algorithms supported)
* Compression support: dynamic compression via Pingora (currently gzip, Brotli and Zstandard
  algorithms supported)

## Known limitations

* Requests with multiple byte ranges are not supported and will result in the full file being
  returned. The complexity required for implementing this feature isn’t worth this rare use case.
* Zero-copy data transfer (a.k.a. sendfile) cannot currently be supported within the Pingora
  framework.

## Code example

You will typically create a [`StaticFilesHandler`] instance and call it during the
`request_filter` stage. If called unconditionally it will handle all requests so that
subsequent stages won’t be reached at all.

```rust
use async_trait::async_trait;
use pingora_core::Result;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_proxy::{ProxyHttp, Session};
use pingora_utils_core::RequestFilter;
use static_files_module::StaticFilesHandler;

pub struct MyServer {
    static_files_handler: StaticFilesHandler,
}

#[async_trait]
impl ProxyHttp for MyServer {
    type CTX = <StaticFilesHandler as RequestFilter>::CTX;
    fn new_ctx(&self) -> Self::CTX {
        StaticFilesHandler::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX
    ) -> Result<bool> {
        self.static_files_handler.handle(session, ctx).await
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        panic!("Unexpected, upstream_peer stage reached");
    }
}
```

You can create a `StaticFilesHandler` instance by specifying its configuration directly:

```rust
use static_files_module::{StaticFilesConf, StaticFilesHandler};

let conf = StaticFilesConf {
    root: "/var/www/html".into(),
    ..Default::default()
};
let static_files_handler: StaticFilesHandler = conf.try_into().unwrap();
```
It is also possible to create a configuration from command line options and a configuration
file, extending the default Pingora data structures. The macros
`pingora_utils_core::merge_opt` and `pingora_utils_core::merge_conf` help merging command
line options and configuration structures respectively, and `pingora_utils_core::FromYaml`
trait helps reading the configuration file.

```rust
use log::error;
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use pingora_utils_core::{FromYaml, merge_opt, merge_conf};
use serde::Deserialize;
use static_files_module::{StaticFilesConf, StaticFilesHandler, StaticFilesOpt};
use std::fs::File;
use std::io::BufReader;
use structopt::StructOpt;

// The command line flags from both structures are merged, so that the user doesn't need to
// care which structure they belong to.
merge_opt!{
    struct MyServerOpt {
        server: ServerOpt,
        static_files: StaticFilesOpt,
    }
}

// The configuration settings from both structures are merged, so that the user doesn't need to
// care which structure they belong to.
merge_conf!{
    struct MyServerConf {
        server: ServerConf,
        static_files: StaticFilesConf,
    }
}

let opt = MyServerOpt::from_args();
let conf = opt
    .server
    .conf
    .as_ref()
    .and_then(|path| MyServerConf::load_from_yaml(path).ok())
    .unwrap_or_else(MyServerConf::default);

let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
server.bootstrap();

let mut static_files_conf = conf.static_files;
static_files_conf.merge_with_opt(opt.static_files);
let static_files_handler: StaticFilesHandler = static_files_conf.try_into().unwrap();
```

For complete and more comprehensive code see [single-static-root example](https://github.com/palant/pingora-utils/tree/main/examples/single-static-root) in the repository.

## Compression support

You can activate support for selected compression algorithms via the `precompressed` configuration setting:

```rust
use static_files_module::{CompressionAlgorithm, StaticFilesConf};

let conf = StaticFilesConf {
    root: "/var/www/html".into(),
    precompressed: vec![CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli],
    ..Default::default()
};
```

This will make `StaticFilesHandler` look for gzip (`.gz`) and Brotli (`.br`) versions of the requested files and serve these pre-compressed files if supported by the client. For example, a client requesting `file.txt` and sending HTTP header `Accept-Encoding: br, gzip` will receive `file.txt.br` file or, if not found, `file.txt.gz` file. The order in which `StaticFilesHandler` will look for pre-compressed files is determined by the client’s compression algorithm preferences.

It is also possible to compress files dynamically on the fly via Pingora’s downstream compression. For that, activate compression for the session before calling `StaticFilesHandler`:

```rust
async fn request_filter(
    &self,
    session: &mut Session,
    ctx: &mut Self::CTX
) -> Result<bool> {
    session.downstream_compression.adjust_level(3);
    self.static_files_handler.handle(session, ctx).await
}
```
