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
use log::{debug, trace};
use module_utils::pingora::{Error, ResponseHeader, SessionWrapper};
use module_utils::router::Router;
use module_utils::{RequestFilter, RequestFilterResult};

use crate::configuration::{Header, HeadersConf};
use crate::processing::{IntoMergedConf, MergedConf};

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, PartialEq, Eq)]
pub struct HeadersHandler {
    router: Router<MergedConf>,
    fallback_router: Router<MergedConf>,
}

impl TryFrom<HeadersConf> for HeadersHandler {
    type Error = Box<Error>;

    fn try_from(value: HeadersConf) -> Result<Self, Self::Error> {
        debug!("Headers configuration received: {value:#?}");

        let merged_cache_control = value.response_headers.cache_control.into_merged();
        let merged_custom = value.response_headers.custom.into_merged();
        let merged = (merged_cache_control, merged_custom).into_merged();
        trace!("Merged headers configuration into: {merged:#?}");

        let mut builder = Router::builder();
        let mut fallback_builder = Router::builder();
        for ((host, path), conf) in merged.into_iter() {
            if host.is_empty() {
                fallback_builder.push(&host, &path, conf);
            } else {
                builder.push(&host, &path, conf);
            }
        }
        let router = builder.build();
        let fallback_router = fallback_builder.build();

        Ok(Self {
            router,
            fallback_router,
        })
    }
}

#[async_trait]
impl RequestFilter for HeadersHandler {
    type Conf = HeadersConf;

    type CTX = Vec<Header>;

    fn new_ctx() -> Self::CTX {
        Vec::new()
    }

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        let list = {
            let path = session.req_header().uri.path();
            trace!(
                "Determining response headers for host/path combination {:?}{path}",
                session.host()
            );

            let match_ = session
                .host()
                .and_then(|host| self.router.lookup(host.as_ref(), path))
                .or_else(|| self.fallback_router.lookup("", path));

            if let Some((conf, tail)) = match_ {
                let tail = tail.as_ref().map(|t| t.as_ref()).unwrap_or(path.as_bytes());
                if tail == b"/" {
                    &conf.as_value().exact
                } else {
                    &conf.as_value().prefix
                }
            } else {
                return Ok(RequestFilterResult::Unhandled);
            }
        };

        session.extensions_mut().insert(list.clone());
        trace!("Prepared headers for response: {list:?}");

        Ok(RequestFilterResult::Unhandled)
    }

    fn request_filter_done(
        &self,
        session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
        result: RequestFilterResult,
    ) {
        if result != RequestFilterResult::ResponseSent {
            // Response hasn’t been sent, move the stored headers into context so that we can still
            // access them in the response_filter phase.
            if let Some(mut headers) = session.extensions_mut().remove() {
                trace!("Copying headers from extensions to context: {headers:?}");
                ctx.append(&mut headers);
            }
        }
    }

    fn response_filter(
        &self,
        session: &mut impl SessionWrapper,
        response: &mut ResponseHeader,
        ctx: Option<&mut <Self as RequestFilter>::CTX>,
    ) {
        if let Some(list) = ctx.or_else(|| session.extensions_mut().get_mut()) {
            trace!("Added headers to response: {list:?}");
            for (name, value) in list.iter() {
                // Conversion from HeaderName/HeaderValue is infallible, ignore errors.
                let _ = response.insert_header(name, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use http::header;
    use module_utils::pingora::{RequestHeader, TestSession};
    use module_utils::{DeserializeMap, FromYaml};
    use std::ops::{Deref, DerefMut};
    use test_log::test;

    #[derive(Debug, Default, PartialEq, Eq, DeserializeMap)]
    struct TestConf {
        send_response: bool,
    }

    #[derive(Debug)]
    struct TestHandler {
        conf: TestConf,
    }

    impl TryFrom<TestConf> for TestHandler {
        type Error = Box<Error>;

        fn try_from(conf: TestConf) -> Result<Self, Self::Error> {
            Ok(TestHandler { conf })
        }
    }

    #[async_trait]
    impl RequestFilter for TestHandler {
        type Conf = TestConf;
        type CTX = ();
        fn new_ctx() -> Self::CTX {}

        async fn request_filter(
            &self,
            session: &mut impl SessionWrapper,
            _ctx: &mut Self::CTX,
        ) -> Result<RequestFilterResult, Box<Error>> {
            if self.conf.send_response {
                let header = make_response_header()?;
                session.write_response_header(Box::new(header)).await?;

                Ok(RequestFilterResult::ResponseSent)
            } else {
                Ok(RequestFilterResult::Handled)
            }
        }
    }

    #[derive(Debug, RequestFilter)]
    struct Handler {
        headers: HeadersHandler,
        test: TestHandler,
    }

    fn make_handler(send_response: bool) -> Handler {
        <Handler as RequestFilter>::Conf::from_yaml(format!(
            r#"
                send_response: {send_response}
                response_headers:
                    cache_control:
                    -
                        max-age: 200
                        public: true
                        include: example.com/subdir/*
                        exclude: example.com/subdir/subsub/*
                    -
                        max-age: 300
                        include: example.com/subdir/file.txt
                    -
                        no-storage: true
                        include: example.net
                    -
                        no-cache: true
                        include:
                        - example.info/subdir/*
                        - localhost/subdir2/
                    custom:
                    -
                        include:
                        - localhost
                        - localhost:8080
                        exclude: localhost/subdir/*
                        X-Me: localhost
                        Cache-Control: max-age=604800
                    -
                        include: example.com
                        X-Me: example.com
                    -
                        Server: My very own web server
            "#,
        ))
        .unwrap()
        .try_into()
        .unwrap()
    }

    async fn make_session(path: &str) -> TestSession {
        let mut header = RequestHeader::build("GET", path.as_bytes(), None).unwrap();

        // Set URI explicitly, making sure the host name is preserved.
        header.set_uri(path.try_into().unwrap());

        TestSession::from(header).await
    }

    fn make_response_header() -> Result<ResponseHeader, Box<Error>> {
        let mut header = ResponseHeader::build(200, None)?;
        header.insert_header("X-Me", "none")?;
        header.insert_header("X-Test", "unchanged")?;
        Ok(header)
    }

    fn assert_headers(header: &ResponseHeader, expected: Vec<(&str, &str)>) {
        let mut headers: Vec<_> = header
            .headers
            .iter()
            .filter(|(name, _)| *name != header::CONNECTION && *name != header::DATE)
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    value.to_str().unwrap().to_owned(),
                )
            })
            .collect();
        headers.sort();

        let mut expected: Vec<_> = expected
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
            .collect();
        expected.sort();

        assert_eq!(headers, expected);
    }

    #[test(tokio::test)]
    async fn request_filter() -> Result<(), Box<Error>> {
        let handler = make_handler(true);

        let mut session = make_session("https://localhost/").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "max-age=604800"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://localhost/subdir/file.txt").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://localhost/subdir2").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "no-cache, max-age=604800"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.com/whatever").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.com/subdir/whatever").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=200, public"),
            ],
        );

        let mut session = make_session("https://example.com/subdir/file.txt").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=300, public"),
            ],
        );

        let mut session = make_session("https://example.com/subdir/subsub/file.txt").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.net/whatever").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-storage"),
            ],
        );

        let mut session = make_session("https://example.info/whatever").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.info/subdir/whatever").await;
        assert!(
            handler
                .call_request_filter(session.deref_mut(), &mut Handler::new_ctx())
                .await?
        );
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-cache"),
            ],
        );

        Ok(())
    }

    #[test(tokio::test)]
    async fn upstream() -> Result<(), Box<Error>> {
        let handler = make_handler(false);

        let mut session = make_session("https://localhost/").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "max-age=604800"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://localhost/subdir/file.txt").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://localhost/subdir2").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "no-cache, max-age=604800"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.com/whatever").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.com/subdir/whatever").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=200, public"),
            ],
        );

        let mut session = make_session("https://example.com/subdir/file.txt").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=300, public"),
            ],
        );

        let mut session = make_session("https://example.com/subdir/subsub/file.txt").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.net/whatever").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-storage"),
            ],
        );

        let mut session = make_session("https://example.info/whatever").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.info/subdir/whatever").await;
        let mut ctx = Handler::new_ctx();
        assert!(
            !handler
                .call_request_filter(session.deref_mut(), &mut ctx)
                .await?
        );
        let mut header = make_response_header().unwrap();
        handler.call_response_filter(session.deref_mut(), &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-cache"),
            ],
        );

        Ok(())
    }
}
