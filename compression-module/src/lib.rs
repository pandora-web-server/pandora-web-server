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

//! # Compression Module for Pingora
//!
//! This crate helps configure Pingora’s built-in compression mechanism. It provides two
//! configuration options:
//!
//! * `compression_level` (`--compression-level` as command-line option): If present, will enable
//!   dynamic downstream compression and use the specified compression level (same level for all
//!   compression algorithms, see
//!   [Pingora issue #228](https://github.com/cloudflare/pingora/issues/228)).
//! * `decompress_upstream` (`--decompress-upstream` as command-line flag): If `true`,
//!   decompression of upstream responses will be enabled.
//!
//! ## Code example
//!
//! You will usually want to merge Pingora’s command-line options and configuration settings with
//! the ones provided by this crate:
//!
//! ```rust
//! use compression_module::{CompressionConf, CompressionHandler, CompressionOpt};
//! use module_utils::{merge_conf, merge_opt, FromYaml};
//! use pingora_core::server::Server;
//! use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
//! use structopt::StructOpt;
//!
//! merge_opt! {
//!     struct Opt {
//!         server: ServerOpt,
//!         compression: CompressionOpt,
//!     }
//! }
//!
//! merge_conf! {
//!     struct Conf {
//!         server: ServerConf,
//!         compression: CompressionConf,
//!     }
//! }
//!
//! let opt = Opt::from_args();
//! let mut conf = opt
//!     .server
//!     .conf
//!     .as_ref()
//!     .and_then(|path| Conf::load_from_yaml(path).ok())
//!     .unwrap_or_else(Conf::default);
//! conf.compression.merge_with_opt(opt.compression);
//!
//! let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
//! server.bootstrap();
//!
//! let compression_handler: CompressionHandler = conf.compression.try_into().unwrap();
//! ```
//!
//! You can then use that handler in your server implementation:
//!
//! ```rust
//! use async_trait::async_trait;
//! use compression_module::CompressionHandler;
//! use module_utils::RequestFilter;
//! use pingora_core::Error;
//! use pingora_core::upstreams::peer::HttpPeer;
//! use pingora_proxy::{ProxyHttp, Session};
//!
//! pub struct MyServer {
//!     compression_handler: CompressionHandler,
//! }
//!
//! #[async_trait]
//! impl ProxyHttp for MyServer {
//!     type CTX = <CompressionHandler as RequestFilter>::CTX;
//!     fn new_ctx(&self) -> Self::CTX {
//!         CompressionHandler::new_ctx()
//!     }
//!
//!     async fn request_filter(
//!         &self,
//!         session: &mut Session,
//!         ctx: &mut Self::CTX,
//!     ) -> Result<bool, Box<Error>> {
//!         // Enable compression according to settings
//!         self.compression_handler.handle(session, ctx).await
//!     }
//!
//!     async fn upstream_peer(
//!         &self,
//!         _session: &mut Session,
//!         _ctx: &mut Self::CTX,
//!     ) -> Result<Box<HttpPeer>, Box<Error>> {
//!         Ok(Box::new(HttpPeer::new(
//!             "example.com:443",
//!             true,
//!             "example.com".to_owned(),
//!         )))
//!     }
//! }
//! ```

use async_trait::async_trait;
use module_utils::{RequestFilter, RequestFilterResult};
use pingora_core::Error;
use pingora_proxy::Session;
use serde::Deserialize;
use structopt::StructOpt;

/// Command line options of the compression module
#[derive(Debug, Default, StructOpt)]
pub struct CompressionOpt {
    /// Compression level to be used for dynamic compression (omit to disable compression)
    #[structopt(long)]
    pub compression_level: Option<u32>,

    /// Decompress upstream responses before passing them on
    #[structopt(long)]
    pub decompress_upstream: bool,
}

/// Configuration settings of the compression module
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CompressionConf {
    /// Compression level to be used for dynamic compression (omit to disable compression).
    pub compression_level: Option<u32>,

    /// If `true`, upstream responses will be decompressed
    pub decompress_upstream: bool,
}

impl CompressionConf {
    /// Merges the command line options into the current configuration. Any command line options
    /// present overwrite existing settings.
    pub fn merge_with_opt(&mut self, opt: CompressionOpt) {
        if opt.compression_level.is_some() {
            self.compression_level = opt.compression_level;
        }

        if opt.decompress_upstream {
            self.decompress_upstream = opt.decompress_upstream;
        }
    }
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug)]
pub struct CompressionHandler {
    conf: CompressionConf,
}

impl TryFrom<CompressionConf> for CompressionHandler {
    type Error = Box<Error>;

    fn try_from(conf: CompressionConf) -> Result<Self, Self::Error> {
        Ok(Self { conf })
    }
}

#[async_trait]
impl RequestFilter for CompressionHandler {
    type Conf = CompressionConf;
    type CTX = ();
    fn new_ctx() -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        if let Some(level) = self.conf.compression_level {
            session.downstream_compression.adjust_level(level);
        }

        if self.conf.decompress_upstream {
            session.upstream_compression.adjust_decompression(true);
        }

        Ok(RequestFilterResult::Unhandled)
    }
}
