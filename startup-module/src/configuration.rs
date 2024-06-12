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

use module_utils::pingora::{http_proxy_service, ProxyHttp, Server, ServerConf, ServerOpt};
use module_utils::DeserializeMap;
use structopt::StructOpt;

/// Run a web server
#[derive(Debug, Default, StructOpt)]
pub struct StartupOpt {
    /// Address and port to listen on, e.g. "127.0.0.1:8080". This command line flag can be
    /// specified multiple times.
    #[structopt(short, long)]
    pub listen: Option<Vec<String>>,
    /// Use this flag to make the server run in the background.
    #[structopt(short, long)]
    pub daemon: bool,
    /// Test the configuration and exit. This is useful to validate the configuration before
    /// restarting the process.
    #[structopt(short, long)]
    pub test: bool,
    /// The path to the configuration file. This command line flag can be specified multiple times.
    #[structopt(short, long)]
    pub conf: Option<Vec<String>>,
}

/// Configuration settings of the compression module
#[derive(Debug, Default, DeserializeMap)]
pub struct StartupConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080".
    pub listen: Vec<String>,
    /// Pingora’s default server configuration options
    #[module_utils(flatten)]
    pub server: ServerConf,
}

impl StartupConf {
    /// Sets up a server with the given configuration and command line options
    pub fn into_server<SV>(self, app: SV, opt: Option<StartupOpt>) -> Server
    where
        SV: ProxyHttp + Send + Sync + 'static,
        <SV as ProxyHttp>::CTX: Send + Sync,
    {
        let opt = opt.unwrap_or_default();

        let mut listen = opt.listen.unwrap_or(self.listen);
        if listen.is_empty() {
            // Make certain we have a listening address
            listen.push("127.0.0.1:8080".to_owned());
            listen.push("[::1]:8080".to_owned());
        }

        let mut server = Server::new_with_opt_and_conf(
            ServerOpt {
                daemon: opt.daemon,
                test: opt.test,
                upgrade: false,
                nocapture: false,
                conf: None,
            },
            self.server,
        );
        server.bootstrap();

        let mut proxy = http_proxy_service(&server.configuration, app);
        for addr in listen {
            proxy.add_tcp(&addr);
        }
        server.add_service(proxy);

        server
    }
}
