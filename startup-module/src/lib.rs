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
use bytes::{Bytes, BytesMut};
pub use configuration::{
    CertKeyConf, ListenAddr, StartupConf, StartupOpt, TlsConf, TlsRedirectorConf,
};
use http::Extensions;
use pandora_module_utils::pingora::{
    Error, HttpPeer, ProxyHttp, ResponseHeader, Session, SessionWrapper,
};
use pandora_module_utils::{RequestFilter, RequestFilterResult};
use pingora::modules::http::HttpModules;
use pingora::ErrorType;
use std::borrow::Cow;
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

#[derive(Debug, RequestFilter)]
struct DummyHandler {}

struct NoDebug<T> {
    inner: T,
}

impl<T> Debug for NoDebug<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("skipped").finish()
    }
}

impl<T> From<T> for NoDebug<T> {
    fn from(value: T) -> Self {
        Self { inner: value }
    }
}

impl<T> Deref for NoDebug<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for NoDebug<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Result of a test execution of the app
#[derive(Debug)]
pub struct AppResult {
    session: NoDebug<Session>,
    err: Option<Box<Error>>,
    extensions: Extensions,
    body: BytesMut,
    handler: DummyHandler,
}

impl AppResult {
    fn new(
        session: Session,
        err: Option<Box<Error>>,
        extensions: Extensions,
        body: BytesMut,
    ) -> Self {
        Self {
            session: session.into(),
            err,
            extensions,
            body,
            handler: DummyHandler {},
        }
    }

    /// Produces the resulting session state of the request
    pub fn session(&mut self) -> impl SessionWrapper + '_ {
        SessionWrapperImpl::new(
            &mut self.session,
            &self.handler,
            &mut self.extensions,
            false,
        )
    }

    /// Retrieves the error if any
    pub fn err(&self) -> &Option<Box<Error>> {
        &self.err
    }

    /// Retrieves the response body
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Retrieves the response body as string
    pub fn body_str(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }
}

/// A basic Pingora app implementation, to be passed to [`StartupConf::into_server`]
///
/// This app will only handle the `request_filter`, `upstream_peer`, `upstream_response_filter` and
/// `logging` phases. All processing will be delegated to the respective `RequestFilter` methods.
#[derive(Debug)]
pub struct DefaultApp<H> {
    handler: H,
    capture_body: bool,
}

impl<H> DefaultApp<H> {
    /// Creates a new app from a [`RequestFilter`] instance.
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            capture_body: false,
        }
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

    /// Handles all request phases for a request like Pingora would do it.
    ///
    /// This method is meant for testing. Will error out if an upstream peer needs to be contacted.
    /// Upon successful completion, `evaluate_result` callback is called to validate the session.
    pub async fn handle_request(&mut self, session: Session) -> AppResult
    where
        H: RequestFilter + Sync,
        H::CTX: Send + Sync,
    {
        self.handle_request_with_upstream(session, |_, _| {
            Err(Error::explain(
                ErrorType::InternalError,
                "Got upstream peer but no handler for it",
            ))
        })
        .await
    }

    /// Handles all request phases for a request like Pingora would do it while also faking
    /// upstream response.
    ///
    /// This method is meant for testing. Will call `upstream_response` callback to produce a fake
    /// upstream response if necessary. Upon successful completion, `evaluate_result` callback is
    /// called to validate the session.
    pub async fn handle_request_with_upstream<C>(
        &mut self,
        mut session: Session,
        upstream_response: C,
    ) -> AppResult
    where
        C: Fn(&mut Session, Box<HttpPeer>) -> Result<ResponseHeader, Box<Error>>,
        H: RequestFilter + Sync,
        H::CTX: Send + Sync,
    {
        let mut modules = HttpModules::new();
        self.init_downstream_modules(&mut modules);
        session.downstream_modules_ctx = modules.build_ctx();

        self.capture_body = true;

        let mut ctx = self.new_ctx();

        let result = async {
            self.early_request_filter(&mut session, &mut ctx).await?;

            let request = session.downstream_session.req_header_mut();
            session
                .downstream_modules_ctx
                .request_header_filter(request)
                .await?;

            match self.request_filter(&mut session, &mut ctx).await {
                Ok(false) => {
                    let upstream_peer = self.upstream_peer(&mut session, &mut ctx).await?;
                    let mut response_header = upstream_response(&mut session, upstream_peer)?;
                    self.upstream_response_filter(&mut session, &mut response_header, &mut ctx);
                    session
                        .downstream_modules_ctx
                        .response_header_filter(&mut response_header, false)
                        .await?;
                    session
                        .write_response_header(Box::new(response_header), false)
                        .await?;

                    let mut body = ctx.extensions.remove::<BytesMut>().map(|body| body.into());
                    session
                        .downstream_modules_ctx
                        .response_body_filter(&mut body, true)
                }
                Ok(true) => Ok(()),
                Err(err) => Err(err),
            }
        }
        .await;

        self.logging(
            &mut session,
            result.as_ref().err().map(|err| err.as_ref()),
            &mut ctx,
        )
        .await;

        self.capture_body = false;

        let body = ctx.extensions.remove::<BytesMut>().unwrap_or_default();

        AppResult::new(session, result.err(), ctx.extensions, body)
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

    async fn early_request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<(), Box<Error>> {
        let mut session = SessionWrapperImpl::new(
            session,
            &self.handler,
            &mut ctx.extensions,
            self.capture_body,
        );
        self.handler
            .early_request_filter(&mut session, &mut ctx.handler)
            .await
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        let mut session = SessionWrapperImpl::new(
            session,
            &self.handler,
            &mut ctx.extensions,
            self.capture_body,
        );
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
        let mut session = SessionWrapperImpl::new(
            session,
            &self.handler,
            &mut ctx.extensions,
            self.capture_body,
        );
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
        let mut session = SessionWrapperImpl::new(
            session,
            &self.handler,
            &mut ctx.extensions,
            self.capture_body,
        );
        self.handler
            .response_filter(&mut session, response, Some(&mut ctx.handler))
    }

    async fn logging(&self, session: &mut Session, e: Option<&Error>, ctx: &mut Self::CTX) {
        let mut session = SessionWrapperImpl::new(
            session,
            &self.handler,
            &mut ctx.extensions,
            self.capture_body,
        );
        self.handler
            .logging(&mut session, e, &mut ctx.handler)
            .await
    }
}

struct SessionWrapperImpl<'a, H> {
    inner: &'a mut Session,
    handler: &'a H,
    extensions: &'a mut Extensions,
    capture_body: bool,
}

impl<'a, H> SessionWrapperImpl<'a, H> {
    /// Creates a new session wrapper for the given Pingora session.
    fn new(
        inner: &'a mut Session,
        handler: &'a H,
        extensions: &'a mut Extensions,
        capture_body: bool,
    ) -> Self
    where
        H: RequestFilter,
    {
        Self {
            inner,
            handler,
            extensions,
            capture_body,
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

        self.deref_mut()
            .write_response_header(resp, end_of_stream)
            .await
    }

    async fn write_response_header_ref(
        &mut self,
        resp: &ResponseHeader,
        end_of_stream: bool,
    ) -> Result<(), Box<Error>> {
        self.write_response_header(Box::new(resp.clone()), end_of_stream)
            .await
    }

    async fn write_response_body(
        &mut self,
        data: Option<Bytes>,
        end_of_stream: bool,
    ) -> Result<(), Box<Error>> {
        if self.capture_body {
            if let Some(data) = data {
                self.extensions_mut()
                    .get_or_insert_default::<BytesMut>()
                    .extend_from_slice(&data);
            }
            Ok(())
        } else {
            self.deref_mut()
                .write_response_body(data, end_of_stream)
                .await
        }
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
