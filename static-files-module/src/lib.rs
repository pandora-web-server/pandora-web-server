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

//! # Static Files Module for Pingora
//!
//! This crate allows extending [Pingora Proxy](https://github.com/cloudflare/pingora) with the
//! capability to serve static files from a directory.
//!
//! ## Supported functionality
//!
//! * `GET` and `HEAD` requests
//! * Configurable directory index files (`index.html` by default)
//! * Page configurable to display on 404 Not Found errors instead of the standard error page
//! * Conditional requests via `If-Modified-Since`, `If-Unmodified-Since`, `If-Match`, `If-None`
//!   match HTTP headers
//! * Byte range requests via `Range` and `If-Range` HTTP headers
//! * Compression support: serving pre-compressed versions of the files (gzip, zlib deflate,
//!   compress, Brotli, Zstandard algorithms supported)
//! * Compression support: dynamic compression via Pingora (currently gzip, Brotli and Zstandard
//!   algorithms supported)
//!
//! ## Known limitations
//!
//! * Requests with multiple byte ranges are not supported and will result in the full file being
//!   returned. The complexity required for implementing this feature isn’t worth this rare use case.
//! * Zero-copy data transfer (a.k.a. sendfile) cannot currently be supported within the Pingora
//!   framework.
//!
//! ## Code example
//!
//! You will typically create a [`StaticFilesHandler`] instance and call it during the
//! `request_filter` stage. If configured and called unconditionally it will handle all requests
//! so that subsequent stages won’t be reached at all.
//!
//! ```rust
//! use async_trait::async_trait;
//! use pingora_core::Result;
//! use pingora_core::upstreams::peer::HttpPeer;
//! use pingora_proxy::{ProxyHttp, Session};
//! use module_utils::RequestFilter;
//! use static_files_module::StaticFilesHandler;
//!
//! pub struct MyServer {
//!     static_files_handler: StaticFilesHandler,
//! }
//!
//! #[async_trait]
//! impl ProxyHttp for MyServer {
//!     type CTX = <StaticFilesHandler as RequestFilter>::CTX;
//!     fn new_ctx(&self) -> Self::CTX {
//!         StaticFilesHandler::new_ctx()
//!     }
//!
//!     async fn request_filter(
//!         &self,
//!         session: &mut Session,
//!         ctx: &mut Self::CTX
//!     ) -> Result<bool> {
//!         self.static_files_handler.handle(session, ctx).await
//!     }
//!
//!     async fn upstream_peer(
//!         &self,
//!         _session: &mut Session,
//!         _ctx: &mut Self::CTX,
//!     ) -> Result<Box<HttpPeer>> {
//!         panic!("Unexpected, upstream_peer stage reached");
//!     }
//! }
//! ```
//!
//! You can create a `StaticFilesHandler` instance by specifying its configuration directly:
//!
//! ```rust,no_run
//! use static_files_module::{StaticFilesConf, StaticFilesHandler};
//!
//! let conf = StaticFilesConf {
//!     root: Some("/var/www/html".into()),
//!     ..Default::default()
//! };
//! let static_files_handler: StaticFilesHandler = conf.try_into().unwrap();
//! ```
//! It is also possible to create a configuration from command line options and a configuration
//! file, extending the default Pingora data structures. The macros
//! [`module_utils::merge_opt`] and [`module_utils::merge_conf`] help merging command
//! line options and configuration structures respectively, and [`module_utils::FromYaml`]
//! trait helps reading the configuration file.
//!
//! ```rust,no_run
//! use log::error;
//! use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
//! use pingora_core::server::Server;
//! use module_utils::{FromYaml, merge_opt, merge_conf};
//! use serde::Deserialize;
//! use static_files_module::{StaticFilesConf, StaticFilesHandler, StaticFilesOpt};
//! use std::fs::File;
//! use std::io::BufReader;
//! use structopt::StructOpt;
//!
//! // The command line flags from both structures are merged, so that the user doesn't need to
//! // care which structure they belong to.
//! #[merge_opt]
//! struct MyServerOpt {
//!     server: ServerOpt,
//!     static_files: StaticFilesOpt,
//! }
//!
//! // The configuration settings from both structures are merged, so that the user doesn't need to
//! // care which structure they belong to.
//! #[merge_conf]
//! struct MyServerConf {
//!     server: ServerConf,
//!     static_files: StaticFilesConf,
//! }
//!
//! let opt = MyServerOpt::from_args();
//! let conf = opt
//!     .server
//!     .conf
//!     .as_ref()
//!     .and_then(|path| MyServerConf::load_from_yaml(path).ok())
//!     .unwrap_or_else(MyServerConf::default);
//!
//! let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
//! server.bootstrap();
//!
//! let mut static_files_conf = conf.static_files;
//! static_files_conf.merge_with_opt(opt.static_files);
//! let static_files_handler: StaticFilesHandler = static_files_conf.try_into().unwrap();
//! ```
//!
//! For complete and more comprehensive code see `single-static-root` example in the repository.
//!
//! ## Compression support
//!
//! You can activate support for selected compression algorithms via the `precompressed` configuration setting:
//!
//! ```rust
//! use static_files_module::{CompressionAlgorithm, StaticFilesConf};
//!
//! let conf = StaticFilesConf {
//!     root: Some("/var/www/html".into()),
//!     precompressed: vec![CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli],
//!     ..Default::default()
//! };
//! ```
//!
//! This will make `StaticFilesHandler` look for gzip (`.gz`) and Brotli (`.br`) versions of the requested files and serve these pre-compressed files if supported by the client. For example, a client requesting `file.txt` and sending HTTP header `Accept-Encoding: br, gzip` will receive `file.txt.br` file or, if not found, `file.txt.gz` file. The order in which `StaticFilesHandler` will look for pre-compressed files is determined by the client’s compression algorithm preferences.
//!
//! It is also possible to compress files dynamically on the fly via Pingora’s downstream compression. For that, activate compression for the session before calling `StaticFilesHandler`:
//!
//! ```rust
//! # use async_trait::async_trait;
//! # use pingora_core::Result;
//! # use pingora_core::upstreams::peer::HttpPeer;
//! # use pingora_proxy::{ProxyHttp, Session};
//! # use module_utils::RequestFilter;
//! # use serde::Deserialize;
//! # use static_files_module::StaticFilesHandler;
//! #
//! # pub struct MyServer {
//! #     static_files_handler: StaticFilesHandler,
//! # }
//! #
//! # #[async_trait]
//! # impl ProxyHttp for MyServer {
//! #     type CTX = <StaticFilesHandler as RequestFilter>::CTX;
//! #     fn new_ctx(&self) -> Self::CTX {
//! #         StaticFilesHandler::new_ctx()
//! #     }
//! #
//! async fn request_filter(
//!     &self,
//!     session: &mut Session,
//!     ctx: &mut Self::CTX
//! ) -> Result<bool> {
//!     session.downstream_compression.adjust_level(3);
//!     self.static_files_handler.handle(session, ctx).await
//! }
//! #
//! #     async fn upstream_peer(
//! #         &self,
//! #         session: &mut Session,
//! #         _ctx: &mut Self::CTX,
//! #     ) -> Result<Box<HttpPeer>> {
//! #         panic!("Unexpected, upstream_peer stage reached");
//! #     }
//! # }
//! ```

mod compression;
mod compression_algorithm;
mod configuration;
mod file_writer;
mod handler;
pub mod metadata;
pub mod path;
pub mod range;
#[cfg(test)]
mod tests;

pub use compression_algorithm::{CompressionAlgorithm, UnsupportedCompressionAlgorithm};
pub use configuration::{StaticFilesConf, StaticFilesOpt};
pub use handler::StaticFilesHandler;
