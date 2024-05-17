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
#[derive(Debug)]
pub struct HeadersHandler {
    router: Router<MergedConf>,
    fallback_router: Router<MergedConf>,
}

impl TryFrom<HeadersConf> for HeadersHandler {
    type Error = Box<Error>;

    fn try_from(value: HeadersConf) -> Result<Self, Self::Error> {
        debug!("Headers configuration received: {value:#?}");

        let merged = value.custom_headers.into_merged();
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
                    &conf.exact
                } else {
                    &conf.prefix
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
