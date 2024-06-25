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

use async_trait::async_trait;
use pandora_module_utils::pingora::{
    http_proxy_service, Error, ErrorType, ProxyHttp, Server, ServerConf, ServerOpt,
};
use pandora_module_utils::{DeserializeMap, OneOrMany};
use pingora::listeners::{TcpSocketOptions, TlsAccept, TlsSettings};
use pingora::services::Service;
use pingora::tls::ext::ssl_add_chain_cert;
use pingora::tls::{
    ext::{ssl_use_certificate, ssl_use_private_key},
    pkey::PKey,
    ssl::{NameType, SslRef},
    x509::X509,
};
use pingora::utils::CertKey;
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use std::collections::HashMap;
use std::fs::read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use structopt::StructOpt;

use crate::redirector::create_redirector;

pub(crate) const TLS_CONF_ERR: ErrorType = ErrorType::Custom("TLSConfigError");

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
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ListenAddr {
    /// IP address and port combination, e.g. `127.0.0.1:8080` or `[::1]:8080`
    pub addr: String,

    /// If `true`, TLS will be enabled for this address.
    ///
    /// This required TLS configuration to be present.
    pub tls: bool,

    /// Determines whether listening on IPv6 `[::]` address should accept IPv4 connections as well.
    ///
    /// If set, the IPV6_V6ONLY flag will be set accordingly for the socket. Otherwise the system
    /// default will be used.
    pub ipv6_only: Option<bool>,
}

impl ListenAddr {
    pub(crate) fn to_socket_options(&self) -> Option<TcpSocketOptions> {
        self.ipv6_only
            .map(|ipv6_only| TcpSocketOptions { ipv6_only })
    }
}

impl From<String> for ListenAddr {
    fn from(value: String) -> Self {
        Self {
            addr: value,
            tls: false,
            ipv6_only: None,
        }
    }
}

impl From<&str> for ListenAddr {
    fn from(value: &str) -> Self {
        value.to_owned().into()
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
                Ok(v.into())
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                use serde::de::Error as _;

                const ADDR_FIELD: &str = "addr";
                const IPV6_ONLY_FIELD: &str = "ipv6_only";
                const TLS_FIELD: &str = "tls";

                let mut addr = None;
                let mut tls = None;
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
                        TLS_FIELD => {
                            if tls.is_some() {
                                return Err(A::Error::duplicate_field(TLS_FIELD));
                            }
                            tls = Some(map.next_value()?);
                        }
                        other => {
                            return Err(A::Error::unknown_field(
                                other,
                                &[ADDR_FIELD, IPV6_ONLY_FIELD, TLS_FIELD],
                            ))
                        }
                    }
                }

                if let Some(addr) = addr {
                    let tls = tls.unwrap_or(false);
                    Ok(Self::Value {
                        addr,
                        ipv6_only,
                        tls,
                    })
                } else {
                    Err(A::Error::missing_field(ADDR_FIELD))
                }
            }
        }

        let visitor = AddrVisitor;
        deserializer.deserialize_any(visitor)
    }
}

/// Certificate/key combination for a single server name
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct CertKeyConf {
    /// Path to the certificate file
    pub cert_path: Option<PathBuf>,

    /// Path to the private key file
    pub key_path: Option<PathBuf>,
}

impl CertKeyConf {
    fn read_file<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, Box<Error>> {
        let path = path.as_ref();
        read(path).map_err(|err| {
            Error::because(
                TLS_CONF_ERR,
                format!("failed reading file {}", path.display()),
                err,
            )
        })
    }

    fn into_certificate(self) -> Result<CertKey, Box<Error>> {
        if let (Some(cert_path), Some(key_path)) = (self.cert_path, self.key_path) {
            const END_MARKER: &[u8] = b"-----END CERTIFICATE-----";
            let mut certs = Vec::new();
            let cert_data = Self::read_file(cert_path)?;
            let mut start = 0;
            while start != cert_data.len() {
                if cert_data[start..].iter().all(|b| b.is_ascii_whitespace()) {
                    break;
                }

                let end = cert_data[start..]
                    .windows(END_MARKER.len())
                    .position(|window| window == END_MARKER)
                    .map(|pos| start + pos + END_MARKER.len())
                    .unwrap_or(cert_data.len());
                certs.push(X509::from_pem(&cert_data[start..end]).map_err(|err| {
                    Error::because(TLS_CONF_ERR, "failed parsing certificate", err)
                })?);
                start = end;
            }

            if certs.is_empty() {
                return Err(Error::explain(
                    TLS_CONF_ERR,
                    "certificate chain shouldn't be empty",
                ));
            }

            let key = PKey::private_key_from_pem(&Self::read_file(key_path)?)
                .map_err(|err| Error::because(TLS_CONF_ERR, "failed parsing private key", err))?;

            Ok(CertKey::new(certs, key))
        } else {
            Err(Error::explain(
                TLS_CONF_ERR,
                "both `cert_path` and `key_path` settings must be present",
            ))
        }
    }
}

/// Certificate/key combination for a single server name
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct TlsRedirectorConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080"
    pub listen: OneOrMany<ListenAddr>,

    /// Default redirect target
    ///
    /// This can be a host name, e.g. `example.com` to redirect requests to `https://example.com/`
    /// or a host name an port combination, e.g. `example.com:8433` to redirect to
    /// `https://example.com:8433/`. No path should be specified, it will be copied from the
    /// original request.
    pub redirect_to: String,

    /// Server names mapped to their respective redirect target
    ///
    /// If the requested name is not found in the list or the request didn’t contain a server name,
    /// the default redirect target will be used.
    pub redirect_by_name: HashMap<String, String>,
}

impl TlsRedirectorConf {
    fn to_redirector(
        &self,
        server_conf: &Arc<ServerConf>,
    ) -> Result<Option<impl Service + 'static>, Box<Error>> {
        if self.listen.is_empty() {
            Ok(None)
        } else {
            create_redirector(self, server_conf).map(Some)
        }
    }
}

/// TLS configuration for the server
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct TlsConf {
    /// Default certificate/key combination
    #[pandora(flatten)]
    pub default: CertKeyConf,

    /// Certificate/key combinations for particular server names
    pub server_names: HashMap<String, CertKeyConf>,

    /// HTTP to HTTPS redirector settings
    pub redirector: TlsRedirectorConf,
}

impl TlsConf {
    fn into_callbacks(self) -> Result<TlsAcceptCallbacks, Box<Error>> {
        let mut certificates = HashMap::with_capacity(self.server_names.len() + 1);
        for (name, conf) in self.server_names.into_iter() {
            let cert = conf.into_certificate().map_err(|err| {
                Error::because(
                    TLS_CONF_ERR,
                    format!("failed setting up certificate/key for server name {name}"),
                    err,
                )
            })?;
            certificates.insert(name, cert);
        }
        let cert = self.default.into_certificate().map_err(|err| {
            Error::because(
                TLS_CONF_ERR,
                "failed setting up default certificate/key",
                err,
            )
        })?;
        certificates.insert(String::new(), cert);
        Ok(TlsAcceptCallbacks { certificates })
    }
}

#[derive(Debug, Clone)]
struct TlsAcceptCallbacks {
    certificates: HashMap<String, CertKey>,
}

#[async_trait]
impl TlsAccept for TlsAcceptCallbacks {
    async fn certificate_callback(&self, ssl: &mut SslRef) {
        let cert = ssl
            .servername(NameType::HOST_NAME)
            .and_then(|name| self.certificates.get(name))
            .or_else(|| self.certificates.get(""));
        if let Some(cert) = cert {
            // Errors are unexpected here, these should only occur if a certificate has been set
            // already or private key and certificate don’t match. Ok to panic then.
            ssl_use_certificate(ssl, cert.leaf()).unwrap();
            for intermediate in cert.intermediates() {
                ssl_add_chain_cert(ssl, intermediate).unwrap();
            }
            ssl_use_private_key(ssl, cert.key()).unwrap();
        }
    }
}

/// Configuration settings of the startup module
#[derive(Debug, Default, PartialEq, Eq, DeserializeMap)]
pub struct StartupConf {
    /// List of address/port combinations to listen on, e.g. "127.0.0.1:8080"
    pub listen: OneOrMany<ListenAddr>,

    /// TLS configuration for the server
    pub tls: TlsConf,

    /// Pingora’s default server configuration options
    #[pandora(flatten)]
    pub server: ServerConf,
}

impl StartupConf {
    /// Sets up a server with the given configuration and command line options
    pub fn into_server<SV>(self, app: SV, opt: Option<StartupOpt>) -> Result<Server, Box<Error>>
    where
        SV: ProxyHttp + Send + Sync + 'static,
        <SV as ProxyHttp>::CTX: Send + Sync,
    {
        let opt = opt.unwrap_or_default();

        let mut listen = opt.listen.map(|l| l.into()).unwrap_or(self.listen);
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

        let mut service = http_proxy_service(&server.configuration, app);
        for addr in &listen {
            if addr.tls {
                continue;
            }

            if let Some(socket_options) = addr.to_socket_options() {
                service.add_tcp_with_settings(&addr.addr, socket_options);
            } else {
                service.add_tcp(&addr.addr);
            }
        }

        if listen.iter().any(|addr| addr.tls) {
            if let Some(redirector) = self.tls.redirector.to_redirector(&server.configuration)? {
                server.add_service(redirector);
            }

            let tls_callbacks = self.tls.into_callbacks()?;
            for addr in &listen {
                if !addr.tls {
                    continue;
                }

                service.add_tls_with_settings(
                    &addr.addr,
                    addr.to_socket_options(),
                    TlsSettings::with_callbacks(Box::new(tls_callbacks.clone()))?,
                );
            }
        }
        server.add_service(service);

        Ok(server)
    }
}
