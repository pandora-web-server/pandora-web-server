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
use http::header;
use log::warn;
use pingora_core::Error;
use pingora_proxy::Session;
use pingora_utils_core::{RequestFilter, RequestFilterResult};
use std::collections::HashMap;

use crate::configuration::VirtualHostsConf;

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug)]
pub struct VirtualHostsHandler<H> {
    handlers: HashMap<String, H>,
    aliases: HashMap<String, String>,
    default: Option<String>,
}

fn host_from_uri(uri: &http::uri::Uri) -> Option<String> {
    let mut host = uri.host()?.to_owned();
    if let Some(port) = uri.port() {
        host.push(':');
        host.push_str(port.as_str());
    }
    Some(host)
}

#[async_trait]
impl<H> RequestFilter for VirtualHostsHandler<H>
where
    H: RequestFilter + Sync,
    H::Conf: Default,
    H::CTX: Send,
{
    type Conf = VirtualHostsConf<H::Conf>;

    type CTX = H::CTX;

    fn new_ctx() -> Self::CTX {
        H::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        let host = session
            .get_header(header::HOST)
            .and_then(|host| host.to_str().ok())
            .map(|host| host.to_owned())
            .or_else(|| host_from_uri(&session.req_header().uri));

        let handler = host
            .and_then(|host| {
                if let Some(handler) = self.handlers.get(&host) {
                    Some(handler)
                } else if let Some(alias) = self.aliases.get(&host) {
                    self.handlers.get(alias)
                } else {
                    None
                }
            })
            .or_else(|| {
                self.default
                    .as_ref()
                    .and_then(|default| self.handlers.get(default))
            });

        if let Some(handler) = handler {
            handler.request_filter(session, ctx).await
        } else {
            Ok(RequestFilterResult::Unhandled)
        }
    }
}

impl<C, H> TryFrom<VirtualHostsConf<C>> for VirtualHostsHandler<H>
where
    C: TryInto<H, Error = Box<Error>> + Default,
{
    type Error = Box<Error>;

    fn try_from(conf: VirtualHostsConf<C>) -> Result<Self, Box<Error>> {
        let mut handlers = HashMap::new();
        let mut aliases = HashMap::new();
        let mut default = None;
        for (host, host_conf) in conf.vhosts.into_iter() {
            for alias in host_conf.host.aliases.into_iter() {
                aliases.insert(alias, host.clone());
            }
            if host_conf.host.default {
                if let Some(previous) = &default {
                    warn!("both {previous} and {host} are marked as default virtual host, ignoring the latter");
                } else {
                    default = Some(host.clone());
                }
            }
            handlers.insert(host, host_conf.config.try_into()?);
        }

        Ok(Self {
            handlers,
            aliases,
            default,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{HostConfig, VirtualHostConf};

    use async_trait::async_trait;
    use test_log::test;
    use tokio_test::io::Builder;

    struct Handler {
        result: RequestFilterResult,
    }

    #[async_trait]
    impl RequestFilter for Handler {
        type Conf = RequestFilterResult;
        type CTX = ();
        fn new_ctx() -> Self::CTX {}
        async fn request_filter(
            &self,
            _session: &mut Session,
            _ctx: &mut Self::CTX,
        ) -> Result<RequestFilterResult, Box<Error>> {
            Ok(self.result)
        }
    }

    impl TryFrom<RequestFilterResult> for Handler {
        type Error = Box<Error>;

        fn try_from(result: RequestFilterResult) -> Result<Self, Self::Error> {
            Ok(Self { result })
        }
    }

    fn handler(add_default: bool) -> VirtualHostsHandler<Handler> {
        let mut vhosts = HashMap::new();
        vhosts.insert(
            "localhost:8080".to_owned(),
            HostConfig::<RequestFilterResult> {
                host: VirtualHostConf {
                    aliases: vec!["127.0.0.1:8080".to_owned(), "[::1]:8080".to_owned()],
                    default: add_default,
                },
                config: RequestFilterResult::ResponseSent,
            },
        );
        vhosts.insert(
            "example.com".to_owned(),
            HostConfig::<RequestFilterResult> {
                host: VirtualHostConf {
                    aliases: vec!["example.com:8080".to_owned()],
                    default: false,
                },
                config: RequestFilterResult::Handled,
            },
        );

        VirtualHostsConf::<RequestFilterResult> { vhosts }
            .try_into()
            .unwrap()
    }

    async fn make_session(uri: &str, host: Option<&str>) -> Session {
        let mut mock = Builder::new();

        mock.read(format!("GET {uri} HTTP/1.1\r\n").as_bytes());
        if let Some(host) = host {
            mock.read(format!("Host: {host}\r\n").as_bytes());
        }
        mock.read(b"Connection: close\r\n");
        mock.read(b"\r\n");

        let mut session = Session::new_h1(Box::new(mock.build()));
        assert!(session.read_request().await.unwrap());

        // Set URI explicitly, otherwise with a H1 session it will all end up in the path.
        session.req_header_mut().set_uri(uri.try_into().unwrap());

        session
    }

    #[test(tokio::test)]
    async fn host_match() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("/", Some("example.com")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Handled
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn host_alias_match() -> Result<(), Box<Error>> {
        let handler = handler(false);
        let mut session = make_session("/", Some("[::1]:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn uri_match() -> Result<(), Box<Error>> {
        let handler = handler(false);
        let mut session = make_session("https://example.com/", None).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Handled
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn uri_alias_match() -> Result<(), Box<Error>> {
        let handler = handler(false);
        let mut session = make_session("http://[::1]:8080/", None).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn host_precedence() -> Result<(), Box<Error>> {
        let handler = handler(false);
        let mut session = make_session("https://localhost:8080/", Some("example.com")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Handled
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn default_fallback() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("/", Some("example.net")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn no_default_fallback() -> Result<(), Box<Error>> {
        let handler = handler(false);
        let mut session = make_session("/", Some("example.net")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        Ok(())
    }
}
