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
use http::{HeaderValue, StatusCode};
use log::{debug, error, trace};
use module_utils::pingora::{Error, SessionWrapper};
use module_utils::router::Router;
use module_utils::standard_response::redirect_response;
use module_utils::{RequestFilter, RequestFilterResult};
use std::collections::HashMap;

use crate::configuration::{
    RegexMatch, RewriteConf, RewriteRule, RewriteType, VariableInterpolation,
};

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
    router: Router<(Vec<Rule>, Vec<Rule>)>,
}

impl TryFrom<RewriteConf> for RewriteHandler {
    type Error = Box<Error>;

    fn try_from(value: RewriteConf) -> Result<Self, Self::Error> {
        debug!("Rewrite configuration received: {value:#?}");

        let mut merged = HashMap::new();

        // Add all configured paths
        for rule in &value.rewrite_rules {
            merged.insert(
                rule.from.path.to_owned(),
                (Vec::<&RewriteRule>::new(), Vec::<&RewriteRule>::new()),
            );
        }

        // Add all rules matching respective paths
        for rule in &value.rewrite_rules {
            for (path, (list_exact, list_prefix)) in merged.iter_mut() {
                if rule.from.matches(path) {
                    list_exact.push(rule);
                    if rule.from.prefix {
                        list_prefix.push(rule);
                    }
                }
            }
        }

        trace!("Merged rewrite configuration into: {merged:#?}");

        fn convert_list(mut list: Vec<&RewriteRule>) -> Vec<Rule> {
            // Make sure more specific rules go first
            list.sort_by(|rule1, rule2| rule2.from.cmp(&rule1.from));
            list.into_iter()
                .map(|rule| Rule {
                    from_regex: rule.from_regex.clone(),
                    query_regex: rule.query_regex.clone(),
                    to: rule.to.clone(),
                    r#type: rule.r#type,
                })
                .collect()
        }

        let mut builder = Router::builder();
        for (path, (list_exact, list_prefix)) in merged.into_iter() {
            let value_exact = (convert_list(list_exact), convert_list(list_prefix));
            let value_prefix = value_exact.clone();
            builder.push("", &path, value_exact, Some(value_prefix));
        }

        Ok(Self {
            router: builder.build(),
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
        let (list, tail) = {
            let path = session.req_header().uri.path();
            trace!("Determining rewrite rules for path {path}");

            let ((list_exact, list_prefix), tail) =
                if let Some(match_) = self.router.lookup("", path) {
                    (match_.0.as_value(), match_.1)
                } else {
                    trace!("No match for the path");
                    return Ok(RequestFilterResult::Unhandled);
                };

            let tail = tail
                .as_ref()
                .map(|t| t.as_ref())
                .unwrap_or(path.as_bytes())
                .to_vec();
            let list = if tail == b"/" {
                list_exact
            } else {
                list_prefix
            };
            (list, tail)
        };

        trace!("Applying rewrite rules: {list:?}");
        trace!("Tail is: {tail:?}");

        for rule in list {
            if let Some(from_regex) = &rule.from_regex {
                if !from_regex.matches(session.req_header().uri.path()) {
                    continue;
                }
            }

            if let Some(query_regex) = &rule.query_regex {
                if !query_regex.matches(session.req_header().uri.query().unwrap_or("")) {
                    continue;
                }
            }

            let target = rule.to.interpolate(|name| match name {
                "tail" => Some(&tail),
                "query" => Some(session.req_header().uri.query().unwrap_or("").as_bytes()),
                name => {
                    if let Some(name) = name.strip_prefix("http_") {
                        Some(
                            session
                                .req_header()
                                .headers
                                .get(name.replace('_', "-"))
                                .map(HeaderValue::as_bytes)
                                .unwrap_or(b""),
                        )
                    } else {
                        None
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
                    session.req_header_mut().set_uri(uri);
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

    use module_utils::pingora::{RequestHeader, TestSession};
    use module_utils::FromYaml;
    use test_log::test;

    fn make_handler(conf: &str) -> RewriteHandler {
        <RewriteHandler as RequestFilter>::Conf::from_yaml(conf)
            .unwrap()
            .try_into()
            .unwrap()
    }

    async fn make_session(path: &str) -> TestSession {
        let header = RequestHeader::build("GET", path.as_bytes(), None).unwrap();

        TestSession::from(header).await
    }

    #[test(tokio::test)]
    async fn internal_redirect() -> Result<(), Box<Error>> {
        let handler = make_handler(
            r#"
                rewrite_rules:
                -
                    from: /path/*
                    to: /another${tail}
            "#,
        );

        let mut session = make_session("/").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/");

        let mut session = make_session("/path").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/another/");

        let mut session = make_session("/path/").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/another/");

        let mut session = make_session("/path/file.txt").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/another/file.txt");

        Ok(())
    }

    #[test(tokio::test)]
    async fn conditions() -> Result<(), Box<Error>> {
        let handler = make_handler(
            r#"
                rewrite_rules:
                -
                    from: /path/*
                    from_regex: "\\.jpg$"
                    to: /another${tail}
                -
                    from: /path/*
                    query_regex: "!^file="
                    to: /different?${query}
                -
                    from: /*
                    from_regex: ^/file\.txt$
                    query_regex: "!no_redirect"
                    to: /other.txt
            "#,
        );

        let mut session = make_session("/path/image.jpg").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/another/image.jpg");

        let mut session = make_session("/path/?a=b").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/different?a=b");

        let mut session = make_session("/path/image.png?a=b&file=c").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.req_header().uri.to_string(),
            "/different?a=b&file=c"
        );

        let mut session = make_session("/path/image.png?file=c").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.req_header().uri.to_string(),
            "/path/image.png?file=c"
        );

        let mut session = make_session("/file.txt").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/other.txt");

        let mut session = make_session("/file.txt?no_redirect").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.req_header().uri.to_string(),
            "/file.txt?no_redirect"
        );

        Ok(())
    }

    #[test(tokio::test)]
    async fn interpolation() -> Result<(), Box<Error>> {
        let handler = make_handler(
            r#"
                rewrite_rules:
                -
                    from: /path/*
                    to: /another${tail}?${query}&host=${http_host}&test=${http_test_header}
            "#,
        );

        let mut session = make_session("/path/file.txt").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.req_header().uri.to_string(),
            "/another/file.txt?&host=&test="
        );

        let mut session = make_session("/path/file.txt?a=b").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.req_header().uri.to_string(),
            "/another/file.txt?a=b&host=&test="
        );

        let mut session = make_session("/path/file.txt?a=b").await;
        session
            .req_header_mut()
            .insert_header("Host", "localhost")?;
        session
            .req_header_mut()
            .insert_header("Test-Header", "successful")?;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(
            session.req_header().uri.to_string(),
            "/another/file.txt?a=b&host=localhost&test=successful"
        );

        Ok(())
    }

    #[test(tokio::test)]
    async fn external_redirect() -> Result<(), Box<Error>> {
        let handler = make_handler(
            r#"
                rewrite_rules:
                -
                    from: /path/*
                    to: /another${tail}
                    type: permanent
                -
                    from: /file.txt
                    to: https://example.com/?${query}
                    type: redirect
            "#,
        );

        let mut session = make_session("/path/file.txt").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(
            session.response_written().map(|r| r.status),
            Some(StatusCode::PERMANENT_REDIRECT)
        );
        assert_eq!(
            session
                .response_written()
                .and_then(|r| r.headers.get("Location"))
                .map(|h| h.to_str().unwrap()),
            Some("/another/file.txt")
        );

        let mut session = make_session("/file.txt?a=b").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(
            session.response_written().map(|r| r.status),
            Some(StatusCode::TEMPORARY_REDIRECT)
        );
        assert_eq!(
            session
                .response_written()
                .and_then(|r| r.headers.get("Location"))
                .map(|h| h.to_str().unwrap()),
            Some("https://example.com/?a=b")
        );

        Ok(())
    }

    #[test(tokio::test)]
    async fn rule_order() -> Result<(), Box<Error>> {
        let handler = make_handler(
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

        let mut session = make_session("/path?12345").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/4");

        let mut session = make_session("/path?1235").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/5");

        let mut session = make_session("/path?123").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/2");

        let mut session = make_session("/path?13").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/3");

        let mut session = make_session("/path?1").await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut RewriteHandler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        assert_eq!(session.req_header().uri.to_string(), "/1");

        Ok(())
    }
}
