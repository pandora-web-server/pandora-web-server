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

//! # Startup Module for Pingora
//!
//! This crate helps configure and set up the Pingora server. It provides a [`StartupOpt`] data
//! structure with the relevant command line and [`StartupConf`] with the configuration file
//! options. Once these data structures are all set up, [`StartupConf::into_server`] method can be
//! used to get a Pingora server instance.
//!
//! ## Configuration
//!
//! The Startup Module currently exposes all of the
//! [Pingora configuration options](module_utils::pingora::ServerConf). In addition, it provides
//! a `listen` configuration option, a list of IP address/port combinations that the server should
//! listen on.
//!
//! The `listen` configuration option is also available as `--listen` command line option. Other
//! command line options are: `--conf` (configuration file or configuration files to load),
//! `--daemon` (run process in background) and `--test` (test configuration and exit).
//!
//! ## Code example
//!
//! ```rust
//! use async_trait::async_trait;
//! use module_utils::pingora::{Error, HttpPeer, ProxyHttp, Session};
//! use module_utils::FromYaml;
//! use startup_module::{StartupConf, StartupOpt};
//! use structopt::StructOpt;
//!
//! pub struct MyServer;
//!
//! #[async_trait]
//! impl ProxyHttp for MyServer {
//!     type CTX = ();
//!     fn new_ctx(&self) -> Self::CTX {}
//!
//!     async fn upstream_peer(
//!         &self,
//!         _session: &mut Session,
//!         _ctx: &mut Self::CTX,
//!     ) -> Result<Box<HttpPeer>, Box<Error>> {
//!         Ok(Box::new(HttpPeer::new(("example.com", 443), true, "example.com".to_owned())))
//!     }
//! }
//!
//! let opt = StartupOpt::from_args();
//! let conf = StartupConf::load_from_files(opt.conf.as_deref().unwrap_or(&[])).unwrap();
//! let server = conf.into_server(MyServer {}, Some(opt));
//!
//! // Do something with the server here, e.g. call server.run_forever()
//! ```
//!
//! For more comprehensive examples see the `examples` directory in the repository.

mod configuration;

use async_trait::async_trait;
pub use configuration::{StartupConf, StartupOpt};
use module_utils::pingora::{Error, HttpPeer, ProxyHttp, ResponseHeader, Session};
use module_utils::RequestFilter;

/// A trivial Pingora app implementation, to be passed to [`StartupConf::into_server`]
///
/// This app will only handle the `request_filter`, `upstream_peer`, `upstream_response_filter` and
/// `logging` phases. All processing will be delegated to the respective `RequestFilter` methods.
#[derive(Debug)]
pub struct DefaultApp<H> {
    handler: H,
}

impl<H> DefaultApp<H> {
    /// Creates a new app from a [`RequestFilter`] instance.
    pub fn new(handler: H) -> Self {
        Self { handler }
    }

    /// Creates a new app from a [`RequestFilter`] configuration.
    ///
    /// Any errors occurring when converting configuration to handler will be passed on.
    pub fn from_conf<C>(conf: C) -> Result<Self, Box<Error>>
    where
        H: RequestFilter<Conf = C> + TryFrom<C, Error = Box<Error>>,
    {
        Ok(Self::new(conf.try_into()?))
    }
}

#[async_trait]
impl<H> ProxyHttp for DefaultApp<H>
where
    H: RequestFilter + Sync,
    H::CTX: Send,
{
    type CTX = <H as RequestFilter>::CTX;

    fn new_ctx(&self) -> Self::CTX {
        H::new_ctx()
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

    async fn logging(&self, session: &mut Session, e: Option<&Error>, ctx: &mut Self::CTX) {
        self.handler.call_logging(session, e, ctx).await
    }
}
