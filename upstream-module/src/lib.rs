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

//! # Upstream Module for Pingora
//!
//! This crate helps configure Pingora’s upstream functionality. It is most useful in combination
//! with the `virtual-hosts-module` crate that allows applying multiple upstream configurations
//! conditionally.
//!
//! Currently only one configuration option is provided: `upstream` (`--upstream` as command line
//! option). The value should be a URL like `http://127.0.0.1:8081` or `https://example.com`.
//!
//! Supported URL schemes are `http://` and `https://`. Other than the scheme, only host name and
//! port are considered. Other parts of the URL are ignored if present.
//!
//! The `UpstreamHandler` type has to be called in both `request_filter` and `upstream_peer`
//! Pingora Proxy phases. The former selects an upstream peer and modifies the request by adding
//! the appropriate `Host` header. The latter retrieves the previously selected upstream peer.
//!
//! ```rust
//! use async_trait::async_trait;
//! use upstream_module::UpstreamHandler;
//! use module_utils::RequestFilter;
//! use pingora_core::Error;
//! use pingora_core::upstreams::peer::HttpPeer;
//! use pingora_proxy::{ProxyHttp, Session};
//!
//! pub struct MyServer {
//!     upstream_handler: UpstreamHandler,
//! }
//!
//! #[async_trait]
//! impl ProxyHttp for MyServer {
//!     type CTX = <UpstreamHandler as RequestFilter>::CTX;
//!     fn new_ctx(&self) -> Self::CTX {
//!         UpstreamHandler::new_ctx()
//!     }
//!
//!     async fn request_filter(
//!         &self,
//!         session: &mut Session,
//!         ctx: &mut Self::CTX,
//!     ) -> Result<bool, Box<Error>> {
//!         // Select upstream peer according to configuration. This could be called based on some
//!         // conditions.
//!         self.upstream_handler.handle(session, ctx).await
//!     }
//!
//!     async fn upstream_peer(
//!         &self,
//!         session: &mut Session,
//!         ctx: &mut Self::CTX,
//!     ) -> Result<Box<HttpPeer>, Box<Error>> {
//!         // Return previously selected peer if any
//!         UpstreamHandler::upstream_peer(session, ctx).await
//!     }
//! }
//! ```
//!
//! To create a handler, you will typically read its configuration from a configuration file,
//! optionally combined with command line options. The following code will extend Pingora's usual
//! configuration file and command line options accordingly.
//!
//! ```rust
//! use upstream_module::{UpstreamConf, UpstreamHandler, UpstreamOpt};
//! use module_utils::{merge_conf, merge_opt, FromYaml};
//! use pingora_core::server::Server;
//! use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
//! use structopt::StructOpt;
//!
//! #[merge_opt]
//! struct Opt {
//!     server: ServerOpt,
//!     upstream: UpstreamOpt,
//! }
//!
//! #[merge_conf]
//! struct Conf {
//!     server: ServerConf,
//!     upstream: UpstreamConf,
//! }
//!
//! let opt = Opt::from_args();
//! let mut conf = opt
//!     .server
//!     .conf
//!     .as_ref()
//!     .and_then(|path| Conf::load_from_yaml(path).ok())
//!     .unwrap_or_else(Conf::default);
//! conf.upstream.merge_with_opt(opt.upstream);
//!
//! let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
//! server.bootstrap();
//!
//! let upstream_handler: UpstreamHandler = conf.upstream.try_into().unwrap();
//! ```
//!
//! For complete and more realistic code see `virtual-hosts` example in the repository.

use async_trait::async_trait;
use http::header;
use http::uri::{Scheme, Uri};
use log::error;
use module_utils::pingora::{Error, ErrorType, HttpPeer, Session};
use module_utils::{RequestFilter, RequestFilterResult};
use serde::{
    de::{Deserializer, Error as _},
    Deserialize,
};
use std::net::{SocketAddr, ToSocketAddrs};
use structopt::StructOpt;

/// Command line options of the compression module
#[derive(Debug, Default, StructOpt)]
pub struct UpstreamOpt {
    /// http:// or https:// URL identifying the server that requests should be forwarded for.
    /// Path and query parts of the URL have no effect.
    #[structopt(long, parse(try_from_str))]
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
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct UpstreamConf {
    /// http:// or https:// URL identifying the server that requests should be forwarded for.
    /// Path and query parts of the URL have no effect.
    #[serde(deserialize_with = "deserialize_uri")]
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
#[derive(Debug, Clone)]
pub struct UpstreamContext {
    addr: SocketAddr,
    tls: bool,
    sni: String,
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug)]
pub struct UpstreamHandler {
    host_port: String,
    context: Option<UpstreamContext>,
}

impl UpstreamHandler {
    /// This function should be called during the `upstream_peer` phase of Pingora Proxy to produce
    /// the upstream peer which was determined in the `request_filter` phase. Will return a 404 Not
    /// Found error if no upstream is configured.
    pub async fn upstream_peer(
        _session: &mut Session,
        ctx: &mut Option<UpstreamContext>,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        if let Some(context) = ctx {
            Ok(Box::new(HttpPeer::new(
                context.addr,
                context.tls,
                context.sni.clone(),
            )))
        } else {
            Err(Error::new(ErrorType::HTTPStatus(404)))
        }
    }
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
        session: &mut Session,
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
}
