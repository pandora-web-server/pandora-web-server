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

mod configuration;
mod redirector;

use async_trait::async_trait;
pub use configuration::{
    CertKeyConf, ListenAddr, StartupConf, StartupOpt, TlsConf, TlsRedirectorConf,
};
use http::Extensions;
use pandora_module_utils::pingora::{
    Error, HttpPeer, ProxyHttp, ResponseHeader, Session, SessionWrapper,
};
use pandora_module_utils::{RequestFilter, RequestFilterResult};
use pingora::ErrorType;
use std::ops::{Deref, DerefMut};

/// A basic Pingora app implementation, to be passed to [`StartupConf::into_server`]
///
/// This app will only handle the `request_filter`, `upstream_peer`, `upstream_response_filter` and
/// `logging` phases. All processing will be delegated to the respective `RequestFilter` methods.
#[derive(Debug)]
pub struct DefaultApp<H> {
    handler: H,
}

impl<H> DefaultApp<H> {
    /// Creates a new app from a [`RequestFilter`] instance.
    pub fn new(handler: H) -> Self {
        Self { handler }
    }

    /// Creates a new app from a [`RequestFilter`] configuration.
    ///
    /// Any errors occurring when converting configuration to handler will be passed on.
    pub fn from_conf<C>(conf: C) -> Result<Self, Box<Error>>
    where
        H: RequestFilter<Conf = C> + TryFrom<C, Error = Box<Error>>,
    {
        Ok(Self::new(conf.try_into()?))
    }
}

/// Context for the default app
#[derive(Debug, Clone)]
pub struct DefaultCtx<C> {
    extensions: Extensions,
    handler: C,
}

#[async_trait]
impl<H> ProxyHttp for DefaultApp<H>
where
    H: RequestFilter + Sync,
    H::CTX: Send,
{
    type CTX = DefaultCtx<<H as RequestFilter>::CTX>;

    fn new_ctx(&self) -> Self::CTX {
        Self::CTX {
            extensions: Extensions::new(),
            handler: H::new_ctx(),
        }
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        let mut session = SessionWrapperImpl::new(session, &self.handler, &mut ctx.extensions);
        Ok(self
            .handler
            .request_filter(&mut session, &mut ctx.handler)
            .await?
            == RequestFilterResult::ResponseSent)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        let mut session = SessionWrapperImpl::new(session, &self.handler, &mut ctx.extensions);
        let result = self
            .handler
            .upstream_peer(&mut session, &mut ctx.handler)
            .await?;
        if let Some(result) = result {
            Ok(result)
        } else {
            Err(Error::new(ErrorType::HTTPStatus(404)))
        }
    }

    fn upstream_response_filter(
        &self,
        session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) {
        let mut session = SessionWrapperImpl::new(session, &self.handler, &mut ctx.extensions);
        self.handler
            .response_filter(&mut session, response, Some(&mut ctx.handler))
    }

    async fn logging(&self, session: &mut Session, e: Option<&Error>, ctx: &mut Self::CTX) {
        let mut session = SessionWrapperImpl::new(session, &self.handler, &mut ctx.extensions);
        self.handler
            .logging(&mut session, e, &mut ctx.handler)
            .await
    }
}

struct SessionWrapperImpl<'a, H> {
    inner: &'a mut Session,
    handler: &'a H,
    extensions: &'a mut Extensions,
}

impl<'a, H> SessionWrapperImpl<'a, H> {
    /// Creates a new session wrapper for the given Pingora session.
    fn new(inner: &'a mut Session, handler: &'a H, extensions: &'a mut Extensions) -> Self
    where
        H: RequestFilter,
    {
        Self {
            inner,
            handler,
            extensions,
        }
    }
}

#[async_trait]
impl<H> SessionWrapper for SessionWrapperImpl<'_, H>
where
    H: RequestFilter,
    for<'a> &'a H: Send,
{
    fn extensions(&self) -> &Extensions {
        self.extensions
    }

    fn extensions_mut(&mut self) -> &mut Extensions {
        self.extensions
    }

    async fn write_response_header(
        &mut self,
        mut resp: Box<ResponseHeader>,
        end_of_stream: bool,
    ) -> Result<(), Box<Error>> {
        self.handler.response_filter(self, &mut resp, None);

        self.deref_mut().write_response_header(resp, end_of_stream).await
    }

    async fn write_response_header_ref(&mut self, resp: &ResponseHeader) -> Result<(), Box<Error>> {
        // TODO: End of stream
        self.write_response_header(Box::new(resp.clone()), false).await
    }
}

impl<H> Deref for SessionWrapperImpl<'_, H> {
    type Target = Session;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<H> DerefMut for SessionWrapperImpl<'_, H> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}
