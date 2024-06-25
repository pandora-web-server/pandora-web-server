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
use pandora_module_utils::pingora::{Error, HttpPeer, ResponseHeader, SessionWrapper};
use pandora_module_utils::router::{Path, Router};
use pandora_module_utils::{RequestFilter, RequestFilterResult};
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

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

/// Context for the virtual hosts handler
#[derive(Debug)]
pub struct VirtualHostsCtx<Ctx> {
    index: Option<usize>,
    handler: Ctx,
}

impl<Ctx> Deref for VirtualHostsCtx<Ctx> {
    type Target = Ctx;

    fn deref(&self) -> &Self::Target {
        &self.handler
    }
}

impl<Ctx> DerefMut for VirtualHostsCtx<Ctx> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.handler
    }
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualHostsHandler<H: Debug> {
    handlers: Router<(Option<Path>, H)>,
}

impl<H: Debug> VirtualHostsHandler<H> {
    /// Retrieves the handler which was previously called for this virtual host.
    ///
    /// This will return `None` if the `request_filter` handler wasn’t called for this context yet
    /// or it didn’t find a matching handler.
    pub fn as_inner(&self, ctx: &<Self as RequestFilter>::CTX) -> Option<&H>
    where
        H: RequestFilter + Sync,
        H::Conf: Default,
        H::CTX: Send,
    {
        self.handlers.retrieve(ctx.index?).map(|(_, h)| h)
    }
}

struct Marker;
type IndexEntry = (usize, PhantomData<Marker>);

#[async_trait]
impl<H> RequestFilter for VirtualHostsHandler<H>
where
    H: RequestFilter + Sync + Debug,
    H::Conf: Default,
    H::CTX: Send,
{
    type Conf = VirtualHostsConf<H::Conf>;

    type CTX = VirtualHostsCtx<H::CTX>;

    fn new_ctx() -> Self::CTX {
        Self::CTX {
            index: None,
            handler: H::new_ctx(),
        }
    }

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        let path = session.req_header().uri.path();
        let host = session.host().unwrap_or_default();

        if let Some(result) = self.handlers.lookup(host.as_ref(), &path) {
            let (strip_path, handler) = result.as_value();
            let index = result.index();
            let new_path = strip_path.as_ref().and_then(|p| p.remove_prefix_from(path));

            ctx.index = Some(index);

            // Save ctx.index in session as well, response_filter could be called without context
            session
                .extensions_mut()
                .insert::<IndexEntry>((index, PhantomData::<Marker>));

            if let Some(new_path) = new_path {
                // Capture original URI, logging might need it
                let orig_uri = session.req_header().uri.clone();
                session.extensions_mut().insert(orig_uri);

                let header = session.req_header_mut();
                header.set_uri(set_uri_path(&header.uri, &new_path));
            }
            handler.request_filter(session, ctx).await
        } else {
            Ok(RequestFilterResult::Unhandled)
        }
    }

    async fn upstream_peer(
        &self,
        session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
    ) -> Result<Option<Box<HttpPeer>>, Box<Error>> {
        if let Some(handler) = self.as_inner(ctx) {
            handler.upstream_peer(session, ctx).await
        } else {
            Ok(None)
        }
    }

    fn response_filter(
        &self,
        session: &mut impl SessionWrapper,
        response: &mut ResponseHeader,
        ctx: Option<&mut Self::CTX>,
    ) {
        let handler = ctx
            .as_ref()
            .and_then(|ctx| ctx.index)
            .or_else(|| session.extensions().get::<IndexEntry>().map(|(i, _)| *i))
            .and_then(|index| self.handlers.retrieve(index))
            .map(|(_, h)| h);
        if let Some(handler) = handler {
            handler.response_filter(session, response, ctx.map(|ctx| ctx.deref_mut()));
        }
    }

    async fn logging(
        &self,
        session: &mut impl SessionWrapper,
        e: Option<&Error>,
        ctx: &mut Self::CTX,
    ) {
        if let Some(handler) = self.as_inner(ctx) {
            handler.logging(session, e, ctx).await;
        }
    }
}

impl<C, H> TryFrom<VirtualHostsConf<C>> for VirtualHostsHandler<H>
where
    H: Debug + Clone + Eq,
    C: TryInto<H, Error = Box<Error>> + Default,
{
    type Error = Box<Error>;

    fn try_from(conf: VirtualHostsConf<C>) -> Result<Self, Box<Error>> {
        let mut handlers = Router::builder();
        let mut default = None;
        for (host, host_conf) in conf.vhosts.into_iter() {
            if host.is_empty() {
                warn!("ignoring empty host name in virtual hosts configuration, please use `default` setting instead");
                continue;
            }

            let mut aliases = BTreeSet::new();
            for alias in host_conf.aliases {
                if alias.is_empty() {
                    warn!("ignoring empty alias for host name {host}, please use `default` setting instead");
                } else {
                    aliases.insert(alias);
                }
            }
            if host_conf.default {
                if let Some(previous) = &default {
                    warn!("both {previous} and {host} are marked as default virtual host, ignoring the latter");
                } else {
                    default = Some(host.clone());
                    aliases.insert(String::new());
                }
            }

            let handler = host_conf.config.try_into()?;
            for alias in &aliases {
                handlers.push(
                    alias,
                    "",
                    (None, handler.clone()),
                    Some((None, handler.clone())),
                );
            }
            handlers.push(&host, "", (None, handler.clone()), Some((None, handler)));

            let mut subpaths = host_conf.subpaths.into_iter().collect::<Vec<_>>();

            // Make sure to add exact match rules last so that these take precedence over prefix
            // rules. This also ensures that these rules are merged with the right prefix rule
            // because these are all added already.
            subpaths.sort_by_key(|(rule, _)| rule.exact);

            for (rule, conf) in subpaths {
                let handler = conf.config.try_into()?;
                let strip_path = if conf.strip_prefix {
                    Some(Path::new(&rule.path))
                } else {
                    None
                };
                for alias in &aliases {
                    handlers.push(
                        alias,
                        &rule.path,
                        (strip_path.clone(), handler.clone()),
                        if rule.exact {
                            None
                        } else {
                            Some((strip_path.clone(), handler.clone()))
                        },
                    );
                }

                let handler_prefix = if rule.exact {
                    None
                } else {
                    Some((strip_path.clone(), handler.clone()))
                };
                handlers.push(&host, &rule.path, (strip_path, handler), handler_prefix);
            }
        }
        let handlers = handlers.build();

        Ok(Self { handlers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pandora_module_utils::pingora::{RequestHeader, TestSession};
    use pandora_module_utils::{DeserializeMap, FromYaml};
    use test_log::test;

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct Conf {
        result: RequestFilterResult,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Handler {
        result: RequestFilterResult,
    }

    #[async_trait]
    impl RequestFilter for Handler {
        type Conf = Conf;
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

    impl TryFrom<Conf> for Handler {
        type Error = Box<Error>;

        fn try_from(conf: Conf) -> Result<Self, Self::Error> {
            Ok(Self {
                result: conf.result,
            })
        }
    }

    fn handler(
        add_default: bool,
    ) -> (
        VirtualHostsHandler<Handler>,
        <VirtualHostsHandler<Handler> as RequestFilter>::CTX,
    ) {
        (
            VirtualHostsConf::<Conf>::from_yaml(format!(
                r#"
                vhosts:
                    localhost:8080:
                        aliases: ["127.0.0.1:8080", "[::1]:8080"]
                        default: {add_default}
                        result: ResponseSent
                        subpaths:
                            /subdir/*:
                                strip_prefix: true
                                result: Unhandled
                            /subdir/file.txt:
                                result: ResponseSent
                            /subdir/subsub/*:
                                result: Handled
                    example.com:
                        aliases: ["example.com:8080"]
                        result: Handled
            "#
            ))
            .unwrap()
            .try_into()
            .unwrap(),
            VirtualHostsHandler::<Handler>::new_ctx(),
        )
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
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/", Some("example.com")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Handled
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn host_alias_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(false);
        let mut session = make_session("/", Some("[::1]:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::ResponseSent
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn uri_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(false);
        let mut session = make_session("https://example.com/", None).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Handled
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn uri_alias_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(false);
        let mut session = make_session("http://[::1]:8080/", None).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::ResponseSent
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn host_precedence() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(false);
        let mut session = make_session("https://localhost:8080/", Some("example.com")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Handled
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn default_fallback() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/", Some("example.net")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::ResponseSent
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn no_default_fallback() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(false);
        let mut session = make_session("/", Some("example.net")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir/", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/");
        assert_eq!(session.extensions().get::<Uri>().unwrap(), "/subdir/");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_match_without_slash() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/");
        assert_eq!(session.extensions().get::<Uri>().unwrap(), "/subdir");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_match_with_suffix() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir/xyz?abc", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/xyz?abc");
        assert_eq!(
            session.extensions().get::<Uri>().unwrap(),
            "/subdir/xyz?abc"
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_match_extra_slashes() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("//subdir///xyz//", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "///xyz//");
        assert_eq!(
            session.extensions().get::<Uri>().unwrap(),
            "//subdir///xyz//"
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_no_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir_xyz", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.req_header().uri, "/subdir_xyz");
        assert!(session.extensions().get::<Uri>().is_none());
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_longer_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir/subsub/xyz", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Handled
        );
        assert_eq!(session.req_header().uri, "/subdir/subsub/xyz");
        assert!(session.extensions().get::<Uri>().is_none());
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_alias_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(false);
        let mut session = make_session("/subdir/xyz?abc", Some("127.0.0.1:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/xyz?abc");
        assert_eq!(
            session.extensions().get::<Uri>().unwrap(),
            "/subdir/xyz?abc"
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn subdir_default_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir/xyz?abc", Some("unknown")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/xyz?abc");
        assert_eq!(
            session.extensions().get::<Uri>().unwrap(),
            "/subdir/xyz?abc"
        );
        Ok(())
    }

    #[test(tokio::test)]
    async fn subpath_exact_match() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir/file.txt", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.req_header().uri, "/subdir/file.txt");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subpath_exact_match_trailing_slash() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir/file.txt/", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.req_header().uri, "/subdir/file.txt/");
        Ok(())
    }

    #[test(tokio::test)]
    async fn subpath_exact_match_with_suffix() -> Result<(), Box<Error>> {
        let (handler, mut ctx) = handler(true);
        let mut session = make_session("/subdir/file.txt/xyz", Some("localhost:8080")).await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ctx).await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri, "/file.txt/xyz");
        assert_eq!(
            session.extensions().get::<Uri>().unwrap(),
            "/subdir/file.txt/xyz"
        );
        Ok(())
    }
}
