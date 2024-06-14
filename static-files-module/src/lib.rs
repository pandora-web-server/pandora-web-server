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
//! You will typically create a [`StaticFilesHandler`] instance and make your server call it during
//! the `request_filter` stage. If configured and called unconditionally it will handle all requests
//! so that subsequent stages won’t be reached at all.
//!
//! The `module-utils` and `startup-modules` provide helpers to simplify merging of configuration
//! and the command-line options of various handlers as well as creating a server instance from the
//! configuration:
//!
//! ```rust
//! use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
//! use startup_module::{DefaultApp, StartupConf, StartupOpt};
//! use static_files_module::{StaticFilesConf, StaticFilesHandler, StaticFilesOpt};
//! use structopt::StructOpt;
//!
//! #[merge_conf]
//! struct Conf {
//!     startup: StartupConf,
//!     static_files: StaticFilesConf,
//! }
//!
//! #[merge_opt]
//! struct Opt {
//!     startup: StartupOpt,
//!     static_files: StaticFilesOpt,
//! }
//!
//! let opt = Opt::from_args();
//! let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
//! conf.static_files.merge_with_opt(opt.static_files);
//!
//! let app = DefaultApp::<StaticFilesHandler>::from_conf(conf.static_files).unwrap();
//! let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();
//!
//! // Do something with the server here, e.g. call server.run_forever()
//! ```
//!
//! For more comprehensive examples see the `examples` directory in the repository.
//!
//! ## Compression support
//!
//! You can activate support for selected compression algorithms via the `precompressed`
//! configuration setting, e.g. with this configuration file:
//!
//! ```yaml
//! root: /var/www/html
//! precompressed:
//! - gz
//! - br
//! ```
//!
//! This will make `StaticFilesHandler` look for gzip (`.gz`) and Brotli (`.br`) versions of the
//! requested files and serve these pre-compressed files if supported by the client. For example,
//! a client requesting `file.txt` and sending HTTP header `Accept-Encoding: br, gzip` will receive
//! `file.txt.br` file or, if not found, `file.txt.gz` file. The order in which
//! `StaticFilesHandler` will look for pre-compressed files is determined by the client’s
//! compression algorithm preferences.
//!
//! It is also possible to compress files dynamically on the fly via Pingora’s downstream
//! compression. For that, activate compression for the session before calling
//! `StaticFilesHandler`. The easiest way to achieve this is combining `StaticFilesHandler` with
//! `CompressionHandler` from `compression-module`. The latter will allow activating dynamic
//! compression via configuration file settings.
//!
//! ```rust
//! use compression_module::CompressionHandler;
//! use module_utils::RequestFilter;
//! use startup_module::DefaultApp;
//! use static_files_module::StaticFilesHandler;
//!
//! #[derive(Debug, RequestFilter)]
//! struct Handler {
//!     compression: CompressionHandler,
//!     static_files: StaticFilesHandler,
//! }
//!
//! let conf = <Handler as RequestFilter>::Conf::default();
//! let app = DefaultApp::<Handler>::from_conf(conf).unwrap();
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
