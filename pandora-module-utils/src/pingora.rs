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
use bytes::Bytes;
use http::{header, Extensions, Uri};
pub use pingora::http::{IntoCaseHeaderName, RequestHeader, ResponseHeader};
pub use pingora::protocols::http::HttpTask;
pub use pingora::protocols::l4::socket::SocketAddr;
pub use pingora::proxy::{http_proxy_service, ProxyHttp, Session};
pub use pingora::server::configuration::{Opt as ServerOpt, ServerConf};
pub use pingora::server::Server;
pub use pingora::upstreams::peer::HttpPeer;
pub use pingora::{Error, ErrorType};
use std::borrow::Cow;
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::ops::{Deref, DerefMut};

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
            let uri = session.uri();
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

    /// Return the client (peer) address of the connection.
    ///
    /// Unlike the identical method of the Pingora session, this value can be overwritten.
    fn client_addr(&self) -> Option<&SocketAddr> {
        let addr = self.extensions().get();
        if addr.is_some() {
            addr
        } else {
            self.deref().client_addr()
        }
    }

    /// Overwrites the client address for this connection.
    fn set_client_addr(&mut self, addr: SocketAddr) {
        self.extensions_mut().insert(addr);
    }

    /// Returns a reference to the associated extensions.
    fn extensions(&self) -> &Extensions;

    /// Returns a mutable reference to the associated extensions.
    fn extensions_mut(&mut self) -> &mut Extensions;

    /// Returns the request URI.
    ///
    /// This might not be the original request URI but manipulated by Rewrite module for example.
    fn uri(&self) -> &Uri {
        &self.req_header().uri
    }

    /// Changes the request URI and saves the original URI.
    ///
    /// This method should be used instead of manipulating the request URI in the header.
    fn set_uri(&mut self, uri: Uri) {
        let current_uri = OriginalUri(self.uri().clone());
        self.extensions_mut().get_or_insert(current_uri);
        self.req_header_mut().set_uri(uri);
    }

    /// Returns the original URI of the request which might have been modified by e.g.
    /// by Rewrite module afterwards.
    fn original_uri(&self) -> &Uri {
        if let Some(OriginalUri(uri)) = self.extensions().get() {
            uri
        } else {
            self.uri()
        }
    }

    /// Returns the name of the authorized user if any
    fn remote_user(&self) -> Option<&str> {
        if let Some(RemoteUser(remote_user)) = self.extensions().get() {
            Some(remote_user)
        } else {
            None
        }
    }

    /// Sets the name of the authorized user
    fn set_remote_user(&mut self, remote_user: String) {
        self.extensions_mut().insert(RemoteUser(remote_user));
    }

    /// See [`Session::write_response_header`](pingora::protocols::http::server::Session::write_response_header)
    async fn write_response_header(&mut self, resp: Box<ResponseHeader>) -> Result<(), Box<Error>> {
        self.deref_mut().write_response_header(resp).await
    }

    /// See [`Session::write_response_header_ref`](pingora::protocols::http::server::Session::write_response_header_ref)
    async fn write_response_header_ref(&mut self, resp: &ResponseHeader) -> Result<(), Box<Error>> {
        self.deref_mut().write_response_header_ref(resp).await
    }

    /// See [`Session::response_written`](pingora::protocols::http::server::Session::response_written)
    fn response_written(&self) -> Option<&ResponseHeader> {
        self.deref().response_written()
    }

    /// See [`Session::write_response_body`](pingora::protocols::http::server::Session::write_response_body)
    async fn write_response_body(&mut self, data: Bytes) -> Result<(), Box<Error>> {
        self.deref_mut().write_response_body(data).await
    }
}

/// Type used to store remote user’s name in `SessionWrapper::extensions`
#[derive(Debug, Clone)]
struct RemoteUser(String);

/// Type used to store original request URI in `SessionWrapper::extensions`
#[derive(Debug, Clone)]
struct OriginalUri(Uri);

/// Creates a new Pingora session for tests with given request header
pub async fn create_test_session(header: RequestHeader) -> Session {
    create_test_session_with_body(header, "").await
}

/// Creates a new Pingora session for tests with given request header and request body
pub async fn create_test_session_with_body(
    mut header: RequestHeader,
    body: impl AsRef<[u8]>,
) -> Session {
    let mut cursor = Cursor::new(Vec::<u8>::new());
    let _ = cursor.write(b"POST / HTTP/1.1\r\n");
    let _ = cursor.write(b"Connection: close\r\n");
    let _ = cursor.write(b"\r\n");
    let _ = cursor.write(body.as_ref());
    let _ = cursor.seek(SeekFrom::Start(0));

    let _ = header.insert_header(header::CONTENT_LENGTH, body.as_ref().len());

    let mut session = Session::new_h1(Box::new(cursor));
    assert!(session.read_request().await.unwrap());
    *session.req_header_mut() = header;

    session
}
