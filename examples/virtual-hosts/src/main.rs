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

//! # Virtual hosts example
//!
//! This web server uses `virtual-hosts-module` crate to handle virtual hosts and
//! `static-files-module` crate for each individual virtual host. The configuration file looks like
//! this:
//!
//! ```yaml
//! # Application-specific settings
//! listen:
//! - "[::]:8080"
//! compression_level: 3
//!
//! # General server settings (https://docs.rs/pingora-core/0.1.1/pingora_core/server/configuration/struct.ServerConf.html)
//! daemon: false
//!
//! # Virtual hosts settings (https://docs.rs/static-files-module/latest/static_files_module/struct.StaticFilesConf.html)
//! vhosts:
//!     localhost:8080:
//!         aliases:
//!         - 127.0.0.1:8080
//!         - "[::1]:8080"
//!         root: ./local-debug-root
//!     example.com:
//!         default: true
//!         root: ./production-root
//! ```
//!
//! An example config file is provided in this directory. You can run this example with the
//! following command:
//!
//! ```sh
//! cargo run --package example-virtual-hosts -- -c config.yaml
//! ```
//!
//! To enable debugging output you can use the `RUST_LOG` environment variable:
//!
//! ```sh
//! RUST_LOG=debug cargo run --package example-virtual-hosts -- -c config.yaml
//! ```

use async_trait::async_trait;
use log::error;
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::Error;
use pingora_proxy::{http_proxy_service, ProxyHttp, Session};
use serde::Deserialize;
use static_files_module::{StaticFilesConf, StaticFilesHandler};
use structopt::StructOpt;
use virtual_hosts_module::{VirtualHostsConf, VirtualHostsHandler};

type HostConf = StaticFilesConf;
type HostHandler = StaticFilesHandler;

/// The application implementing the Pingora Proxy interface
struct VirtualHostsApp {
    handler: VirtualHostsHandler<HostHandler>,
    compression_level: Option<u32>,
}

impl VirtualHostsApp {
    /// Creates a new application instance with the given virtual hosts handler.
    fn new(handler: VirtualHostsHandler<HostHandler>, compression_level: Option<u32>) -> Self {
        Self {
            handler,
            compression_level,
        }
    }
}

/// Command line options of this application
#[derive(Debug, StructOpt)]
struct VirtualHostsAppOpt {
    /// Address and port to listen on, e.g. "127.0.0.1:8080". This command line flag can be
    /// specified multiple times.
    #[structopt(short, long)]
    listen: Option<Vec<String>>,

    /// Compression level to be used for dynamic compression (omit to disable compression).
    #[structopt(long)]
    compression_level: Option<u32>,
}

merge_opt! {
    /// Run a web server exposing static content under several virtual hosts.
    ///
    /// This application is based on pingora-proxy and virtual-hosts-module.
    struct Opt {
        app: VirtualHostsAppOpt,
        server: ServerOpt,
    }
}

/// Application-specific configuration settings
#[derive(Debug, Deserialize)]
struct VirtualHostsAppConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080".
    listen: Vec<String>,

    /// Compression level to be used for dynamic compression (omit to disable compression).
    compression_level: Option<u32>,
}

impl Default for VirtualHostsAppConf {
    fn default() -> Self {
        Self {
            listen: vec!["127.0.0.1:8080".to_owned(), "[::1]:8080".to_owned()],
            compression_level: None,
        }
    }
}

merge_conf! {
    /// The combined configuration of Pingora server and [`VirtualHostsHandler`].
    struct Conf {
        app: VirtualHostsAppConf,
        server: ServerConf,
        virtual_hosts: VirtualHostsConf<HostConf>,
    }
}

#[async_trait]
impl ProxyHttp for VirtualHostsApp {
    type CTX = <VirtualHostsHandler<HostHandler> as RequestFilter>::CTX;

    fn new_ctx(&self) -> Self::CTX {
        VirtualHostsHandler::<HostHandler>::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        if let Some(level) = self.compression_level {
            session.downstream_compression.adjust_level(level);
        }
        self.handler.handle(session, ctx).await
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

fn main() {
    env_logger::init();

    let opt = Opt::from_args();
    let conf = opt
        .server
        .conf
        .as_ref()
        .and_then(|path| match Conf::load_from_yaml(path) {
            Ok(conf) => Some(conf),
            Err(err) => {
                error!("{err}");
                None
            }
        })
        .unwrap_or_else(Conf::default);

    let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
    server.bootstrap();

    let handler = match VirtualHostsHandler::new(conf.virtual_hosts) {
        Ok(handler) => handler,
        Err(err) => {
            error!("{err}");
            return;
        }
    };
    let compression_level = opt.app.compression_level.or(conf.app.compression_level);

    let mut proxy = http_proxy_service(
        &server.configuration,
        VirtualHostsApp::new(handler, compression_level),
    );
    for addr in opt.app.listen.unwrap_or(conf.app.listen) {
        proxy.add_tcp(&addr);
    }
    server.add_service(proxy);

    server.run_forever();
}
