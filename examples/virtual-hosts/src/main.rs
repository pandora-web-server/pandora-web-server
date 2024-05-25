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
//! This web server uses `virtual-hosts-module` crate to handle virtual hosts. The
//! `compression-module` and `static-files-module` crates are used for each individual virtual
//! host. The configuration file looks like this:
//!
//! ```yaml
//! # Application-specific settings
//! listen:
//! - "[::]:8080"
//!
//! # General server settings (https://docs.rs/pingora-core/0.2.0/pingora_core/server/configuration/struct.ServerConf.html)
//! daemon: false
//!
//! # Virtual hosts settings:
//! # * https://docs.rs/virtual-hosts-module/latest/virtual_hosts_module/struct.VirtualHostsConf.html
//! # * https://docs.rs/compression-module/latest/compression_module/struct.CompressionConf.html
//! # * https://docs.rs/static-files-module/latest/static_files_module/struct.StaticFilesConf.html
//! vhosts:
//!     localhost:8080:
//!         aliases:
//!         - 127.0.0.1:8080
//!         - "[::1]:8080"
//!         root: ./local-debug-root
//!     example.com:
//!         default: true
//!         compression_level: 3
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
use compression_module::CompressionHandler;
use headers_module::HeadersHandler;
use log::error;
use module_utils::pingora::{Error, HttpPeer, ResponseHeader, Session};
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use pingora_proxy::{http_proxy_service, ProxyHttp};
use rewrite_module::RewriteHandler;
use serde::Deserialize;
use static_files_module::StaticFilesHandler;
use structopt::StructOpt;
use upstream_module::UpstreamHandler;
use virtual_hosts_module::VirtualHostsHandler;

/// The application implementing the Pingora Proxy interface
struct VirtualHostsApp {
    handler: Handler,
}

impl VirtualHostsApp {
    /// Creates a new application instance with the given virtual hosts handler.
    fn new(handler: Handler) -> Self {
        Self { handler }
    }
}

#[derive(Debug, RequestFilter)]
struct Handler {
    headers: HeadersHandler,
    virtual_hosts: VirtualHostsHandler<HostHandler>,
}

#[derive(Debug, RequestFilter)]
struct HostHandler {
    compression: CompressionHandler,
    rewrite: RewriteHandler,
    upstream: UpstreamHandler,
    static_files: StaticFilesHandler,
}

/// Command line options of this application
#[derive(Debug, StructOpt)]
struct VirtualHostsAppOpt {
    /// Address and port to listen on, e.g. "127.0.0.1:8080". This command line flag can be
    /// specified multiple times.
    #[structopt(short, long)]
    listen: Option<Vec<String>>,
}

/// Run a web server exposing static content under several virtual hosts.
///
/// This application is based on pingora-proxy and virtual-hosts-module.
#[merge_opt]
struct Opt {
    app: VirtualHostsAppOpt,
    server: ServerOpt,
}

/// Application-specific configuration settings
#[derive(Debug, Deserialize)]
struct VirtualHostsAppConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080".
    listen: Vec<String>,
}

impl Default for VirtualHostsAppConf {
    fn default() -> Self {
        Self {
            listen: vec!["127.0.0.1:8080".to_owned(), "[::1]:8080".to_owned()],
        }
    }
}

/// The combined configuration of Pingora server and [`VirtualHostsHandler`].
#[merge_conf]
struct Conf {
    app: VirtualHostsAppConf,
    server: ServerConf,
    handler: <Handler as RequestFilter>::Conf,
}

#[async_trait]
impl ProxyHttp for VirtualHostsApp {
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
        UpstreamHandler::upstream_peer(session, &mut ctx.virtual_hosts.upstream).await
    }

    fn upstream_response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) {
        self.handler
            .handle_response(session, upstream_response, ctx)
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

    let handler = match Handler::new(conf.handler) {
        Ok(handler) => handler,
        Err(err) => {
            error!("{err}");
            return;
        }
    };

    let mut proxy = http_proxy_service(&server.configuration, VirtualHostsApp::new(handler));
    for addr in opt.app.listen.unwrap_or(conf.app.listen) {
        proxy.add_tcp(&addr);
    }
    server.add_service(proxy);

    server.run_forever();
}
