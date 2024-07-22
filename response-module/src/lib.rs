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
use headers_module::configuration::CustomHeadersConf;
use http::{header, HeaderName, HeaderValue, StatusCode};
use pandora_module_utils::pingora::{ResponseHeader, SessionWrapper};
use pandora_module_utils::{pingora::Error, RequestFilterResult};
use pandora_module_utils::{DeserializeMap, RequestFilter};
use serde::de::{Deserialize, Deserializer, Unexpected};

fn deserialize_status_code<'de, D>(deserializer: D) -> Result<StatusCode, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let status = u16::deserialize(deserializer)?;
    status.try_into().map_err(|_| {
        D::Error::invalid_value(Unexpected::Unsigned(status.into()), &"an HTTP status code")
    })
}

/// Configuration file settings of the response module
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct ResponseConf {
    /// The response text
    pub response: Option<String>,
    /// HTTP status code of the response
    #[pandora(deserialize_with = "deserialize_status_code")]
    pub response_status: StatusCode,
    /// HTTP headers to add to the response if any
    pub response_headers: CustomHeadersConf,
}

/// Response module handler
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHandler {
    response: Option<String>,
    response_status: StatusCode,
    response_headers: Vec<(HeaderName, HeaderValue)>,
}

impl TryFrom<ResponseConf> for ResponseHandler {
    type Error = Box<Error>;

    fn try_from(conf: ResponseConf) -> Result<Self, Self::Error> {
        Ok(Self {
            response: conf.response,
            response_status: conf.response_status,
            response_headers: conf.response_headers.headers.into_iter().collect(),
        })
    }
}

#[async_trait]
impl RequestFilter for ResponseHandler {
    type Conf = ResponseConf;

    type CTX = ();

    fn new_ctx() -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        if let Some(response) = &self.response {
            let mut response_header =
                ResponseHeader::build(self.response_status, Some(self.response_headers.len() + 1))?;
            for (name, value) in &self.response_headers {
                response_header.insert_header(name, value)?;
            }
            response_header.insert_header(header::CONTENT_LENGTH, response.len())?;
            session
                .write_response_header(Box::new(response_header), false)
                .await?;
            session
                .write_response_body(Some(response.clone().into()), true)
                .await?;
            Ok(RequestFilterResult::ResponseSent)
        } else {
            Ok(RequestFilterResult::Unhandled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pandora_module_utils::{
        pingora::{create_test_session, ErrorType, RequestHeader, Session},
        FromYaml,
    };
    use startup_module::DefaultApp;
    use test_log::test;

    fn make_app(conf: &str) -> DefaultApp<ResponseHandler> {
        DefaultApp::new(
            <ResponseHandler as RequestFilter>::Conf::from_yaml(conf)
                .unwrap()
                .try_into()
                .unwrap(),
        )
    }

    async fn make_session() -> Session {
        let header = RequestHeader::build("GET", b"/", None).unwrap();
        create_test_session(header).await
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
    async fn unconfigured() {
        let mut app = make_app("{}");
        let session = make_session().await;
        let result = app.handle_request(session).await;
        assert_eq!(
            result.err().as_ref().map(|err| &err.etype),
            Some(&ErrorType::HTTPStatus(404))
        );
    }

    #[test(tokio::test)]
    async fn configured() {
        let mut app = make_app("response: hi");
        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_eq!(result.body_str(), "hi");

        let session = result.session();
        let response = session.response_written().unwrap();
        assert_eq!(response.status, 200);
        assert_headers(response, vec![("Content-Length", "2")]);
    }

    #[test(tokio::test)]
    async fn custom_status() {
        let mut app = make_app(
            r#"
                response: ""
                response_status: 201
            "#,
        );
        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_eq!(result.body_str(), "");

        let session = result.session();
        let response = session.response_written().unwrap();
        assert_eq!(response.status, 201);
        assert_headers(response, vec![("Content-Length", "0")]);
    }

    #[test(tokio::test)]
    async fn with_headers() {
        let mut app = make_app(
            r#"
                response: hi
                response_headers:
                    Content-Type: text/plain
                    X-Custom: custom
            "#,
        );
        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert!(result.err().is_none());
        assert_eq!(result.body_str(), "hi");

        let session = result.session();
        let response = session.response_written().unwrap();
        assert_eq!(response.status, 200);
        assert_headers(
            response,
            vec![
                ("Content-Length", "2"),
                ("Content-Type", "text/plain"),
                ("X-Custom", "custom"),
            ],
        );
    }
}
