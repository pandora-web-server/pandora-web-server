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

use async_trait::async_trait;
use clap::Parser;
use pandora_module_utils::pingora::{Error, SessionWrapper, SocketAddr};
use pandora_module_utils::{DeserializeMap, RequestFilter};
use std::net::IpAddr;

/// Command line options of the IP anonymization module
#[derive(Debug, Parser)]
pub struct IPAnonymizationOpt {
    /// Enables IP address anonymization
    #[clap(long)]
    pub anonymization_enabled: bool,
}

/// IP anonymization configuration
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct IPAnonymizationConf {
    /// If `true`, part of the client’s IP address will be removed, ensuring that logged addresses
    /// cannot be traced back to an individual user.
    pub anonymization_enabled: bool,
}

impl IPAnonymizationConf {
    /// Merges the command line options into the current configuration. Command line options
    /// present overwrite existing settings.
    pub fn merge_with_opt(&mut self, opt: IPAnonymizationOpt) {
        if opt.anonymization_enabled {
            self.anonymization_enabled = true;
        }
    }
}

/// IP Anonymization module handler
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IPAnonymizationHandler {
    conf: IPAnonymizationConf,
}

impl TryFrom<IPAnonymizationConf> for IPAnonymizationHandler {
    type Error = Box<Error>;

    fn try_from(conf: IPAnonymizationConf) -> Result<Self, Self::Error> {
        Ok(Self { conf })
    }
}

fn anonymize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(addr) => {
            let mut octets = addr.octets();
            octets[octets.len() - 1] = 0;
            IpAddr::from(octets)
        }
        IpAddr::V6(addr) => {
            let mut octets = addr.octets();
            if octets[0..12] == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF] {
                // This is an IPv4 address in disguise
                octets[octets.len() - 1] = 0;
            } else {
                octets[4..].fill(0);
            }
            IpAddr::from(octets)
        }
    }
}

fn anonymize_address(addr: Option<&SocketAddr>) -> Option<SocketAddr> {
    let addr = addr?;
    if let SocketAddr::Inet(addr) = addr {
        let ip = anonymize_ip(addr.ip());
        Some(SocketAddr::Inet((ip, addr.port()).into()))
    } else {
        None
    }
}

#[async_trait]
impl RequestFilter for IPAnonymizationHandler {
    type Conf = IPAnonymizationConf;

    type CTX = ();

    fn new_ctx() -> Self::CTX {}

    async fn early_request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<(), Box<Error>> {
        if self.conf.anonymization_enabled {
            if let Some(addr) = anonymize_address(session.client_addr()) {
                session.set_client_addr(addr);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pandora_module_utils::pingora::{create_test_session, ErrorType, RequestHeader, Session};
    use pandora_module_utils::FromYaml;
    use startup_module::DefaultApp;
    use std::str::FromStr;
    use test_log::test;

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct IPAddressConf {
        ip_address: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct IPAddressHandler {
        ip_address: String,
    }

    #[async_trait]
    impl RequestFilter for IPAddressHandler {
        type Conf = IPAddressConf;
        type CTX = ();
        fn new_ctx() -> Self::CTX {}

        async fn early_request_filter(
            &self,
            session: &mut impl SessionWrapper,
            _ctx: &mut Self::CTX,
        ) -> Result<(), Box<Error>> {
            session.set_client_addr(SocketAddr::Inet(
                (IpAddr::from_str(&self.ip_address).unwrap(), 8000).into(),
            ));
            Ok(())
        }
    }

    impl TryFrom<IPAddressConf> for IPAddressHandler {
        type Error = Box<Error>;

        fn try_from(conf: IPAddressConf) -> Result<Self, Self::Error> {
            Ok(Self {
                ip_address: conf.ip_address,
            })
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, RequestFilter)]
    struct Handler {
        address: IPAddressHandler,
        anonymization: IPAnonymizationHandler,
    }

    fn make_app(conf: &str) -> DefaultApp<Handler> {
        DefaultApp::new(
            <Handler as RequestFilter>::Conf::from_yaml(conf)
                .unwrap()
                .try_into()
                .unwrap(),
        )
    }

    async fn make_session() -> Session {
        let header = RequestHeader::build("GET", b"/", None).unwrap();
        create_test_session(header).await
    }

    #[test(tokio::test)]
    async fn unconfigured() {
        let mut app = make_app(
            r#"
                ip_address: 1.2.3.4
                anonymization_enabled: false
            "#,
        );

        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(
            result.session().client_addr(),
            Some(&SocketAddr::Inet(
                (IpAddr::from_str("1.2.3.4").unwrap(), 8000).into()
            ))
        );
    }

    #[test(tokio::test)]
    async fn ipv4() {
        let mut app = make_app(
            r#"
                ip_address: 1.2.3.4
                anonymization_enabled: true
            "#,
        );

        // IPv4
        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(
            result.session().client_addr(),
            Some(&SocketAddr::Inet(
                (IpAddr::from_str("1.2.3.0").unwrap(), 8000).into()
            ))
        );
    }

    #[test(tokio::test)]
    async fn ipv4_mapped_ipv6() {
        let mut app = make_app(
            r#"
                ip_address: ::ffff:1.2.3.4
                anonymization_enabled: true
            "#,
        );

        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(
            result.session().client_addr(),
            Some(&SocketAddr::Inet(
                (IpAddr::from_str("::ffff:1.2.3.0").unwrap(), 8000).into()
            ))
        );
    }

    #[test(tokio::test)]
    async fn ipv6() {
        let mut app = make_app(
            r#"
                ip_address: 1234:5678:90ab:cdef:1234:5678:90ab:cdef
                anonymization_enabled: true
            "#,
        );

        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(
            result.session().client_addr(),
            Some(&SocketAddr::Inet(
                (IpAddr::from_str("1234:5678::").unwrap(), 8000).into()
            ))
        );
    }
}
