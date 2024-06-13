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
//! This is a simple web server using `startup-module`, `anonymization-module`, `log-module`,
//! `compression-module`, `auth-module`, `rewrite-module`, `headers-module` and
//! `static-files-module` crates. It combines all their respective command line options and their
//! config file settings.
//!
//! An example config file is provided in this directory. You can run this example with the
//! following command:
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
use auth_module::{AuthHandler, AuthOpt};
use common_log_module::{CommonLogHandler, CommonLogOpt};
use compression_module::{CompressionHandler, CompressionOpt};
use headers_module::HeadersHandler;
use ip_anonymization_module::{IPAnonymizationHandler, IPAnonymizationOpt};
use log::error;
use module_utils::pingora::{Error, HttpPeer, ProxyHttp, ResponseHeader, Session};
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use rewrite_module::RewriteHandler;
use startup_module::{StartupConf, StartupOpt};
use static_files_module::{StaticFilesHandler, StaticFilesOpt};
use structopt::StructOpt;

/// The application implementing the Pingora Proxy interface
struct StaticRootApp {
    handler: Handler,
}

impl StaticRootApp {
    /// Creates a new application instance with the given handler.
    fn new(handler: Handler) -> Self {
        Self { handler }
    }
}

/// Handler combining Compression and Static Files modules
#[derive(Debug, RequestFilter)]
struct Handler {
    anonymization: IPAnonymizationHandler,
    log: CommonLogHandler,
    compression: CompressionHandler,
    auth: AuthHandler,
    rewrite: RewriteHandler,
    headers: HeadersHandler,
    static_files: StaticFilesHandler,
}

/// Run a web server exposing a single directory with static content.
#[merge_opt]
struct Opt {
    startup: StartupOpt,
    anonymization: IPAnonymizationOpt,
    log: CommonLogOpt,
    auth: AuthOpt,
    compression: CompressionOpt,
    static_files: StaticFilesOpt,
}

/// The combined configuration of Pingora server and [`StaticFilesHandler`].
#[merge_conf]
struct Conf {
    startup: StartupConf,
    handler: <Handler as RequestFilter>::Conf,
}

#[async_trait]
impl ProxyHttp for StaticRootApp {
    type CTX = <Handler as RequestFilter>::CTX;

    fn new_ctx(&self) -> Self::CTX {
        Handler::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        self.handler.call_request_filter(session, ctx).await
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        self.handler.call_upstream_peer(session, ctx).await
    }

    fn upstream_response_filter(
        &self,
        session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) {
        self.handler.call_response_filter(session, response, ctx)
    }

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, ctx: &mut Self::CTX) {
        self.handler.log.logging(session, &mut ctx.log).await;
    }
}

fn main() {
    env_logger::init();

    let opt = Opt::from_args();
    let mut conf = match Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])) {
        Ok(conf) => conf,
        Err(err) => {
            error!("{err}");
            Conf::default()
        }
    };

    conf.handler.anonymization.merge_with_opt(opt.anonymization);
    conf.handler.log.merge_with_opt(opt.log);
    conf.handler.auth.merge_with_opt(opt.auth);
    conf.handler.compression.merge_with_opt(opt.compression);
    conf.handler.static_files.merge_with_opt(opt.static_files);

    let handler = match Handler::new(conf.handler) {
        Ok(handler) => handler,
        Err(err) => {
            error!("{err}");
            return;
        }
    };

    let server = conf
        .startup
        .into_server(StaticRootApp::new(handler), Some(opt.startup));

    server.run_forever();
}
