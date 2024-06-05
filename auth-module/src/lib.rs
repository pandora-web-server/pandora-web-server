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

//! # Auth Module for Pingora
//!
//! This crate allows putting up an authentication check before further processing of the request
//! happens. Only authorized users can proceed, others get a “401 Unauthorized” response. This
//! barrier can apply to the entire server or, with the help of the Virtual Hosts Module, only a
//! single virtual host/subdirectory.
//!
//! A configuration could look like this:
//!
//! ```yaml
//! auth_realm: Protected area
//! auth_credentials:
//!     me: $2y$12$iuKHb5UsRqktrX2X9.iSEOP1n1.tS7s/KB.Dq3HlE0E6CxlfsJyZK
//!     you: $2y$12$diY.HNTgfg0tIJKJxwmq.edEep5RcuAuQaAvXsP22oSPKY/dS1IVW
//! ```
//!
//! This sets up two users `me` and `you` with their respective password hashes, corresponding with
//! the passwords `test` and `test2`.
//!
//! ## Password hashes
//!
//! The supported password hashes use the [bcrypt algorithm](https://en.wikipedia.org/wiki/Bcrypt)
//! and should start with either `$2b$` or `$2y$`. While `$2a$` and `$2x$` hashes can be handled as
//! well, these should be considered insecure due to implementation bugs.
//!
//! A hash can be generated using the `htpasswd` tool distributed along with the Apache web server:
//!
//! ```sh
//! htpasswd -nBC 12 user
//! ```
//!
//! Alternatively, you can use this module to generate a password hash for you:
//!
//! 1. To activate the module, make sure the `auth_credentials` setting isn’t empty. It doesn’t
//! have to contain a valid set of credentials, any value will do.
//! 2. Add the `auth_display_hash: true` setting to your configuration.
//! 3. Run the server and navigate to the password protected area with your browser.
//! 4. When prompted by the browser, enter the credentials you want to use.
//! 5. When prompted for credentials again, close the prompt to see the “401 Unauthorized” page.
//!
//! The page will contain the credentials you should add to your configuration. You can remove the
//! `auth_display_hash: true` setting now.
//!
//! ## Code example
//!
//! You would normally put this handler in front of other handlers, such as the Static Files
//! Module. You would use macros to merge the configuration and the command-line options of the
//! handlers and Pingora:
//!
//! ```rust
//! use auth_module::{AuthHandler, AuthOpt};
//! use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
//! use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
//! use pingora_core::server::Server;
//! use static_files_module::{StaticFilesHandler, StaticFilesOpt};
//! use structopt::StructOpt;
//!
//! #[derive(Debug, RequestFilter)]
//! struct Handler {
//!     auth: AuthHandler,
//!     static_files: StaticFilesHandler,
//! }
//!
//! #[merge_conf]
//! struct Conf {
//!     server: ServerConf,
//!     handler: <Handler as RequestFilter>::Conf,
//! }
//!
//! #[merge_opt]
//! struct Opt {
//!     server: ServerOpt,
//!     auth: AuthOpt,
//! }
//!
//! let opt = Opt::from_args();
//! let conf = opt
//!     .server
//!     .conf
//!     .as_ref()
//!     .and_then(|path| Some(Conf::load_from_yaml(path).unwrap()))
//!     .unwrap_or_default();
//!
//! let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
//! server.bootstrap();
//!
//! let handler = Handler::new(conf.handler);
//! ```
//!
//! You can then use that handler in your server implementation:
//!
//! ```rust
//! use async_trait::async_trait;
//! use module_utils::RequestFilter;
//! use pingora_core::Error;
//! use pingora_core::upstreams::peer::HttpPeer;
//! use pingora_http::ResponseHeader;
//! use pingora_proxy::{ProxyHttp, Session};
//!
//! # use auth_module::AuthHandler;
//! # #[derive(Debug, RequestFilter)]
//! # struct Handler {
//! #     auth: AuthHandler,
//! # }
//! struct MyServer {
//!     handler: Handler,
//! }
//!
//! #[async_trait]
//! impl ProxyHttp for MyServer {
//!     type CTX = <Handler as RequestFilter>::CTX;
//!     fn new_ctx(&self) -> Self::CTX {
//!         Handler::new_ctx()
//!     }
//!
//!     async fn request_filter(
//!         &self,
//!         session: &mut Session,
//!         ctx: &mut Self::CTX,
//!     ) -> Result<bool, Box<Error>> {
//!         self.handler.handle(session, ctx).await
//!     }
//!
//!     async fn upstream_peer(
//!         &self,
//!         session: &mut Session,
//!         ctx: &mut Self::CTX,
//!     ) -> Result<Box<HttpPeer>, Box<Error>> {
//!         panic!("Unexpected, upstream_peer stage reached");
//!     }
//! }
//! ```
//!
//! For complete code see `single-static-root` and `virtual-hosts` examples in the repository.

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bcrypt::{hash, verify, DEFAULT_COST};
use http::{header, Method, StatusCode};
use log::{error, info, trace};
use maud::{html, DOCTYPE};
use module_utils::pingora::{Error, ResponseHeader, SessionWrapper};
use module_utils::{DeserializeMap, RequestFilter, RequestFilterResult};
use std::collections::HashMap;
use structopt::StructOpt;

/// Command line options of the auth module
#[derive(Debug, StructOpt)]
pub struct AuthOpt {
    /// Use to display a configuration suggestion for your failed login on the 401 Unauthorized
    /// page.
    ///
    /// This allows you to produce a hash for your password without using any third-party tools.
    #[structopt(long)]
    pub auth_display_hash: bool,
    /// The authentication realm to communicate to the browser
    #[structopt(long)]
    pub auth_realm: Option<String>,
    /// Authorization credentials using the format user:hash. This command line flag can be
    /// specified multiple times.
    ///
    /// Supported hashes use the bcrypt format and start with $2b$ or $2y$. Use --auth-display-hash
    /// command line flag to generate a password hash without third-party tools.
    #[structopt(long)]
    pub auth_credentials: Option<Vec<String>>,
}

/// Authentication configuration
#[derive(Debug, DeserializeMap)]
pub struct AuthConf {
    /// If `true`, the credentials of failed login attempts will be displayed on the resulting
    /// 401 Unauthorized page.
    pub auth_display_hash: bool,
    /// Realm for the authentication challenge
    pub auth_realm: String,
    /// Accepted credentials by user name
    pub auth_credentials: HashMap<String, String>,
}

impl AuthConf {
    /// Merges the command line options into the current configuration. Command line options
    /// present overwrite existing settings, with the exception of `--auth-credentials` that adds
    /// to the existing ones.
    pub fn merge_with_opt(&mut self, opt: AuthOpt) {
        if opt.auth_display_hash {
            self.auth_display_hash = true;
        }

        if let Some(auth_realm) = opt.auth_realm {
            self.auth_realm = auth_realm;
        }

        if let Some(auth_credentials) = opt.auth_credentials {
            for entry in auth_credentials {
                if let Some((user, hash)) = entry.split_once(':') {
                    self.auth_credentials
                        .insert(user.to_owned(), hash.to_owned());
                } else {
                    error!("Invalid credentials, missing separator between user name and hash: {entry}");
                }
            }
        }
    }
}

impl Default for AuthConf {
    fn default() -> Self {
        Self {
            auth_display_hash: false,
            auth_realm: "Server authentication".to_owned(),
            auth_credentials: HashMap::new(),
        }
    }
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug)]
pub struct AuthHandler {
    conf: AuthConf,
}

impl TryFrom<AuthConf> for AuthHandler {
    type Error = Box<Error>;

    fn try_from(conf: AuthConf) -> Result<Self, Self::Error> {
        Ok(Self { conf })
    }
}

async fn error_response(
    session: &mut impl SessionWrapper,
    realm: &str,
    credentials: Option<(&str, &[u8])>,
) -> Result<(), Box<Error>> {
    let text = html! {
        (DOCTYPE)
        html {
            head {
                title {
                    "401 Unauthorized"
                }
            }

            body {
                center {
                    h1 {
                        "401 Unauthorized"
                    }
                }

                @if let Some((user, password)) = credentials.and_then(|(u, p)| Some((u, hash(p, DEFAULT_COST).ok()?))) {
                    p {
                        "If you are the administrator of this server, you might want to add the following to your configuration:"
                    }
                    pre {
                        "auth_credentials:\n"
                        "    \"" (user) "\": " (password)
                    }
                }
            }
        }
    }.into_string();

    let mut header = ResponseHeader::build(StatusCode::UNAUTHORIZED, Some(3))?;
    header.append_header(header::CONTENT_LENGTH, text.len().to_string())?;
    header.append_header(header::CONTENT_TYPE, "text/html")?;
    header.append_header(header::WWW_AUTHENTICATE, format!("Basic realm=\"{realm}\""))?;
    // TODO header.append_header(header::WWW_AUTHENTICATE, )?;
    session.write_response_header(Box::new(header)).await?;

    if session.req_header().method != Method::HEAD {
        session.write_response_body(text.into()).await?;
    }

    Ok(())
}

#[async_trait]
impl RequestFilter for AuthHandler {
    type Conf = AuthConf;

    type CTX = ();

    fn new_ctx() -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        if self.conf.auth_credentials.is_empty() {
            return Ok(RequestFilterResult::Unhandled);
        }

        let auth = match session.req_header().headers.get(header::AUTHORIZATION) {
            Some(auth) => auth,
            None => {
                trace!("Rejecting request, no Authorization header");
                error_response(session, &self.conf.auth_realm, None).await?;
                return Ok(RequestFilterResult::ResponseSent);
            }
        };

        let auth = match auth.to_str() {
            Ok(auth) => auth,
            Err(err) => {
                info!(
                    "Rejecting request, Authorization header cannot be converted to string: {err}"
                );
                error_response(session, &self.conf.auth_realm, None).await?;
                return Ok(RequestFilterResult::ResponseSent);
            }
        };

        let (scheme, credentials) = auth.split_once(' ').unwrap_or(("", ""));
        if scheme != "Basic" {
            info!("Rejecting request, unsupported authorization scheme: {scheme}");
            error_response(session, &self.conf.auth_realm, None).await?;
            return Ok(RequestFilterResult::ResponseSent);
        }

        let credentials = match BASE64_STANDARD.decode(credentials) {
            Ok(credentials) => credentials,
            Err(err) => {
                info!("Rejecting request, failed decoding base64: {err}");
                error_response(session, &self.conf.auth_realm, None).await?;
                return Ok(RequestFilterResult::ResponseSent);
            }
        };

        let (user, password, display_hash) =
            if let Some(index) = credentials.iter().position(|b| *b == b':') {
                (
                    String::from_utf8(credentials[0..index].to_vec()).unwrap_or_default(),
                    &credentials[index + 1..],
                    self.conf.auth_display_hash,
                )
            } else {
                ("".to_owned(), "".as_bytes(), false)
            };
        let credentials = if display_hash {
            Some((user.as_str(), password))
        } else {
            None
        };

        let result = if let Some(expected) = self.conf.auth_credentials.get(&user) {
            verify(password, expected)
        } else {
            // This user name is unknown. We still go through verification to prevent timing
            // attacks. But we test an empty password against bcrypt-hashed string "test", this is
            // guaranteed to fail.
            verify(
                b"",
                "$2y$12$/GSb/xs3Ss/Jq0zv5qBZWeH3oz8RzEi.PuOhPJ8qiP6yCc2dtDbnK",
            )
        };

        let valid = match result {
            Ok(valid) => valid,
            Err(err) => {
                info!("Rejecting request, bcrypt failure: {err}");
                error_response(session, &self.conf.auth_realm, credentials).await?;
                return Ok(RequestFilterResult::ResponseSent);
            }
        };

        if !valid {
            info!("Rejecting request, wrong password");
            error_response(session, &self.conf.auth_realm, credentials).await?;
            return Ok(RequestFilterResult::ResponseSent);
        }

        Ok(RequestFilterResult::Unhandled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use module_utils::pingora::{RequestHeader, TestSession};
    use module_utils::standard_response::response_text;
    use module_utils::FromYaml;
    use test_log::test;

    fn default_conf() -> &'static str {
        r#"
            auth_realm: "Protected area"
            auth_credentials:
                # test
                me: $2y$04$V15kxj8/a7JsIb6lXkcK7ex.IiNSM3.nbLJaLbkAi10iVXUip/JoC
                # test2
                another: $2y$04$s/KAIlzQM8VfPsf9.YKAGOfZhMp44lcXHLB9avFGnON3D1QKG9clS
        "#
    }

    fn make_handler(conf: &str) -> AuthHandler {
        <AuthHandler as RequestFilter>::Conf::from_yaml(conf)
            .unwrap()
            .try_into()
            .unwrap()
    }

    async fn make_session() -> TestSession {
        let header = RequestHeader::build("GET", b"/", None).unwrap();
        TestSession::from(header).await
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
    async fn request_filter() -> Result<(), Box<Error>> {
        let error_response = response_text(StatusCode::UNAUTHORIZED);

        // Unconfigured, should allow request through
        let handler = make_handler("auth_realm: unconfigured");
        let mut session = make_session().await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );

        // No Authorization header
        let handler = make_handler(default_conf());
        let mut session = make_session().await;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.response_written().unwrap().status, 401);
        assert_headers(
            session.response_written().unwrap(),
            vec![
                ("Content-Type", "text/html"),
                ("Content-Length", &error_response.len().to_string()),
                ("WWW-Authenticate", "Basic realm=\"Protected area\""),
            ],
        );
        assert_eq!(
            String::from_utf8_lossy(&session.response_body),
            error_response
        );

        // Unknown auth scheme
        let handler = make_handler(default_conf());
        let mut session = make_session().await;
        session
            .req_header_mut()
            .insert_header("Authorization", "Unknown bWU6dGVzdA==")?;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.response_written().unwrap().status, 401);
        assert_headers(
            session.response_written().unwrap(),
            vec![
                ("Content-Type", "text/html"),
                ("Content-Length", &error_response.len().to_string()),
                ("WWW-Authenticate", "Basic realm=\"Protected area\""),
            ],
        );
        assert_eq!(
            String::from_utf8_lossy(&session.response_body),
            error_response
        );

        // Missing credentials
        let handler = make_handler(default_conf());
        let mut session = make_session().await;
        session
            .req_header_mut()
            .insert_header("Authorization", "Basic")?;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.response_written().unwrap().status, 401);
        assert_headers(
            session.response_written().unwrap(),
            vec![
                ("Content-Type", "text/html"),
                ("Content-Length", &error_response.len().to_string()),
                ("WWW-Authenticate", "Basic realm=\"Protected area\""),
            ],
        );
        assert_eq!(
            String::from_utf8_lossy(&session.response_body),
            error_response
        );

        // Credentials without colon
        let handler = make_handler(default_conf());
        let mut session = make_session().await;
        session
            .req_header_mut()
            .insert_header("Authorization", "Basic bWV0ZXN0")?;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.response_written().unwrap().status, 401);
        assert_headers(
            session.response_written().unwrap(),
            vec![
                ("Content-Type", "text/html"),
                ("Content-Length", &error_response.len().to_string()),
                ("WWW-Authenticate", "Basic realm=\"Protected area\""),
            ],
        );
        assert_eq!(
            String::from_utf8_lossy(&session.response_body),
            error_response
        );

        // Wrong credentials
        let handler = make_handler(default_conf());
        let mut session = make_session().await;
        session
            .req_header_mut()
            .insert_header("Authorization", "Basic bWU6dGVzdDI=")?;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.response_written().unwrap().status, 401);
        assert_headers(
            session.response_written().unwrap(),
            vec![
                ("Content-Type", "text/html"),
                ("Content-Length", &error_response.len().to_string()),
                ("WWW-Authenticate", "Basic realm=\"Protected area\""),
            ],
        );
        assert_eq!(
            String::from_utf8_lossy(&session.response_body),
            error_response
        );

        // Wrong user name
        let handler = make_handler(default_conf());
        let mut session = make_session().await;
        session
            .req_header_mut()
            .insert_header("Authorization", "Basic eW91OnRlc3Q=")?;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert_eq!(session.response_written().unwrap().status, 401);
        assert_headers(
            session.response_written().unwrap(),
            vec![
                ("Content-Type", "text/html"),
                ("Content-Length", &error_response.len().to_string()),
                ("WWW-Authenticate", "Basic realm=\"Protected area\""),
            ],
        );
        assert_eq!(
            String::from_utf8_lossy(&session.response_body),
            error_response
        );

        // Correct credentials
        let handler = make_handler(default_conf());
        let mut session = make_session().await;
        session
            .req_header_mut()
            .insert_header("Authorization", "Basic bWU6dGVzdA==")?;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::Unhandled
        );

        // Display hash on wrong credentials
        let handler = make_handler(
            r#"
                auth_display_hash: true
                auth_credentials:
                    me: abc
            "#,
        );
        let mut session = make_session().await;
        session
            .req_header_mut()
            .insert_header("Authorization", "Basic JzxtZT4nOnRlc3Q=")?;
        assert_eq!(
            handler.request_filter(&mut session, &mut ()).await?,
            RequestFilterResult::ResponseSent
        );
        assert!(String::from_utf8_lossy(&session.response_body)
            .contains("&quot;'&lt;me&gt;'&quot;: $2b$"));

        Ok(())
    }
}
