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

//! # Common Log Module for Pingora
//!
//! This crate implements the creation of access log files in the
//! [Common Log Format](https://en.wikipedia.org/wiki/Common_Log_Format) that can be processed
//! further by a variety of tools. A configuration could look like this:
//!
//! ```yaml
//! log_file: access.log
//! log_format: [
//!     remote_addr, -, -, time_local, request, status, bytes_sent, http_referer, http_user_agent
//! ]
//! ```
//!
//! The `log_file` field is also available as `--log-file` command line option.
//!
//! The supported fields for the `log_format` setting are:
//!
//! * `-`: Verbatim `-` character (for unsupported fields)
//! * `remote_addr`: client’s IP address
//! * `remote_port`: client’s TCP port
//! * `time_local`: date and time of the request, e.g. `[10/Oct/2000:13:55:36 -0700]`
//! * `time_iso8601`: date and time in the ISO 8601 format, e.g. `[2000-10-10T13:55:36-07:00]`
//! * `request`: quoted request line, e.g. `"GET / HTTP/1.1"`
//! * `status`: status code of the response, e.g. `200`
//! * `bytes_sent`: number of bytes sent as response
//! * `processing_time`: time from request being received to response in milliseconds
//! * `http_<header>`: quoted value of an HTTP request header. For example, `http_user_agent` adds
//!   the value of the `User-Agent` HTTP header to the log.
//! * `sent_http_<header>`: quoted value of an HTTP response header. For example,
//!   `sent_http_content_type` adds the value of the `Content-Type` HTTP header to the log.
//!
//! This module will add one line per request to the log file. A log file will be created if
//! necessary, data in already existing files will be kept.
//!
//! Multiple log files are possible via `virtual-hosts-module` for example. Adding Common Log
//! Module to its host handler will make sure that each virtual host has its own logging
//! configuration.
//!
//! On Unix-based systems, the process can be sent a `HUP` or `USR1` signal to make it re-open log
//! files. This is useful after the logs have been rotated for example.
//!
//! ## Code example
//!
//! `CommonLogHandler` first handles the `request_filter` phase where it captures relevant data
//! before it has been altered. Later the actual logging is performed during the `logging` phase.
//!
//! ```rust
//! use common_log_module::{CommonLogHandler, CommonLogOpt};
//! use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
//! use startup_module::{DefaultApp, StartupConf, StartupOpt};
//! use static_files_module::StaticFilesHandler;
//! use structopt::StructOpt;
//!
//! #[derive(Debug, RequestFilter)]
//! struct Handler {
//!     log: CommonLogHandler,
//!     static_files: StaticFilesHandler,
//! }
//!
//! #[merge_conf]
//! struct Conf {
//!     startup: StartupConf,
//!     handler: <Handler as RequestFilter>::Conf,
//! }
//!
//! #[merge_opt]
//! struct Opt {
//!     startup: StartupOpt,
//!     log: CommonLogOpt,
//! }
//!
//! let opt = Opt::from_args();
//! let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
//! conf.handler.log.merge_with_opt(opt.log);
//!
//! let app = DefaultApp::<Handler>::from_conf(conf.handler).unwrap();
//! let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();
//!
//! // Do something with the server here, e.g. call server.run_forever()
//! ```
//!
//! For more comprehensive examples see the `examples` directory in the repository.

pub mod configuration;
mod handler;
#[cfg(unix)]
mod signal;
mod writer;

pub use configuration::{CommonLogConf, CommonLogOpt};
pub use handler::CommonLogHandler;
