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

//! # Pandora Web Server
//!
//! This is a modular web server, supporting different configurations per host name and subpath.
//!
//! ## Features
//!
//! * Flexible configuration via one or multiple YAML files
//! * Basic configurations possible via command-line options
//! * Built on Cloudflare’s fast Pingora framework
//! * Flexible selection of modules to add to the build
//! * Easy to create an own build with custom modules
//!
//! ### Modules
//!
//! * **Auth**: Puts parts of the webspace behind an authentication wall. Supports page-based
//!   logins (recommended) and HTTP Basic authentication.
//! * **Common Log**: Access logging using [Common Log
//!   Format](https://en.wikipedia.org/wiki/Common_Log_Format), fields to be logged are
//!   configurable.
//! * **Compression**: Dynamic compression of server responses and (if necessary) decompression of
//!   upstream responses.
//! * **Headers**: Structured configuration of `Cache-Control` and `Content-Security-Policy`
//!   headers, supports adding custom response headers.
//! * **IP Anonymization**: Removes part of the IP address, making sure no personal data is
//!   collected here.
//! * **Rewrite**: Flexible rules allowing internal or external redirection of requests.
//! * **Static Files**: Serves static files from a directory, supports pre-compressed files.
//! * **Startup**: Listening on any number of IP addresses/ports, TLS support, automatic
//!   redirecting from HTTP to HTTPS.
//! * **Upstream**: Delegates the request to an upstream HTTP server.
//! * **Virtual Hosts**: Separate configurations per host name and (optionally) subpaths within a
//!   host.
//!
//! ## Configuration
//!
//! The default preset puts the configuration for Startup, IP Anonymization and Headers modules at
//! the top level, all other modules are configured per host name. A configuration file could look
//! like this then:
//!
//! ```yaml
//! # Startup module settings (https://docs.rs/startup-module/latest/startup_module/struct.StartupConf.html)
//! listen:
//! - "[::]:8080"
//! daemon: false
//!
//! # IP Anonymization module settings (https://docs.rs/ip-anonymization-module/latest/ip_anonymization_module/struct.IPAnonymizationConf.html)
//! anonymization_enabled: true
//!
//! # Headers module settings (https://docs.rs/headers-module/latest/headers_module/struct.HeadersConf.html)
//! response_headers:
//!     custom:
//!     - Server: "My server is the best"
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
//! Example config files for this preset are provided in this directory.
//!
//! ## Building and running the web server
//!
//! To create a release build with the default features, run the following command:
//!
//! ```sh
//! cargo build --release
//! ```
//!
//! You can also run a debug build with the example configuration files from this directory:
//!
//! ```sh
//! cargo run -- -c config/*.yaml
//! ```
//!
//! To enable debugging output you can use the `RUST_LOG` environment variable:
//!
//! ```sh
//! RUST_LOG=debug cargo run -- -c config/*.yaml
//! ```
//!
//! You can find more information on the `RUST_LOG` environment variable in the [documentation of
//! the `env_logger` crate](https://docs.rs/env_logger/latest/env_logger/).
//!
//! ## Selecting other features
//!
//! In additions to the default features, the preset `default-single-host` is also available. It
//! can be built with the following command:
//!
//! ```sh
//! cargo build --release --no-default-features --features=default-single-host
//! ```
//!
//! The resulting web server will have no host-based configuration, all modules are to be
//! configured at the top level.
//!
//! Features of this crate also allow selecting for each module whether it should be used at the
//! top level or in a per-host configuration:
//!
//! | Module            | Top-level feature             | Per-host feature              |
//! |-------------------|-------------------------------|-------------------------------|
//! | Auth              | `auth-top-level`              | `auth-per-host`               |
//! | Common Log        | `common-log-top-level`        | `common-log-per-host`         |
//! | Compression       | `compression-top-level`       | `compression-per-host`        |
//! | Headers           | `headers-top-level`           | `headers-per-host`            |
//! | IP Anonymization  | `ip-anonymization-top-level`  | `ip-anonymization-per-host`   |
//! | Rewrite           | `rewrite-top-level`           | `rewrite-per-host`            |
//! | Static Files      | `static-files-top-level`      | `static-files-per-host`       |
//! | Upstream          | `upstream-top-level`          | `upstream-per-host`           |
//!
//! For example, if your server only needs to serve static files and write access logs, you can
//! build it with the following command:
//!
//! ```sh
//! cargo build --release --no-default-features --features=static-files-top-level,common-log-top-level
//! ```
//!
//! The Startup module is always present at the top level, and the Virtual Hosts module is added
//! automatically if any per-host feature is enabled.
//!
//! *Note*: It is technically possible to include a module both at the top and per-host level. It
//! will be configurable on both levels then. Whether this approach makes sense and how the two
//! module instances will interact with each other is a different question however. Such setups are
//! unsupported.

use clap::Parser;
use log::error;
use pandora_module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use startup_module::{DefaultApp, StartupConf, StartupOpt};

#[derive(Debug, Clone, PartialEq, Eq, RequestFilter)]
struct Handler {
    #[cfg(feature = "ip-anonymization-top-level")]
    anonymization: ip_anonymization_module::IPAnonymizationHandler,
    #[cfg(feature = "common-log-top-level")]
    log: common_log_module::CommonLogHandler,
    #[cfg(feature = "compression-top-level")]
    compression: compression_module::CompressionHandler,
    #[cfg(feature = "headers-top-level")]
    headers: headers_module::HeadersHandler,
    #[cfg(feature = "auth-top-level")]
    auth: auth_module::AuthHandler,
    #[cfg(feature = "rewrite-top-level")]
    rewrite: rewrite_module::RewriteHandler,
    #[cfg(feature = "upstream-top-level")]
    upstream: upstream_module::UpstreamHandler,
    #[cfg(feature = "static-files-top-level")]
    static_files: static_files_module::StaticFilesHandler,
    #[cfg(any(
        feature = "auth-per-host",
        feature = "common-log-per-host",
        feature = "compression-per-host",
        feature = "headers-per-host",
        feature = "ip-anonymization-per-host",
        feature = "rewrite-per-host",
        feature = "static-files-per-host",
        feature = "upstream-per-host"
    ))]
    virtual_hosts: virtual_hosts_module::VirtualHostsHandler<HostHandler>,
}

#[derive(Debug, Clone, PartialEq, Eq, RequestFilter)]
struct HostHandler {
    #[cfg(feature = "ip-anonymization-per-host")]
    anonymization: ip_anonymization_module::IPAnonymizationHandler,
    #[cfg(feature = "common-log-per-host")]
    log: common_log_module::CommonLogHandler,
    #[cfg(feature = "compression-per-host")]
    compression: compression_module::CompressionHandler,
    #[cfg(feature = "headers-per-host")]
    headers: headers_module::HeadersHandler,
    #[cfg(feature = "auth-per-host")]
    auth: auth_module::AuthHandler,
    #[cfg(feature = "rewrite-per-host")]
    rewrite: rewrite_module::RewriteHandler,
    #[cfg(feature = "upstream-per-host")]
    upstream: upstream_module::UpstreamHandler,
    #[cfg(feature = "static-files-per-host")]
    static_files: static_files_module::StaticFilesHandler,
}

/// Run Pandora Web Server
#[merge_opt]
struct Opt {
    startup: StartupOpt,
    #[cfg(feature = "ip-anonymization-top-level")]
    anonymization: ip_anonymization_module::IPAnonymizationOpt,
    #[cfg(feature = "common-log-top-level")]
    log: common_log_module::CommonLogOpt,
    #[cfg(feature = "auth-top-level")]
    compression: compression_module::CompressionOpt,
    #[cfg(feature = "static-files-top-level")]
    auth: auth_module::AuthOpt,
    #[cfg(feature = "compression-top-level")]
    static_files: static_files_module::StaticFilesOpt,
}

/// The configuration of Pandora Web Server
#[merge_conf]
struct Conf {
    startup: StartupConf,
    handler: <Handler as RequestFilter>::Conf,
}

fn main() {
    env_logger::init();

    let opt = Opt::parse();

    #[allow(unused_mut)]
    let mut conf = match Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])) {
        Ok(conf) => conf,
        Err(err) => {
            error!("{err}");
            Conf::default()
        }
    };

    #[cfg(feature = "ip-anonymization-top-level")]
    conf.handler.anonymization.merge_with_opt(opt.anonymization);
    #[cfg(feature = "common-log-top-level")]
    conf.handler.log.merge_with_opt(opt.log);
    #[cfg(feature = "compression-top-level")]
    conf.handler.compression.merge_with_opt(opt.compression);
    #[cfg(feature = "auth-top-level")]
    conf.handler.auth.merge_with_opt(opt.auth);
    #[cfg(feature = "static-files-top-level")]
    conf.handler.static_files.merge_with_opt(opt.static_files);

    let server = match DefaultApp::<Handler>::from_conf(conf.handler)
        .and_then(|app| conf.startup.into_server(app, Some(opt.startup)))
    {
        Ok(server) => server,
        Err(err) => {
            error!("{err}");
            return;
        }
    };

    server.run_forever();
}
