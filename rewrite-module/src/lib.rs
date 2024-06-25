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

//! # Rewrite Module for Pandora Web Server
//!
//! This crate adds URI rewriting capabilities. A modified URI can be passed on to further
//! processors, or a redirect response can make the client emit a new request. A number of rules
//! can be defined in the configuration file, for example:
//!
//! ```yaml
//! rewrite_rules:
//! - from: /old.txt
//!   query_regex: "!noredirect"
//!   to: /file.txt
//!   type: permanent
//! - from: /view.php
//!   query_regex: "^file=large\\.txt$"
//!   to: /large.txt
//! - from: /images/*
//!   from_regex: "\\.jpg$"
//!   to: https://example.com${tail}
//!   type: redirect
//! ```
//!
//! ## Rewrite rules
//!
//! The following parameters can be defined for a rule:
//!
//! * `from` restricts the rule to a specific path or a path prefix (if the value ends with `/*`).
//! * `from_regex` allows further refining the path restriction via a regular expression. Putting
//!   `!` before the regular expression makes the rule apply to paths *not* matched by the regular
//!   expression.
//! * `query_regex` restricts the rule to particular query strings only. Putting `!` before the
//!   regular expression makes the rule apply to query strings *not* matched by the regular
//!   expression.
//! * `to` is the new path and query string to be used if the rule is applied. Some variables will
//!   are replaced here:
//!   * `${tail}`: The part of the original path matched by `/*` in `from`
//!   * `${query}`: The original query string
//!   * `${http_<header>}`: The value of an HTTP header, e.g. `${http_host}` will be replaced by
//!     the value of the `Host` header
//! * `type` is the rewrite type, one of `internal` (default, internal redirect), `redirect`
//!   (temporary redirect) or `permanent` (permanent redirect)
//!
//! If multiple rules potentially apply to a particular request, the rule with the longer path in
//! the `from` field is applied. If multiple rules with the same path in `from` exist, exact
//! matches are preferred over prefix matches.
//!
//! ## Code example
//!
//! You would normally combine the handler of this module with the handlers of other modules such
//! as `static-files-module`. The `pandora-module-utils` and `startup-module` crates provide
//! helpers to simplify merging of configuration and the command-line options of various handlers
//! as well as creating a server instance from the configuration:
//!
//! ```rust
//! use pandora_module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
//! use rewrite_module::RewriteHandler;
//! use startup_module::{DefaultApp, StartupConf, StartupOpt};
//! use static_files_module::{StaticFilesHandler, StaticFilesOpt};
//! use structopt::StructOpt;
//!
//! #[derive(Debug, RequestFilter)]
//! struct Handler {
//!     rewrite: RewriteHandler,
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
//!     static_files: StaticFilesOpt,
//! }
//!
//! let opt = Opt::from_args();
//! let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
//! conf.handler.static_files.merge_with_opt(opt.static_files);
//!
//! let app = DefaultApp::<Handler>::from_conf(conf.handler).unwrap();
//! let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();
//!
//! // Do something with the server here, e.g. call server.run_forever()
//! ```

pub mod configuration;
mod handler;

pub use handler::RewriteHandler;
