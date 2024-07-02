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

#![doc = include_str!("../README.md")]

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
