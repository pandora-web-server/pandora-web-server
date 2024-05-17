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

//! # Module helpers
//!
//! This crate contains some helpers that are useful when using `static-files-module` or
//! `virtual-hosts-module` crates for example.

pub mod pingora;
pub mod router;
mod trie;

use async_trait::async_trait;
use log::trace;
use pingora::{wrap_session, Error, ErrorType, ResponseHeader, Session, SessionWrapper};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub use module_utils_macros::{merge_conf, merge_opt, RequestFilter};

/// Request filter result indicating how the current request should be processed further
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum RequestFilterResult {
    /// Response has been sent, no further processing should happen. Other Pingora phases should
    /// not be triggered.
    ResponseSent,

    /// Request has been handled and further request filters should not run. Response hasn’t been
    /// sent however, next Pingora phase should deal with that.
    Handled,

    /// Request filter could not handle this request, next request filter should run if it exists.
    #[default]
    Unhandled,
}

/// Trait to be implemented by request filters.
#[async_trait]
pub trait RequestFilter: Sized {
    /// Configuration type of this handler.
    type Conf;

    /// Creates a new instance of the handler from its configuration.
    fn new(conf: Self::Conf) -> Result<Self, Box<Error>>
    where
        Self: Sized,
        Self::Conf: TryInto<Self, Error = Box<Error>>,
    {
        conf.try_into()
    }

    /// Handles the `request_filter` phase of the current request.
    ///
    /// This will wrap the current session and call `request_filter` methods of the individual
    /// handlers then. It will then create a result that can be returned from the `request_filter`
    /// phase directly.
    async fn handle(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool, Box<Error>>
    where
        Self::CTX: Send,
    {
        let mut session = wrap_session(session, self);
        let result = self.request_filter(&mut session, ctx).await?;
        self.request_filter_done(&mut session, ctx, result);
        Ok(result == RequestFilterResult::ResponseSent)
    }

    /// Handles the `upstream_response_filter` or `response_filter` phase of the current request.
    ///
    /// This will wrap the current session and call `response_filter` methods of the individual
    /// handlers then.
    fn handle_response(
        &self,
        session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) where
        Self: Sync,
    {
        let mut session = wrap_session(session, self);
        self.response_filter(&mut session, response, Some(ctx))
    }

    /// Per-request state of this handler, see [`pingora_proxy::ProxyHttp::CTX`]
    type CTX;

    /// Creates a new sate object, see [`pingora_proxy::ProxyHttp::new_ctx`]
    fn new_ctx() -> Self::CTX;

    /// Handler to run during Pingora’s `request_filter` state, see
    /// [`pingora_proxy::ProxyHttp::request_filter`]. This uses a different return type to account
    /// for the existence of multiple request filters.
    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>>;

    /// Called after `request_filter` was called for all handlers and a result was produced. This
    /// allows the handler to perform some post-processing.
    fn request_filter_done(
        &self,
        _session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
        _result: RequestFilterResult,
    ) {
    }

    /// Called when a response header is about to be sent, either from a request filter or an
    /// upstream response.
    ///
    /// *Note*: A context will only be available for the latter call.
    fn response_filter(
        &self,
        _session: &mut impl SessionWrapper,
        _response: &mut ResponseHeader,
        _ctx: Option<&mut Self::CTX>,
    ) {
    }
}

/// Trait for configuration structures that can be loaded from YAML files. This trait has a blanket
/// implementation for any structure implementing [`serde::Deserialize`].
pub trait FromYaml {
    /// Loads configuration from a YAML file.
    fn load_from_yaml<P>(path: P) -> Result<Self, Box<Error>>
    where
        P: AsRef<Path>,
        Self: Sized;
}

impl<D> FromYaml for D
where
    D: DeserializeOwned + Debug + ?Sized,
{
    fn load_from_yaml<P: AsRef<Path>>(path: P) -> Result<Self, Box<Error>> {
        let file = File::open(path.as_ref()).map_err(|err| {
            Error::because(
                ErrorType::FileOpenError,
                "failed opening configuration file",
                err,
            )
        })?;
        let reader = BufReader::new(file);

        let conf = serde_yaml::from_reader(reader).map_err(|err| {
            Error::because(
                ErrorType::FileReadError,
                "failed reading configuration file",
                err,
            )
        })?;
        trace!("Loaded configuration file: {conf:#?}");

        Ok(conf)
    }
}
