# Static Files Module for Pingora

This crate allows extending [Pingora Proxy](https://github.com/cloudflare/pingora) with the capability to serve static files from a directory.

## Supported functionality

* `GET` and `HEAD` requests
* Configurable directory index files (`index.html` by default)
* Page configurable to display on 404 Not Found errors instead of the standard error page
* Conditional requests via `If-Modified-Since`, `If-Unmodified-Since`, `If-Match`, `If-None` match HTTP headers
* Byte range requests via `Range` and `If-Range` HTTP headers
* Compression support: serving pre-compressed versions of the files (gzip, zlib deflate, compress, Brotli, Zstandard algorithms supported)
* Compression support: dynamic compression via Pingora (currently gzip, Brotli and Zstandard algorithms supported)

## Known limitations

* Requests with multiple byte ranges are not supported and will result in the full file being returned. The complexity required for implementing this feature isn’t worth this rare use case.
* Zero-copy data transfer (a.k.a. sendfile) cannot currently be supported within the Pingora framework.

## Code example

You will typically create a `StaticFilesHandler` instance and call it during the `request_filter` stage. If called unconditionally it will handle all requests so that subsequent stages won’t be reached at all.

```rust
use async_trait::async_trait;
use pingora_core::Result;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_proxy::{ProxyHttp, Session};
use serde::Deserialize;
use static_files_module::StaticFilesHandler;

pub struct MyServer {
    static_files_handler: StaticFilesHandler,
}

#[async_trait]
impl ProxyHttp for MyServer {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX
    ) -> Result<bool> {
        self.static_files_handler.handle(session).await
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
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
let static_files_handler = StaticFilesHandler::new(conf);
```

It is also possible to create a configuration from command line options and configuration file, extending the default Pingora data structures. *Note*: Reading the configuration file is currently [more complicated than necessary](https://github.com/cloudflare/pingora/issues/232).

```rust
use log::error;
use pingora_core::server::configuration::{Opt, ServerConf};
use pingora_core::server::Server;
use static_files_module::{StaticFilesConf, StaticFilesHandler, StaticFilesOpt};
use std::fs::File;
use std::io::BufReader;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct MyServerOpt {
    // These are the default Pingora command line options. structopt(flatten) makes sure that these
    // are treated the same as top-level fields.
    #[structopt(flatten)]
    server: Opt,

    // These are the command line options specific to StaticFilesHandler.
    #[structopt(flatten)]
    static_files: StaticFilesOpt,
}

#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MyServerConf {
    // These are the default Pingora configuration file settings. serde(flatten) makes sure that
    // these are treated the same as top-level fields.
    #[serde(flatten)]
    server: ServerConf,

    // These are the configuration file settings specific to StaticFilesHandler.
    #[serde(flatten)]
    static_files: StaticFilesConf,
}

let opt = MyServerOpt::from_args();
let conf = opt
    .server
    .conf
    .as_ref()
    .and_then(|path| match File::open(path) {
        Ok(file) => Some(file),
        Err(err) => {
            error!("Failed opening configuration file: {err}");
            None
        }
    })
    .map(BufReader::new)
    .and_then(|reader| match serde_yaml::from_reader(reader) {
        Ok(conf) => Some(conf),
        Err(err) => {
            error!("Failed reading configuration file: {err}");
            None
        }
    })
    .unwrap_or_else(MyServerConf::default);

let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
server.bootstrap();

let mut static_files_conf = conf.static_files;
static_files_conf.merge_with_opt(opt.static_files);
let static_files_handler = StaticFilesHandler::new(static_files_conf);
```

For complete and more comprehensive code see [single-static-root example](../../../tree/main/examples/single-static-root) in the repository.

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
    _ctx: &mut Self::CTX
) -> Result<bool> {
    session.downstream_compression.adjust_level(3);
    self.static_files_handler.handle(session).await
}
```
