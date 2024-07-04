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
use clap::{value_parser, Parser};
use http::header;
use http::uri::{Scheme, Uri};
use log::error;
use pandora_module_utils::pingora::{Error, ErrorType, HttpPeer, SessionWrapper};
use pandora_module_utils::{DeserializeMap, RequestFilter, RequestFilterResult};
use serde::de::{Deserializer, Error as _};
use serde::Deserialize as _;
use std::net::{SocketAddr, ToSocketAddrs};

/// Command line options of the compression module
#[derive(Debug, Default, Parser)]
pub struct UpstreamOpt {
    /// http:// or https:// URL identifying the server that requests should be forwarded for.
    /// Path and query parts of the URL have no effect.
    #[clap(long, value_parser = value_parser!(String))]
    pub upstream: Option<Uri>,
}

fn deserialize_uri<'de, D>(d: D) -> Result<Option<Uri>, D::Error>
where
    D: Deserializer<'de>,
{
    let uri = String::deserialize(d)?;
    let uri = uri
        .parse()
        .map_err(|err| D::Error::custom(format!("URL {uri} could not be parsed: {err}")))?;
    Ok(Some(uri))
}

/// Configuration settings of the compression module
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct UpstreamConf {
    /// http:// or https:// URL identifying the server that requests should be forwarded for.
    /// Path and query parts of the URL have no effect.
    #[pandora(deserialize_with = "deserialize_uri")]
    pub upstream: Option<Uri>,
}

impl UpstreamConf {
    /// Merges the command line options into the current configuration. Any command line options
    /// present overwrite existing settings.
    pub fn merge_with_opt(&mut self, opt: UpstreamOpt) {
        if opt.upstream.is_some() {
            self.upstream = opt.upstream;
        }
    }
}

/// Context data of the handler
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamContext {
    addr: SocketAddr,
    tls: bool,
    sni: String,
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamHandler {
    host_port: String,
    context: Option<UpstreamContext>,
}

impl TryFrom<UpstreamConf> for UpstreamHandler {
    type Error = Box<Error>;

    fn try_from(conf: UpstreamConf) -> Result<Self, Self::Error> {
        if let Some(upstream) = conf.upstream {
            let scheme = upstream.scheme().ok_or_else(|| {
                error!("provided upstream URL has no scheme: {upstream}");
                Error::new(ErrorType::InternalError)
            })?;

            let tls = if scheme == &Scheme::HTTP {
                false
            } else if scheme == &Scheme::HTTPS {
                true
            } else {
                error!("provided upstream URL is neither HTTP nor HTTPS: {upstream}");
                return Err(Error::new(ErrorType::InternalError));
            };

            let host = upstream.host().ok_or_else(|| {
                error!("provided upstream URL has no host name: {upstream}");
                Error::new(ErrorType::InternalError)
            })?;

            let port = upstream.port_u16().unwrap_or(if tls { 443 } else { 80 });

            let addr = (host, port)
                .to_socket_addrs()
                .map_err(|err| {
                    error!("failed resolving upstream host name {host}: {err}");
                    Error::new(ErrorType::InternalError)
                })?
                .next()
                .ok_or_else(|| {
                    error!("DNS lookup of upstream host name {host} didn't produce any results");
                    Error::new(ErrorType::InternalError)
                })?;

            let mut host_port = host.to_owned();
            if let Some(port) = upstream.port() {
                host_port.push(':');
                host_port.push_str(port.as_str());
            }

            Ok(Self {
                host_port,
                context: Some(UpstreamContext {
                    tls,
                    addr,
                    sni: host.to_owned(),
                }),
            })
        } else {
            Ok(Self {
                host_port: Default::default(),
                context: None,
            })
        }
    }
}

#[async_trait]
impl RequestFilter for UpstreamHandler {
    type Conf = UpstreamConf;
    type CTX = Option<UpstreamContext>;
    fn new_ctx() -> Self::CTX {
        None
    }

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        if let Some(context) = &self.context {
            session
                .req_header_mut()
                .insert_header(header::HOST, &self.host_port)?;

            *ctx = Some(context.clone());

            Ok(RequestFilterResult::Handled)
        } else {
            Ok(RequestFilterResult::Unhandled)
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
    ) -> Result<Option<Box<HttpPeer>>, Box<Error>> {
        if let Some(context) = ctx {
            Ok(Some(Box::new(HttpPeer::new(
                context.addr,
                context.tls,
                context.sni.clone(),
            ))))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use http::HeaderValue;
    use pandora_module_utils::pingora::{
        create_test_session, RequestHeader, ResponseHeader, Session,
    };
    use pandora_module_utils::FromYaml;
    use startup_module::DefaultApp;
    use test_log::test;

    fn make_app(configured: bool) -> DefaultApp<UpstreamHandler> {
        let conf = if configured {
            UpstreamConf::from_yaml(
                r#"
                    upstream: https://example.com
                "#,
            )
            .unwrap()
        } else {
            UpstreamConf::default()
        };
        DefaultApp::new(conf.try_into().unwrap())
    }

    async fn make_session() -> Session {
        let header = RequestHeader::build("GET", b"/", None).unwrap();
        create_test_session(header).await
    }

    #[test(tokio::test)]
    async fn unconfigured() {
        let mut app = make_app(false);
        let session = make_session().await;
        let result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
    }

    #[test(tokio::test)]
    async fn handled() {
        let mut app = make_app(true);
        let session = make_session().await;
        let result = app
            .handle_request_with_upstream(session, |session, peer| {
                assert_eq!(peer.scheme.to_string(), "HTTPS".to_owned());
                assert_eq!(peer.sni, "example.com");
                assert_eq!(
                    session.req_header().headers.get("Host"),
                    Some(&HeaderValue::from_str("example.com").unwrap())
                );

                ResponseHeader::build(200, None)
            })
            .await;
        assert!(result.err().is_none());
    }
}
