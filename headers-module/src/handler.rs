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
use http::{HeaderName, HeaderValue};
use log::{debug, trace};
use pandora_module_utils::merger::{Merger, StrictHostPathMatcher};
use pandora_module_utils::pingora::{Error, ResponseHeader, SessionWrapper};
use pandora_module_utils::router::Router;
use pandora_module_utils::{OneOrMany, RequestFilter, RequestFilterResult};

use crate::configuration::{Header, HeadersConf, IntoHeaders, WithMatchRules};

fn merge_rules<C>(rules: OneOrMany<WithMatchRules<C>>) -> Merger<StrictHostPathMatcher, Vec<Header>>
where
    C: Default + Clone + Eq + IntoHeaders,
{
    let mut merger = Merger::new();
    for rule in rules {
        merger.push(rule.match_rules, rule.conf);
    }
    merger.merge_into_merger(|values| {
        let mut result = C::default();
        for conf in values {
            result.merge_with(conf);
        }
        result.into_headers()
    })
}

#[derive(Debug, Clone)]
struct HeadersList(Vec<Header>);

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadersHandler {
    router: Router<Vec<Header>>,
}

impl TryFrom<HeadersConf> for HeadersHandler {
    type Error = Box<Error>;

    fn try_from(value: HeadersConf) -> Result<Self, Self::Error> {
        debug!("Headers configuration received: {value:#?}");

        let cache_control = merge_rules(value.response_headers.cache_control);
        let content_security_policy = merge_rules(value.response_headers.content_security_policy);
        let custom = merge_rules(value.response_headers.custom);

        let mut merged = cache_control;
        merged.extend([content_security_policy, custom]);
        trace!("Merged headers configuration into: {merged:#?}");

        let router = merged.merge(|values| {
            let mut result = Vec::<(HeaderName, HeaderValue)>::new();
            for headers in values {
                for (name, value) in headers {
                    if let Some(existing) = result.iter().position(|(n, _)| n == name) {
                        // Combine duplicate headers
                        // https://datatracker.ietf.org/doc/html/rfc7230#section-3.2.2
                        let mut new_value = result[existing].1.as_bytes().to_vec();
                        new_value.extend_from_slice(b", ");
                        new_value.extend_from_slice(value.as_bytes());
                        result[existing].1 = HeaderValue::from_bytes(&new_value).unwrap();
                    } else {
                        result.push((name.clone(), value.clone()))
                    }
                }
            }
            result
        });

        Ok(Self { router })
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
        let path = session.uri().path();
        trace!(
            "Determining response headers for host/path combination {:?}{path}",
            session.host()
        );

        let host = session.host().unwrap_or_default();
        let list = if let Some(list) = self.router.lookup(host.as_ref(), path) {
            list.as_value()
        } else {
            return Ok(RequestFilterResult::Unhandled);
        };

        session.extensions_mut().insert(HeadersList(list.clone()));
        trace!("Prepared headers for response: {list:?}");

        Ok(RequestFilterResult::Unhandled)
    }

    fn response_filter(
        &self,
        session: &mut impl SessionWrapper,
        response: &mut ResponseHeader,
        _ctx: Option<&mut <Self as RequestFilter>::CTX>,
    ) {
        if let Some(HeadersList(list)) = session.extensions().get() {
            for (name, value) in list.iter() {
                // Conversion from HeaderName/HeaderValue is infallible, ignore errors.
                let _ = response.insert_header(name, value);
            }
            trace!("Added headers to response: {list:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use http::header;
    use pandora_module_utils::pingora::{ProxyHttp, RequestHeader, TestSession};
    use pandora_module_utils::{DeserializeMap, FromYaml};
    use startup_module::DefaultApp;
    use std::ops::Deref;
    use test_log::test;

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
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

    fn make_app(send_response: bool) -> DefaultApp<Handler> {
        DefaultApp::new(
            <Handler as RequestFilter>::Conf::from_yaml(format!(
                r#"
                send_response: {send_response}
                response_headers:
                    cache_control:
                    -
                        max-age: 300
                        include: example.com/subdir/file.txt
                    -
                        max-age: 200
                        public: true
                        include: example.com/subdir/*
                        exclude: example.com/subdir/subsub/*
                    -
                        no-storage: true
                        include: example.net
                    -
                        no-cache: true
                        include:
                        - example.info/subdir/*
                        - localhost/subdir2/
                    content_security_policy:
                    -
                        script-src: ["'self'"]
                        object-src: ["'none'"]
                        report-to: https://example.com/report
                        include: /*
                        exclude: example.com/subdir/*
                    -
                        script-src: [https://example.com/]
                        report-to: https://example.com/other-report
                        include: example.net
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
            .unwrap(),
        )
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
        let app = make_app(true);

        let mut session = make_session("https://localhost/").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "max-age=604800"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://localhost/subdir/file.txt").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://localhost/subdir2").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "max-age=604800, no-cache"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://example.com/whatever").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://example.com/subdir/whatever").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
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
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
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
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.net/whatever").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-storage"),
                ("Content-Security-Policy", "object-src 'none'; script-src 'self' https://example.com/; report-to https://example.com/other-report"),
            ],
        );

        let mut session = make_session("https://example.info/whatever").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://example.info/subdir/whatever").await;
        assert!(app.request_filter(&mut session, &mut app.new_ctx()).await?);
        assert_headers(
            session.deref().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-cache"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        Ok(())
    }

    #[test(tokio::test)]
    async fn upstream() -> Result<(), Box<Error>> {
        let app = make_app(false);

        let mut session = make_session("https://localhost/").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "max-age=604800"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://localhost/subdir/file.txt").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://localhost/subdir2").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "localhost"),
                ("Cache-Control", "max-age=604800, no-cache"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://example.com/whatever").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://example.com/subdir/whatever").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
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
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
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
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let mut session = make_session("https://example.net/whatever").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-storage"),
                ("Content-Security-Policy", "object-src 'none'; script-src 'self' https://example.com/; report-to https://example.com/other-report"),
            ],
        );

        let mut session = make_session("https://example.info/whatever").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        let mut session = make_session("https://example.info/subdir/whatever").await;
        let mut ctx = app.new_ctx();
        assert!(!app.request_filter(&mut session, &mut ctx).await?);
        let mut header = make_response_header().unwrap();
        app.upstream_response_filter(&mut session, &mut header, &mut ctx);
        assert_headers(
            &header,
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-cache"),
                (
                    "Content-Security-Policy",
                    "object-src 'none'; script-src 'self'; report-to https://example.com/report",
                ),
            ],
        );

        Ok(())
    }
}
