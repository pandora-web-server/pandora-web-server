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

//! Exposes some types from `pingora-core` and `pingora-proxy` crates, so that typical modules no
//! longer need them as direct dependencies.

use async_trait::async_trait;
use http::{header, Extensions};
pub use pingora_core::protocols::http::HttpTask;
pub use pingora_core::upstreams::peer::HttpPeer;
pub use pingora_core::{Error, ErrorType};
pub use pingora_http::{IntoCaseHeaderName, ResponseHeader};
pub use pingora_proxy::Session;
use std::borrow::Cow;
use std::ops::{Deref, DerefMut};
use tokio_test::io::Mock;

use crate::RequestFilter;

/// A trait implemented by wrappers around Pingora’s session
///
/// All the usual methods and fields of [`Session`] are available as well.
#[async_trait]
pub trait SessionWrapper: Send + Deref<Target = Session> + DerefMut {
    /// Attempts to determine the request host if one was specified.
    fn host(&self) -> Option<Cow<'_, str>>
    where
        Self: Sized,
    {
        fn host_from_header(session: &impl SessionWrapper) -> Option<Cow<'_, str>> {
            let host = session.get_header(header::HOST)?;
            host.to_str().ok().map(|h| h.into())
        }

        fn host_from_uri(session: &impl SessionWrapper) -> Option<Cow<'_, str>> {
            let uri = &session.req_header().uri;
            let host = uri.host()?;
            if let Some(port) = uri.port() {
                let mut host = host.to_owned();
                host.push(':');
                host.push_str(port.as_str());
                Some(host.into())
            } else {
                Some(host.into())
            }
        }

        host_from_header(self).or_else(|| host_from_uri(self))
    }

    /// Returns a reference to the associated extensions.
    ///
    /// *Note*: The extensions are only present for the lifetime of the wrapper. Unlike `Session`
    /// or `CTX` data, they don’t survive across Pingora phases.
    fn extensions(&self) -> &Extensions;

    /// Returns a mutable reference to the associated extensions.
    ///
    /// *Note*: The extensions are only present for the lifetime of the wrapper. Unlike `Session`
    /// or `CTX` data, they don’t survive across Pingora phases.
    fn extensions_mut(&mut self) -> &mut Extensions;

    /// Modifies `Session::write_response_header` to ensure that additional response headers are
    /// written.
    async fn write_response_header(&mut self, resp: Box<ResponseHeader>) -> Result<(), Box<Error>>;

    /// Modifies `Session::write_response_header_ref` to ensure that additional response headers
    /// are written.
    async fn write_response_header_ref(&mut self, resp: &ResponseHeader) -> Result<(), Box<Error>> {
        self.write_response_header(Box::new(resp.clone())).await
    }
}

struct SessionWrapperImpl<'a, H> {
    inner: &'a mut Session,
    handler: &'a H,
    extensions: Extensions,
}

impl<'a, H> SessionWrapperImpl<'a, H> {
    fn from(inner: &'a mut Session, handler: &'a H) -> Self
    where
        H: RequestFilter,
    {
        Self {
            inner,
            handler,
            extensions: Extensions::new(),
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
        &self.extensions
    }

    fn extensions_mut(&mut self) -> &mut Extensions {
        &mut self.extensions
    }

    async fn write_response_header(
        &mut self,
        mut resp: Box<ResponseHeader>,
    ) -> Result<(), Box<Error>> {
        self.handler.response_filter(self, &mut resp, None);

        self.deref_mut().write_response_header(resp).await
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

/// Creates a new session wrapper for the given Pingora session.
pub(crate) fn wrap_session<'a, H>(
    session: &'a mut Session,
    handler: &'a H,
) -> impl SessionWrapper + 'a
where
    H: RequestFilter + Sized + Sync,
{
    SessionWrapperImpl::from(session, handler)
}

/// A `SessionWrapper` implementation used for tests.
pub struct TestSession {
    inner: Session,
    extensions: Extensions,
}

impl TestSession {
    /// Creates a new test session based on a mock of the network communication.
    pub async fn from(mock: Mock) -> Self {
        let mut inner = Session::new_h1(Box::new(mock));
        assert!(inner.read_request().await.unwrap());
        Self {
            inner,
            extensions: Extensions::new(),
        }
    }
}

#[async_trait]
impl SessionWrapper for TestSession {
    fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    fn extensions_mut(&mut self) -> &mut Extensions {
        &mut self.extensions
    }

    async fn write_response_header(&mut self, resp: Box<ResponseHeader>) -> Result<(), Box<Error>> {
        self.deref_mut().write_response_header(resp).await
    }
}

impl Deref for TestSession {
    type Target = Session;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for TestSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl std::fmt::Debug for TestSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestSession").finish()
    }
}
