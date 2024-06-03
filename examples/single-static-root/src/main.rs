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
//! This is a simple web server using `log-module`, `compression-module`, `auth-module`,
//! `rewrite-module`, `headers-module` and `static-files-module` crates. It combines their
//! respective command line options with the usual [Pingora command line options](Opt) and
//! their config file settings with [Pingora`s](ServerConf). In addition, it provides the following
//! setting:
//!
//! * `listen` (`--listen` as command line flag): A list of IP address/port combinations the server
//!   should listen on, e.g. `0.0.0.0:8080`.
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
use log::error;
use module_utils::{merge_conf, merge_opt, DeserializeMap, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::{Error, ErrorType};
use pingora_proxy::{http_proxy_service, ProxyHttp, Session};
use rewrite_module::RewriteHandler;
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
    log: CommonLogHandler,
    compression: CompressionHandler,
    auth: AuthHandler,
    rewrite: RewriteHandler,
    headers: HeadersHandler,
    static_files: StaticFilesHandler,
}

/// Command line options of this application
#[derive(Debug, StructOpt)]
struct StaticRootAppOpt {
    /// Address and port to listen on, e.g. "127.0.0.1:8080". This command line flag can be
    /// specified multiple times.
    #[structopt(short, long)]
    listen: Option<Vec<String>>,
    /// Use this flag to make the server run in the background.
    #[structopt(short, long)]
    daemon: bool,
    /// Test the configuration and exit. This is useful to validate the configuration before
    /// restarting the process.
    #[structopt(short, long)]
    test: bool,
    /// The path to the configuration file. This command line flag can be specified multiple times.
    #[structopt(short, long)]
    conf: Option<Vec<String>>,
}

/// Run a web server exposing a single directory with static content.
#[merge_opt]
struct Opt {
    app: StaticRootAppOpt,
    log: CommonLogOpt,
    auth: AuthOpt,
    compression: CompressionOpt,
    static_files: StaticFilesOpt,
}

/// Application-specific configuration settings
#[derive(Debug, Default, DeserializeMap)]
struct StaticRootAppConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080".
    listen: Vec<String>,
}

/// The combined configuration of Pingora server and [`StaticFilesHandler`].
#[merge_conf]
struct Conf {
    app: StaticRootAppConf,
    server: ServerConf,
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
        self.handler.handle(session, ctx).await
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        Err(Error::new(ErrorType::HTTPStatus(404)))
    }

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, ctx: &mut Self::CTX) {
        self.handler.log.logging(session, &mut ctx.log).await;
    }
}

fn main() {
    env_logger::init();

    let opt = Opt::from_args();
    let mut conf = match Conf::load_from_files(opt.app.conf.unwrap_or_default()) {
        Ok(conf) => conf,
        Err(err) => {
            error!("{err}");
            Conf::default()
        }
    };

    if conf.app.listen.is_empty() {
        // Make certain we have a listening address
        conf.app.listen.push("127.0.0.1:8080".to_owned());
        conf.app.listen.push("[::1]:8080".to_owned());
    }

    let mut server = Server::new_with_opt_and_conf(
        ServerOpt {
            daemon: opt.app.daemon,
            test: opt.app.test,
            upgrade: false,
            nocapture: false,
            conf: None,
        },
        conf.server,
    );
    server.bootstrap();

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

    let mut proxy = http_proxy_service(&server.configuration, StaticRootApp::new(handler));
    for addr in opt.app.listen.unwrap_or(conf.app.listen) {
        proxy.add_tcp(&addr);
    }
    server.add_service(proxy);

    server.run_forever();
}
