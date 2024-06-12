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
//! This web server uses `virtual-hosts-module` crate to handle virtual hosts. Various modules like
//! `compression-module` and `static-files-module` crates are used for each individual virtual
//! host. Other modules like `custom-headers-module` are used at the top level and apply to all
//! virtual hosts. The configuration file looks like this:
//!
//! ```yaml
//! # Startup module settings (https://docs.rs/startup-module/latest/startup_module/struct.StartupConf.html)
//! listen:
//! - "[::]:8080"
//! daemon: false
//!
//! # Headers module settings (https://docs.rs/headers-module/latest/headers_module/struct.HeadersConf.html)
//! custom_headers:
//! - headers:
//!     Server: "My server is the best"
//!
//! # Virtual hosts settings:
//! # * https://docs.rs/virtual-hosts-module/latest/virtual_hosts_module/struct.VirtualHostsConf.html
//! # * https://docs.rs/log-module/latest/log_module/struct.LogConf.html
//! # * https://docs.rs/compression-module/latest/compression_module/struct.CompressionConf.html
//! # * https://docs.rs/auth-module/latest/auth_module/struct.AuthConf.html
//! # * https://docs.rs/rewrite-module/latest/rewrite_module/struct.RewriteConf.html
//! # * https://docs.rs/upstream-module/latest/upstream_module/struct.UpstreamConf.html
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
//! Example config files are provided in this directory. You can run this example with the
//! following command:
//!
//! ```sh
//! cargo run --package example-virtual-hosts -- -c config/*.yaml
//! ```
//!
//! To enable debugging output you can use the `RUST_LOG` environment variable:
//!
//! ```sh
//! RUST_LOG=debug cargo run --package example-virtual-hosts -- -c config/*.yaml
//! ```

use async_trait::async_trait;
use auth_module::AuthHandler;
use common_log_module::CommonLogHandler;
use compression_module::CompressionHandler;
use headers_module::HeadersHandler;
use ip_anonymization_module::IPAnonymizationHandler;
use log::error;
use module_utils::pingora::{Error, HttpPeer, ProxyHttp, ResponseHeader, Session};
use module_utils::{merge_conf, FromYaml, RequestFilter};
use rewrite_module::RewriteHandler;
use startup_module::{StartupConf, StartupOpt};
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
    anonymization: IPAnonymizationHandler,
    headers: HeadersHandler,
    virtual_hosts: VirtualHostsHandler<HostHandler>,
}

#[derive(Debug, RequestFilter)]
struct HostHandler {
    log: CommonLogHandler,
    compression: CompressionHandler,
    auth: AuthHandler,
    rewrite: RewriteHandler,
    upstream: UpstreamHandler,
    static_files: StaticFilesHandler,
}

/// The combined configuration of Pingora server and [`VirtualHostsHandler`].
#[merge_conf]
struct Conf {
    startup: StartupConf,
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

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, ctx: &mut Self::CTX) {
        if let Some(handler) = self.handler.virtual_hosts.as_inner(&ctx.virtual_hosts) {
            handler
                .log
                .logging(session, &mut ctx.virtual_hosts.log)
                .await;
        }
    }
}

fn main() {
    env_logger::init();

    let opt = StartupOpt::from_args();
    let conf = match Conf::load_from_files(opt.conf.as_deref().unwrap_or(&[])) {
        Ok(conf) => conf,
        Err(err) => {
            error!("{err}");
            Conf::default()
        }
    };

    let handler = match Handler::new(conf.handler) {
        Ok(handler) => handler,
        Err(err) => {
            error!("{err}");
            return;
        }
    };

    let server = conf
        .startup
        .into_server(VirtualHostsApp::new(handler), Some(opt));
    server.run_forever();
}
