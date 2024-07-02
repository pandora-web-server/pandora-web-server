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

mod web_app;

use auth_module::{AuthHandler, AuthOpt};
use clap::Parser;
use common_log_module::{CommonLogHandler, CommonLogOpt};
use compression_module::{CompressionHandler, CompressionOpt};
use ip_anonymization_module::{IPAnonymizationHandler, IPAnonymizationOpt};
use log::error;
use pandora_module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use rewrite_module::RewriteHandler;
use startup_module::{DefaultApp, StartupConf, StartupOpt};

use web_app::{WebAppHandler, WebAppOpt};

#[derive(Debug, Clone, PartialEq, Eq, RequestFilter)]
struct Handler {
    anonymization: IPAnonymizationHandler,
    compression: CompressionHandler,
    log: CommonLogHandler,
    auth: AuthHandler,
    rewrite: RewriteHandler,
    web_app: WebAppHandler,
}

/// Run Pandora Web Server
#[merge_opt]
struct Opt {
    startup: StartupOpt,
    anonymization: IPAnonymizationOpt,
    compression: CompressionOpt,
    log: CommonLogOpt,
    auth: AuthOpt,
    web_app: WebAppOpt,
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

    conf.handler.anonymization.merge_with_opt(opt.anonymization);
    conf.handler.compression.merge_with_opt(opt.compression);
    conf.handler.log.merge_with_opt(opt.log);
    conf.handler.auth.merge_with_opt(opt.auth);
    conf.handler.web_app.merge_with_opt(opt.web_app);

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
