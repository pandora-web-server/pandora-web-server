// Copyright 2024 Wladimir Palant
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! # Single static root example
//!
//! This is a simple web server using `static-files-module` crate. It combines the usual [Pingora command line options](Opt) with the [command line options of `static-files-module`](StaticFilesOpt) and the usual [Pingora config file settings](ServerConf) with the [config file settings of `static-files-module`](StaticFilesConf). In addition, it provides the following settings:
//!
//! * `listen` (`--listen` as command line flag): A list of IP address/port combinations the server should listen on, e.g. `0.0.0.0:8080`.
//! * `compression_level` (`--compression-level` as command line flag): If present, dynamic compression will be enabled and compression level set to the value provided for all algorithms (see [Pingora issue #228](https://github.com/cloudflare/pingora/issues/228)).
//!
//! An example config file is provided in this directory. You can run this example with the following command:
//!
//! ```sh
//! cargo run --package example-single-static-root -- -c config.yaml
//! ```
//!
//! To enable debugging output you can use the `RUST_LOG` environment variable:
//!
//! ```sh
//! RUST_LOG=debug cargo run --package example-single-static-root -- -c config.yaml
//! ```

use async_trait::async_trait;
use log::error;
use pingora_core::server::configuration::{Opt, ServerConf};
use pingora_core::server::Server;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::{Error, ErrorType};
use pingora_proxy::{http_proxy_service, ProxyHttp, Session};
use pingora_utils_core::FromYaml;
use serde::Deserialize;
use static_files_module::{StaticFilesConf, StaticFilesHandler, StaticFilesOpt};
use structopt::StructOpt;

/// The application implementing the Pingora Proxy interface
struct StaticRootApp {
    handler: StaticFilesHandler,
    compression_level: Option<u32>,
}

impl StaticRootApp {
    /// Creates a new application instance with the given static files handler.
    fn new(handler: StaticFilesHandler, compression_level: Option<u32>) -> Self {
        Self {
            handler,
            compression_level,
        }
    }
}

/// Run a web server exposing a single directory with static content.
///
/// This application is based on pingora-proxy and static-files-module.
#[derive(Debug, StructOpt)]
struct StaticRootAppOpt {
    /// Address and port to listen on, e.g. "127.0.0.1:8080". This command line flag can be
    /// specified multiple times.
    #[structopt(short, long)]
    listen: Option<Vec<String>>,

    /// Compression level to be used for dynamic compression (omit to disable compression).
    #[structopt(long)]
    compression_level: Option<u32>,

    #[structopt(flatten)]
    server: Opt,

    #[structopt(flatten)]
    static_files: StaticFilesOpt,

    /// This only exists to overwrite about text again because Opt overwrites it above. Yes, this
    /// is a [structopt bug](https://github.com/TeXitoi/structopt/issues/539).
    #[allow(dead_code)]
    #[structopt(flatten)]
    dummy: StructOptDummy,
}

/// Run a web server exposing a single directory with static content.
///
/// This application is based on pingora-proxy and static-files-module.
#[derive(Debug, StructOpt)]
struct StructOptDummy {}

/// The combined configuration of Pingora server and [`StaticFilesHandler`].
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
struct StaticRootAppConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080".
    listen: Vec<String>,

    /// Compression level to be used for dynamic compression (omit to disable compression).
    compression_level: Option<u32>,

    #[serde(flatten)]
    server: ServerConf,

    #[serde(flatten)]
    static_files: StaticFilesConf,
}

impl Default for StaticRootAppConf {
    fn default() -> Self {
        Self {
            listen: vec!["127.0.0.1:8080".to_owned(), "[::1]:8080".to_owned()],
            compression_level: None,
            server: Default::default(),
            static_files: Default::default(),
        }
    }
}

#[async_trait]
impl ProxyHttp for StaticRootApp {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        if let Some(level) = self.compression_level {
            session.downstream_compression.adjust_level(level);
        }
        self.handler.handle(session).await
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        Err(Error::new(ErrorType::HTTPStatus(404)))
    }
}

fn main() {
    env_logger::init();

    let opt = StaticRootAppOpt::from_args();
    let conf = opt
        .server
        .conf
        .as_ref()
        .and_then(|path| match StaticRootAppConf::load_from_yaml(path) {
            Ok(conf) => Some(conf),
            Err(err) => {
                error!("{err}");
                None
            }
        })
        .unwrap_or_else(StaticRootAppConf::default);

    let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
    server.bootstrap();

    let mut static_files_conf = conf.static_files;
    static_files_conf.merge_with_opt(opt.static_files);
    let handler = match StaticFilesHandler::new(static_files_conf) {
        Ok(handler) => handler,
        Err(err) => {
            error!("{err}");
            return;
        }
    };
    let compression_level = opt.compression_level.or(conf.compression_level);

    let mut proxy = http_proxy_service(
        &server.configuration,
        StaticRootApp::new(handler, compression_level),
    );
    for addr in opt.listen.unwrap_or(conf.listen) {
        proxy.add_tcp(&addr);
    }
    server.add_service(proxy);

    server.run_forever();
}
