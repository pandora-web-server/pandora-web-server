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

//! A very basic Pandora Web Server module.
//!
//! This module processes a configured list of rules where each rule is associated with a text
//! response. Rules can contain parameters which can then be inserted into the response, e.g.:
//!
//! ```yaml
//! routes:
//!   /user/{id}: Hello, {id}!
//! ```
//!
//! **Important note**: Please don’t do this with HTML responses, this would be a Cross-Site
//! Scripting vulnerability.
//!
//! Routes can also be added via the `--add-route` command line parameter, e.g.:
//!
//! ```sh
//! cargo run -- --add-route "/user/{id}=Hello, {id}!"
//! ```

use async_trait::async_trait;
use clap::Parser;
use matchit::Router;
use pandora_module_utils::pingora::{Error, ErrorType, ResponseHeader, SessionWrapper};
use pandora_module_utils::{DeserializeMap, RequestFilter, RequestFilterResult};
use regex::{Captures, Regex};
use std::collections::HashMap;

/// Configuration file options
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub(crate) struct WebAppConf {
    /// Routes to be handled and the corresponding responses
    routes: HashMap<String, String>,
}

impl WebAppConf {
    pub(crate) fn merge_with_opt(&mut self, opt: WebAppOpt) {
        if let Some(add_route) = opt.add_route {
            for (key, value) in add_route {
                self.routes.insert(key, value);
            }
        }
    }
}

/// Command line options
#[derive(Debug, Parser)]
pub(crate) struct WebAppOpt {
    /// Add a route to be handled, e.g. "/user/{id}=Welcome, {id}!"
    #[clap(long, value_parser = WebAppOpt::parse_route)]
    add_route: Option<Vec<(String, String)>>,
}

impl WebAppOpt {
    fn parse_route(route: &str) -> Result<(String, String), Box<Error>> {
        let (route, value) = route.split_once('=').ok_or_else(|| {
            Error::explain(
                ErrorType::ReadError,
                "separator = between route and value is missing",
            )
        })?;
        Ok((route.to_owned(), value.to_owned()))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WebAppHandler {
    router: Router<String>,
    param_regex: Regex,
}

impl PartialEq for WebAppHandler {
    fn eq(&self, _other: &Self) -> bool {
        // Router instances cannot be compared
        false
    }
}

impl Eq for WebAppHandler {}

#[async_trait]
impl RequestFilter for WebAppHandler {
    type CTX = ();
    type Conf = WebAppConf;

    fn new_ctx() -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        let match_ = match self.router.at(session.uri().path()) {
            Ok(value) => value,
            Err(_) => return Ok(RequestFilterResult::Unhandled),
        };
        let value = self
            .param_regex
            .replace_all(match_.value, |captures: &Captures<'_>| {
                match_.params.get(&captures[1]).unwrap_or_default()
            })
            .to_string();

        let mut header = ResponseHeader::build(200, Some(2))?;
        header.insert_header("Content-Type", "text/plain")?;
        header.insert_header("Content-Length", value.len().to_string())?;
        session.write_response_header(Box::new(header), false).await?;
        session.write_response_body(Some(value.into()), true).await?;
        Ok(RequestFilterResult::ResponseSent)
    }
}

impl TryFrom<WebAppConf> for WebAppHandler {
    type Error = Box<Error>;

    fn try_from(conf: WebAppConf) -> Result<Self, Self::Error> {
        let mut router = Router::new();
        for (key, value) in conf.routes {
            router.insert(key, value).map_err(|err| {
                Error::because(ErrorType::InternalError, "failed adding route", err)
            })?;
        }

        let param_regex = Regex::new(r"\{(\w+)\}").unwrap();

        Ok(Self {
            router,
            param_regex,
        })
    }
}
