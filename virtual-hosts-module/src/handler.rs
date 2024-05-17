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
use http::uri::Uri;
use log::warn;
use module_utils::pingora::{Error, SessionWrapper};
use module_utils::router::Router;
use module_utils::{RequestFilter, RequestFilterResult};
use std::collections::HashMap;
use std::fmt::Debug;

use crate::configuration::VirtualHostsConf;

fn set_uri_path(uri: &Uri, path: &[u8]) -> Uri {
    let mut parts = uri.clone().into_parts();
    let mut path_and_query = String::from_utf8_lossy(path).to_string();
    let query = parts
        .path_and_query
        .as_ref()
        .and_then(|path_and_query| path_and_query.query());
    if let Some(query) = query {
        path_and_query.push('?');
        path_and_query.push_str(query);
    }
    parts.path_and_query = path_and_query.parse().ok();
    parts.try_into().unwrap_or_else(|_| uri.clone())
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug)]
pub struct VirtualHostsHandler<H: Debug> {
    handlers: Router<(bool, H)>,
    aliases: HashMap<String, String>,
    default: Option<String>,
}

impl<H: Debug> VirtualHostsHandler<H> {
    fn best_match(
        &self,
        host: impl AsRef<[u8]>,
        path: impl AsRef<[u8]>,
    ) -> Option<(&H, Option<Vec<u8>>)> {
        self.handlers
            .lookup(&host, &path)
            .map(|((strip_prefix, handler), tail)| {
                (
                    handler,
                    tail.filter(|_| *strip_prefix).map(|t| t.as_ref().to_vec()),
                )
            })
    }
}

#[async_trait]
impl<H> RequestFilter for VirtualHostsHandler<H>
where
    H: RequestFilter + Sync + Debug,
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
        session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        let path = session.req_header().uri.path();
        let handler = session
            .host()
            .and_then(|host| {
                if let Some(handler) = self.best_match(host.as_ref(), path) {
                    Some(handler)
                } else if let Some(alias) = self.aliases.get(host.as_ref()) {
                    self.best_match(alias, path)
                } else {
                    None
                }
            })
            .or_else(|| {
                self.default
                    .as_ref()
                    .and_then(|default| self.best_match(default, path))
            });

        if let Some((handler, new_path)) = handler {
            if let Some(new_path) = new_path {
                let header = session.req_header_mut();
                header.set_uri(set_uri_path(&header.uri, &new_path));
            }
            handler.request_filter(session, ctx).await
        } else {
            Ok(RequestFilterResult::Unhandled)
        }
    }
}

impl<C, H> TryFrom<VirtualHostsConf<C>> for VirtualHostsHandler<H>
where
    H: Debug,
    C: TryInto<H, Error = Box<Error>> + Default,
{
    type Error = Box<Error>;

    fn try_from(conf: VirtualHostsConf<C>) -> Result<Self, Box<Error>> {
        let mut handlers = Router::builder();
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
            handlers.push(&host, "", (false, host_conf.config.try_into()?));

            for (path, conf) in host_conf.host.subdirs {
                handlers.push(
                    &host,
                    path,
                    (conf.subdir.strip_prefix, conf.config.try_into()?),
                );
            }
        }
        let handlers = handlers.build();

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
    use crate::configuration::{SubDirCombined, SubDirConf, VirtualHostCombined, VirtualHostConf};

    use async_trait::async_trait;
    use module_utils::pingora::{RequestHeader, TestSession};
    use test_log::test;

    #[derive(Debug)]
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
            _session: &mut impl SessionWrapper,
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

        let mut subdirs = HashMap::new();
        subdirs.insert(
            "/subdir/".to_owned(),
            SubDirCombined::<RequestFilterResult> {
                subdir: SubDirConf { strip_prefix: true },
                config: RequestFilterResult::Unhandled,
            },
        );
        subdirs.insert(
            "/subdir/subsub".to_owned(),
            SubDirCombined::<RequestFilterResult> {
                subdir: SubDirConf {
                    strip_prefix: false,
                },
                config: RequestFilterResult::Handled,
            },
        );

        vhosts.insert(
            "localhost:8080".to_owned(),
            VirtualHostCombined::<RequestFilterResult> {
                host: VirtualHostConf {
                    aliases: vec!["127.0.0.1:8080".to_owned(), "[::1]:8080".to_owned()],
                    default: add_default,
                    subdirs,
                },
                config: RequestFilterResult::ResponseSent,
            },
        );

        vhosts.insert(
            "example.com".to_owned(),
            VirtualHostCombined::<RequestFilterResult> {
                host: VirtualHostConf {
                    aliases: vec!["example.com:8080".to_owned()],
                    default: false,
                    subdirs: HashMap::new(),
                },
                config: RequestFilterResult::Handled,
            },
        );

        VirtualHostsConf::<RequestFilterResult> { vhosts }
            .try_into()
            .unwrap()
    }

    async fn make_session(uri: &str, host: Option<&str>) -> TestSession {
        let header = RequestHeader::build("GET", uri.as_bytes(), None).unwrap();
        let mut session = TestSession::from(header).await;

        if let Some(host) = host {
            session
                .req_header_mut()
                .insert_header("Host", host)
                .unwrap();
        }

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

    #[test(tokio::test)]
    async fn subdir_match() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("/subdir/", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_match_without_slash() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("/subdir", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_match_with_suffix() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("/subdir/xyz?abc", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/xyz?abc");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_match_extra_slashes() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("//subdir///xyz//", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "///xyz//");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_no_match() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("/subdir_xyz", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.req_header().uri, "/subdir_xyz");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_longer_match() -> Result<(), Box<Error>> {
        let handler = handler(true);
        let mut session = make_session("/subdir/subsub/xyz", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Handled
        );
        assert_eq!(session.req_header().uri, "/subdir/subsub/xyz");
        Ok(())
    }
}
