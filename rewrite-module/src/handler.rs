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

//! Handler for the `request_filter` phase.

use async_trait::async_trait;
use http::StatusCode;
use log::{debug, error, trace};
use pandora_module_utils::merger::Merger;
use pandora_module_utils::pingora::{Error, SessionWrapper};
use pandora_module_utils::router::{Path, Router};
use pandora_module_utils::standard_response::redirect_response;
use pandora_module_utils::{RequestFilter, RequestFilterResult};

use crate::configuration::{RegexMatch, RewriteConf, RewriteType, Variable, VariableInterpolation};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    from_regex: Option<RegexMatch>,
    query_regex: Option<RegexMatch>,
    to: VariableInterpolation,
    r#type: RewriteType,
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteHandler {
    router: Router<Vec<(Path, Rule)>>,
}

impl TryFrom<RewriteConf> for RewriteHandler {
    type Error = Box<Error>;

    fn try_from(mut conf: RewriteConf) -> Result<Self, Self::Error> {
        debug!("Rewrite configuration received: {conf:#?}");

        let mut merger = Merger::new();

        // Add in reverse order, so that the first rule listed in configuration takes precedence.
        conf.rewrite_rules.reverse();

        // Sort by prefix so that exact rules get priority.
        conf.rewrite_rules.sort_by(|a, b| a.from.cmp(&b.from));

        for rule in conf.rewrite_rules {
            let path = rule.from.path.clone();
            let from = rule.from;
            let rule = Rule {
                from_regex: rule.from_regex,
                query_regex: rule.query_regex,
                to: rule.to,
                r#type: rule.r#type,
            };

            merger.push(from, (path, rule));
        }

        Ok(Self {
            router: merger.merge(|rules| rules.cloned().collect::<Vec<_>>()),
        })
    }
}

#[async_trait]
impl RequestFilter for RewriteHandler {
    type Conf = RewriteConf;

    type CTX = ();

    fn new_ctx() -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        let path = session.uri().path();
        trace!("Determining rewrite rules for path {path}");

        let list = if let Some(list) = self.router.lookup("", path) {
            list
        } else {
            trace!("No match for the path");
            return Ok(RequestFilterResult::Unhandled);
        };

        trace!("Applying rewrite rules: {list:?}");

        // Iterate in reverse order, merging puts rules in reverse order of precedence.
        for (rule_path, rule) in list.iter().rev() {
            if let Some(from_regex) = &rule.from_regex {
                if !from_regex.matches(session.uri().path()) {
                    continue;
                }
            }

            if let Some(query_regex) = &rule.query_regex {
                if !query_regex.matches(session.uri().query().unwrap_or("")) {
                    continue;
                }
            }

            trace!(
                "Matched rule for path `{}`",
                String::from_utf8_lossy(rule_path)
            );

            let target = rule.to.interpolate(|variable, result| match variable {
                Variable::Tail => {
                    if let Some(mut tail) = rule_path.remove_prefix_from(path) {
                        result.append(&mut tail);
                    } else {
                        result.extend_from_slice(path.as_bytes());
                    }
                }
                Variable::Query => {
                    if let Some(query) = session.uri().query() {
                        result.push(b'?');
                        result.extend_from_slice(query.as_bytes());
                    }
                }
                Variable::Header(name) => {
                    if let Some(value) = session.req_header().headers.get(name) {
                        result.extend_from_slice(value.as_bytes())
                    }
                }
            });

            match rule.r#type {
                RewriteType::Internal => {
                    let uri = match target.as_slice().try_into() {
                        Ok(uri) => uri,
                        Err(err) => {
                            error!("Could not parse {target:?} as URI: {err}");
                            return Ok(RequestFilterResult::Unhandled);
                        }
                    };
                    session.set_uri(uri);
                    break;
                }
                RewriteType::Redirect | RewriteType::Permanent => {
                    let location = match String::from_utf8(target) {
                        Ok(location) => location,
                        Err(err) => {
                            error!("Failed converting redirect target to UTF-8: {err}");
                            return Ok(RequestFilterResult::Unhandled);
                        }
                    };
                    let status = if rule.r#type == RewriteType::Redirect {
                        StatusCode::TEMPORARY_REDIRECT
                    } else {
                        StatusCode::PERMANENT_REDIRECT
                    };
                    redirect_response(session, status, &location).await?;
                    return Ok(RequestFilterResult::ResponseSent);
                }
            }
        }

        Ok(RequestFilterResult::Unhandled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pandora_module_utils::pingora::{create_test_session, ErrorType, RequestHeader, Session};
    use pandora_module_utils::FromYaml;
    use startup_module::DefaultApp;
    use test_log::test;

    fn make_app(conf: &str) -> DefaultApp<RewriteHandler> {
        DefaultApp::new(
            <RewriteHandler as RequestFilter>::Conf::from_yaml(conf)
                .unwrap()
                .try_into()
                .unwrap(),
        )
    }

    async fn make_session(path: &str) -> Session {
        let header = RequestHeader::build("GET", path.as_bytes(), None).unwrap();

        create_test_session(header).await
    }

    #[test(tokio::test)]
    async fn internal_redirect() {
        let mut app = make_app(
            r#"
                rewrite_rules:
                    from: /path/*
                    to: /another${tail}
            "#,
        );

        let session = make_session("/").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/");
        assert_eq!(result.session().original_uri(), "/");

        let session = make_session("/path").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/another/");
        assert_eq!(result.session().original_uri(), "/path");

        let session = make_session("/path/").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/another/");
        assert_eq!(result.session().original_uri(), "/path/");

        let session = make_session("/path/file.txt").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/another/file.txt");
        assert_eq!(result.session().original_uri(), "/path/file.txt");
    }

    #[test(tokio::test)]
    async fn conditions() {
        let mut app = make_app(
            r#"
                rewrite_rules:
                -
                    from: /path/*
                    from_regex: "\\.jpg$"
                    to: /another${tail}
                -
                    from: /path/image.jpg
                    query_regex: "query"
                    to: /nowhere
                -
                    from: /path/*
                    query_regex: "!^file="
                    to: /different${query}
                -
                    from: /*
                    from_regex: ^/file\.txt$
                    query_regex: "!no_redirect"
                    to: /other.txt
            "#,
        );

        let session = make_session("/path/image.jpg").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/another/image.jpg");

        let session = make_session("/path/?a=b").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/different?a=b");

        let session = make_session("/path/image.png?a=b&file=c").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/different?a=b&file=c");

        let session = make_session("/path/image.png?file=c").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/path/image.png?file=c");

        let session = make_session("/file.txt").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/other.txt");

        let session = make_session("/file.txt?no_redirect").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/file.txt?no_redirect");
    }

    #[test(tokio::test)]
    async fn interpolation() {
        let mut app = make_app(
            r#"
                rewrite_rules:
                    from: /path/*
                    to: /another${tail}${tail}${query}&host=${http_host}&test=${http_test_header}
            "#,
        );

        let session = make_session("/path/file.txt").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(
            result.session().uri(),
            "/another/file.txt/file.txt&host=&test="
        );

        let session = make_session("/path/file.txt?a=b").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(
            result.session().uri(),
            "/another/file.txt/file.txt?a=b&host=&test="
        );

        let mut session = make_session("/path/file.txt?a=b").await;
        session
            .req_header_mut()
            .insert_header("Host", "localhost")
            .unwrap();
        session
            .req_header_mut()
            .insert_header("Test-Header", "successful")
            .unwrap();
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(
            result.session().uri(),
            "/another/file.txt/file.txt?a=b&host=localhost&test=successful"
        );
    }

    #[test(tokio::test)]
    async fn external_redirect() {
        let mut app = make_app(
            r#"
                rewrite_rules:
                -
                    from: /path/*
                    to: /another${tail}
                    type: permanent
                -
                    from: /file.txt
                    to: https://example.com/${query}
                    type: redirect
            "#,
        );

        let session = make_session("/path/file.txt").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_eq!(
            result.session().response_written().map(|r| r.status),
            Some(StatusCode::PERMANENT_REDIRECT)
        );
        assert_eq!(
            result
                .session()
                .response_written()
                .and_then(|r| r.headers.get("Location"))
                .map(|h| h.to_str().unwrap()),
            Some("/another/file.txt")
        );

        let session = make_session("/file.txt?a=b").await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_eq!(
            result.session().response_written().map(|r| r.status),
            Some(StatusCode::TEMPORARY_REDIRECT)
        );
        assert_eq!(
            result
                .session()
                .response_written()
                .and_then(|r| r.headers.get("Location"))
                .map(|h| h.to_str().unwrap()),
            Some("https://example.com/?a=b")
        );
    }

    #[test(tokio::test)]
    async fn rule_order() {
        let mut app = make_app(
            r#"
                rewrite_rules:
                -
                    from: /*
                    query_regex: "1"
                    to: /1
                -
                    from: /path/*
                    query_regex: "2"
                    to: /2
                -
                    from: /path/*
                    query_regex: "3"
                    to: /3
                -
                    from: /path
                    query_regex: "4"
                    to: /4
                -
                    from: /path
                    query_regex: "5"
                    to: /5
            "#,
        );

        let session = make_session("/path?12345").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/4");

        let session = make_session("/path?1235").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/5");

        let session = make_session("/path?123").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/2");

        let session = make_session("/path?13").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/3");

        let session = make_session("/path?1").await;
        let mut result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
        assert_eq!(result.session().uri(), "/1");
    }
}
