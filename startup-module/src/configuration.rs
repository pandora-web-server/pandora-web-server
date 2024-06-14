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

use module_utils::pingora::{
    http_proxy_service, ProxyHttp, Server, ServerConf, ServerOpt, TcpSocketOptions,
};
use module_utils::DeserializeMap;
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use structopt::StructOpt;

/// Run a web server
#[derive(Debug, Default, StructOpt)]
pub struct StartupOpt {
    /// Address and port to listen on, e.g. "127.0.0.1:8080". This command line flag can be
    /// specified multiple times.
    #[structopt(short, long, parse(from_str))]
    pub listen: Option<Vec<ListenAddr>>,
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

/// Address for the server to listen on
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ListenAddr {
    /// IP address and port combination, e.g. `127.0.0.1:8080` or `[::1]:8080`
    pub addr: String,

    /// Determines whether listening on IPv6 [::] address should accept IPv4 connections as well.
    ///
    /// If set, the IPV6_V6ONLY flag will be set accordingly for the socket. Otherwise the system
    /// default will be used.
    pub ipv6_only: Option<bool>,
}

impl From<&str> for ListenAddr {
    fn from(value: &str) -> Self {
        Self {
            addr: value.to_owned(),
            ipv6_only: None,
        }
    }
}

impl<'de> Deserialize<'de> for ListenAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AddrVisitor;

        impl<'de> Visitor<'de> for AddrVisitor {
            type Value = ListenAddr;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("address string or ListenAddr structure")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.into())
            }

            fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.into())
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Self::Value {
                    addr: v,
                    ipv6_only: None,
                })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                use serde::de::Error as _;

                const ADDR_FIELD: &str = "addr";
                const IPV6_ONLY_FIELD: &str = "ipv6_only";

                let mut addr = None;
                let mut ipv6_only = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        ADDR_FIELD => {
                            if addr.is_some() {
                                return Err(A::Error::duplicate_field(ADDR_FIELD));
                            }
                            addr = Some(map.next_value()?);
                        }
                        IPV6_ONLY_FIELD => {
                            if ipv6_only.is_some() {
                                return Err(A::Error::duplicate_field(IPV6_ONLY_FIELD));
                            }
                            ipv6_only = Some(map.next_value()?);
                        }
                        other => {
                            return Err(A::Error::unknown_field(
                                other,
                                &[ADDR_FIELD, IPV6_ONLY_FIELD],
                            ))
                        }
                    }
                }

                if let Some(addr) = addr {
                    Ok(Self::Value { addr, ipv6_only })
                } else {
                    Err(A::Error::missing_field(ADDR_FIELD))
                }
            }
        }

        let visitor = AddrVisitor;
        deserializer.deserialize_any(visitor)
    }
}

/// Configuration settings of the startup module
#[derive(Debug, Default, DeserializeMap)]
pub struct StartupConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080".
    pub listen: Vec<ListenAddr>,
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
            listen.push("127.0.0.1:8080".into());
            listen.push("[::1]:8080".into());
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
            if let Some(ipv6_only) = addr.ipv6_only {
                proxy.add_tcp_with_settings(&addr.addr, TcpSocketOptions { ipv6_only });
            } else {
                proxy.add_tcp(&addr.addr);
            }
        }
        server.add_service(proxy);

        server
    }
}
