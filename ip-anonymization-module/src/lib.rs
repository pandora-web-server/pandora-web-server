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

use std::net::IpAddr;

use async_trait::async_trait;
use clap::Parser;
use pandora_module_utils::pingora::{Error, SessionWrapper, SocketAddr};
use pandora_module_utils::{DeserializeMap, RequestFilter, RequestFilterResult};

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

/// Handler for Pingora’s `request_filter` phase
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

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        if self.conf.anonymization_enabled {
            if let Some(addr) = anonymize_address(session.client_addr()) {
                session.set_client_addr(addr);
            }
        }
        Ok(RequestFilterResult::Unhandled)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    use pandora_module_utils::pingora::{RequestHeader, TestSession};
    use pandora_module_utils::FromYaml;
    use test_log::test;

    fn make_handler(conf: &str) -> IPAnonymizationHandler {
        <IPAnonymizationHandler as RequestFilter>::Conf::from_yaml(conf)
            .unwrap()
            .try_into()
            .unwrap()
    }

    async fn make_session() -> TestSession {
        let header = RequestHeader::build("GET", b"/", None).unwrap();
        TestSession::from(header).await
    }

    #[test(tokio::test)]
    async fn unconfigured() -> Result<(), Box<Error>> {
        let handler = make_handler("anonymization_enabled: false");

        let mut session = make_session().await;
        session.set_client_addr(SocketAddr::Inet(([1, 2, 3, 4], 8000).into()));
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.client_addr(),
            Some(&SocketAddr::Inet(([1, 2, 3, 4], 8000).into()))
        );

        Ok(())
    }

    #[test(tokio::test)]
    async fn enabled() -> Result<(), Box<Error>> {
        let handler = make_handler("anonymization_enabled: true");

        // IPv4
        let mut session = make_session().await;
        session.set_client_addr(SocketAddr::Inet(
            (IpAddr::from_str("1.2.3.4").unwrap(), 8000).into(),
        ));
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.client_addr(),
            Some(&SocketAddr::Inet(
                (IpAddr::from_str("1.2.3.0").unwrap(), 8000).into()
            ))
        );

        // IPv4 mapped to IPv6
        let mut session = make_session().await;
        session.set_client_addr(SocketAddr::Inet(
            (IpAddr::from_str("::ffff:1.2.3.4").unwrap(), 8000).into(),
        ));
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.client_addr(),
            Some(&SocketAddr::Inet(
                (IpAddr::from_str("::ffff:1.2.3.0").unwrap(), 8000).into()
            ))
        );

        // IPv6
        let mut session = make_session().await;
        session.set_client_addr(SocketAddr::Inet(
            (
                IpAddr::from_str("1234:5678:90ab:cdef:1234:5678:90ab:cdef").unwrap(),
                8000,
            )
                .into(),
        ));
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.client_addr(),
            Some(&SocketAddr::Inet(
                (IpAddr::from_str("1234:5678::").unwrap(), 8000).into()
            ))
        );

        Ok(())
    }
}
