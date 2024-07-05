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
        trace!("Merged headers configuration into: {router:#?}");

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
    use pandora_module_utils::pingora::{create_test_session, HttpPeer, RequestHeader, Session};
    use pandora_module_utils::{DeserializeMap, FromYaml};
    use startup_module::DefaultApp;
    use test_log::test;
    use upstream_module::{UpstreamConf, UpstreamHandler};

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct TestConf {
        send_response: bool,
    }

    #[derive(Debug)]
    struct TestHandler {
        inner: Option<UpstreamHandler>,
    }

    impl TryFrom<TestConf> for TestHandler {
        type Error = Box<Error>;

        fn try_from(conf: TestConf) -> Result<Self, Self::Error> {
            let inner = if conf.send_response {
                None
            } else {
                Some(
                    UpstreamConf {
                        upstream: Some("http://127.0.0.1".try_into().unwrap()),
                    }
                    .try_into()
                    .unwrap(),
                )
            };
            Ok(TestHandler { inner })
        }
    }

    #[async_trait]
    impl RequestFilter for TestHandler {
        type Conf = TestConf;
        type CTX = <UpstreamHandler as RequestFilter>::CTX;
        fn new_ctx() -> Self::CTX {
            UpstreamHandler::new_ctx()
        }

        async fn request_filter(
            &self,
            session: &mut impl SessionWrapper,
            ctx: &mut Self::CTX,
        ) -> Result<RequestFilterResult, Box<Error>> {
            if let Some(inner) = &self.inner {
                inner.request_filter(session, ctx).await
            } else {
                let header = make_response_header()?;
                session.write_response_header(Box::new(header)).await?;

                Ok(RequestFilterResult::ResponseSent)
            }
        }

        async fn upstream_peer(
            &self,
            session: &mut impl SessionWrapper,
            ctx: &mut Self::CTX,
        ) -> Result<Option<Box<HttpPeer>>, Box<Error>> {
            if let Some(inner) = &self.inner {
                inner.upstream_peer(session, ctx).await
            } else {
                Ok(None)
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

    async fn make_session(path: &str) -> Session {
        let mut header = RequestHeader::build("GET", path.as_bytes(), None).unwrap();

        // Set URI explicitly, making sure the host name is preserved.
        header.set_uri(path.try_into().unwrap());

        create_test_session(header).await
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
    async fn request_filter() {
        let mut app = make_app(true);

        let session = make_session("https://localhost/").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://localhost/subdir/file.txt").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://localhost/subdir2").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://example.com/whatever").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://example.com/subdir/whatever").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=200, public"),
            ],
        );

        let session = make_session("https://example.com/subdir/file.txt").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=300, public"),
            ],
        );

        let session = make_session("https://example.com/subdir/subsub/file.txt").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let session = make_session("https://example.net/whatever").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-storage"),
                ("Content-Security-Policy", "object-src 'none'; script-src 'self' https://example.com/; report-to https://example.com/other-report"),
            ],
        );

        let session = make_session("https://example.info/whatever").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://example.info/subdir/whatever").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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
    }

    #[test(tokio::test)]
    async fn upstream() {
        let mut app = make_app(false);

        let session = make_session("https://localhost/").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://localhost/subdir/file.txt").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://localhost/subdir2").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://example.com/whatever").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://example.com/subdir/whatever").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=200, public"),
            ],
        );

        let session = make_session("https://example.com/subdir/file.txt").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "max-age=300, public"),
            ],
        );

        let session = make_session("https://example.com/subdir/subsub/file.txt").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "example.com"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
            ],
        );

        let session = make_session("https://example.net/whatever").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
            vec![
                ("X-Me", "none"),
                ("X-Test", "unchanged"),
                ("Server", "My very own web server"),
                ("Cache-Control", "no-storage"),
                ("Content-Security-Policy", "object-src 'none'; script-src 'self' https://example.com/; report-to https://example.com/other-report"),
            ],
        );

        let session = make_session("https://example.info/whatever").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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

        let session = make_session("https://example.info/subdir/whatever").await;
        let mut result = app
            .handle_request_with_upstream(session, |_, _| make_response_header())
            .await;
        assert!(result.err().is_none());
        assert_headers(
            result.session().response_written().unwrap(),
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
    }
}
