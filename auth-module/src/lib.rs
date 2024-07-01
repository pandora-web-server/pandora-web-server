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

//! # Auth Module for Pandora Web Server
//!
//! This crate allows putting up an authentication check before further processing of the request
//! happens. Only authorized users will be able to access the content. This barrier can apply to
//! the entire web server or, with the help of the Virtual Hosts Module, only a single virtual
//! host/subdirectory.
//!
//! A configuration could look like this:
//!
//! ```yaml
//! auth_mode: page
//! auth_credentials:
//!     me: $2y$12$iuKHb5UsRqktrX2X9.iSEOP1n1.tS7s/KB.Dq3HlE0E6CxlfsJyZK
//!     you: $2y$12$diY.HNTgfg0tIJKJxwmq.edEep5RcuAuQaAvXsP22oSPKY/dS1IVW
//! ```
//!
//! This sets up two users `me` and `you` with their respective password hashes, corresponding with
//! the passwords `test` and `test2`.
//!
//! ## Common settings
//!
//! The following settings always apply:
//!
//! * `auth_credentials`: A map containing user names and respective password hashes (see “Password
//!   hashes” section below). The module activates only if this setting has some entries.
//! * `auth_mode`: Either `http` or `page`. The former uses
//!   [Basic access authentication](https://en.wikipedia.org/wiki/Basic_access_authentication), the
//!   latter (default) displays a web page to handle logging in.
//! * `auth_display_hash`: If `true`, unsuccessful login attempts will display the password hash
//!   for the entered password (see “Password hashes” section below).
//! * `auth_rate_limits`: Login rate limits to prevent denial-of-service attacks against the server
//!   by triggering the (necessarily slow) login validation frequently. This can contain three
//!   entries `total`, `per_ip` and `per_user`, the default values are 16, 4 and 4 respectively.
//!   The value 0 disables the rate limiting category. Note that the default values might be too
//!   low for `http` mode where each server request is considered a login attempt.
//!
//! ## `http` mode settings
//!
//! In `http` mode the `auth_realm` setting also applies. It determines the “realm” parameter sent
//! to the browser in the authentication challenge. Modern browsers no longer display this
//! parameter to the user, but will automatically use the same credentials when encountering
//! website areas with identical “realm.”
//!
//! ## `page` mode settings
//!
//! In `page` mode several additional settings apply:
//!
//! * `auth_page_strings`: This map allows adjusting the text of the default login page. The
//!   strings `title` (page title), `heading` (heading above the login form), `error` (error
//!   message on invalid login), `username_label` (label of the user name field), `password_label`
//!   (label of the password field), `button_text` (text of the submit button) can be specified.
//! * `auth_page_session`: Various session-related parameters:
//!   * `login_page`: An optional path of the login page to use instead of the default page.
//!     *Note*: while the request to the login page will be allowed, requests to resources used by
//!     that page won’t be. These resources either have to be placed outside the area protected by
//!     the Authentication Module, or the page can use inline resource and `data:` URIs to avoid
//!     dependencies.
//!   * `token_secret`: Hex-encoded secret used to sign tokens issued on successful login. Normally
//!     you should generate 16 bytes (32 hex digits) randomly. If this setting is omitted, a secret
//!     will be randomly generated during server startup. While this is a viable option, a server
//!     restart will always invalidate all previously issued tokens, requiring users to log in
//!     again.
//!   * `cookie_name`: The cookie used to store the token issued upon successful login.
//!   * `secure_cookie`: If set, determines explicitly whether the `Secure` attribute should be
//!     used for the login cookie. Default behavior is to set this attribute for HTTPS sessions.
//!   * `session_expiration`: The time interval after which a login session will expire, requiring
//!     the user to log in again. This interval can be specified in hours (e.g. `2h`) or days (e.g.
//!     `7d`).
//!
//! ## Password hashes
//!
//! The supported password hashes use the [bcrypt algorithm](https://en.wikipedia.org/wiki/Bcrypt)
//! and should start with either `$2b$` or `$2y$`. While `$2a$` and `$2x$` hashes can be handled as
//! well, using these is discouraged due to implementation bugs.
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
//!    have to contain a valid set of credentials, any value will do.
//! 2. Add the `auth_display_hash: true` setting to your configuration.
//! 3. Run the server and navigate to the password protected area with your browser.
//! 4. When prompted by the browser, enter the credentials you want to use.
//! 5. When using `http` mode, close the prompt when the browser prompts you for credentials again.
//!    You should see the “401 Unauthorized” page then.
//!
//! The page will contain a configuration suggestion with the generated credentials. You can remove
//! the `auth_display_hash: true` setting now.
//!
//! ## Code example
//!
//! You would normally put this handler in front of other handlers, such as the Static Files
//! Module. The `pandora-module-utils` and `startup-module` crates provide helpers to simplify
//! merging of configuration and the command-line options of various handlers as well as creating
//! a server instance from the configuration:
//!
//! ```rust
//! use auth_module::{AuthHandler, AuthOpt};
//! use clap::Parser;
//! use pandora_module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
//! use startup_module::{DefaultApp, StartupConf, StartupOpt};
//! use static_files_module::{StaticFilesHandler, StaticFilesOpt};
//!
//! #[derive(Debug, RequestFilter)]
//! struct Handler {
//!     auth: AuthHandler,
//!     static_files: StaticFilesHandler,
//! }
//!
//! #[merge_conf]
//! struct Conf {
//!     startup: StartupConf,
//!     handler: <Handler as RequestFilter>::Conf,
//! }
//!
//! #[merge_opt]
//! struct Opt {
//!     startup: StartupOpt,
//!     auth: AuthOpt,
//!     static_files: StaticFilesOpt,
//! }
//!
//! let opt = Opt::parse();
//! let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
//! conf.handler.auth.merge_with_opt(opt.auth);
//! conf.handler.static_files.merge_with_opt(opt.static_files);
//!
//! let app = DefaultApp::<Handler>::from_conf(conf.handler).unwrap();
//! let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();
//!
//! // Do something with the server here, e.g. call server.run_forever()
//! ```

mod basic;
mod common;
mod page;

use async_trait::async_trait;
use clap::Parser;
use http::Uri;
use log::{error, info};
use pandora_module_utils::pingora::{Error, ErrorType, SessionWrapper};
use pandora_module_utils::{DeserializeMap, RequestFilter, RequestFilterResult};
use serde::{de::Unexpected, Deserialize, Deserializer};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use basic::basic_auth;
use page::page_auth;

/// Authentication mode
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// Basic HTTP authentication
    HTTP,
    /// Webpage-based authentication
    #[default]
    Page,
}

impl FromStr for AuthMode {
    type Err = Box<Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "http" => Ok(Self::HTTP),
            "page" => Ok(Self::Page),
            _ => Err(Error::explain(
                ErrorType::InternalError,
                "invalid auth mode value",
            )),
        }
    }
}

/// Command line options of the auth module
#[derive(Debug, Parser)]
pub struct AuthOpt {
    /// Use to display a configuration suggestion for your failed login on the 401 Unauthorized
    /// page.
    ///
    /// This allows you to produce a hash for your password without using any third-party tools.
    #[clap(long)]
    pub auth_display_hash: bool,
    /// Authorization credentials using the format user:hash. This command line flag can be
    /// specified multiple times.
    ///
    /// Supported hashes use the bcrypt format and start with $2b$ or $2y$. Use --auth-display-hash
    /// command line flag to generate a password hash without third-party tools.
    #[clap(long)]
    pub auth_credentials: Option<Vec<String>>,
    /// Authentication mode, either "http" or "page"
    #[clap(long)]
    pub auth_mode: Option<AuthMode>,
    /// The authentication realm to communicate to the browser (HTTP mode only)
    #[clap(long)]
    pub auth_realm: Option<String>,
}

/// Login rate limits
#[derive(Debug, Clone, PartialEq, Eq, DeserializeMap)]
pub struct AuthRateLimits {
    /// Total number of requests allowed per second
    ///
    /// The value 0 disables rate limiting here.
    total: isize,
    /// Number of requests allowed per second per IP address
    ///
    /// The value 0 disables rate limiting here.
    per_ip: isize,
    /// Number of requests allowed per second per user name
    ///
    /// The value 0 disables rate limiting here.
    per_user: isize,
}

impl Default for AuthRateLimits {
    fn default() -> Self {
        Self {
            total: 16,
            per_ip: 4,
            per_user: 4,
        }
    }
}

/// Texts used on the auth page
#[derive(Debug, Clone, PartialEq, Eq, DeserializeMap)]
pub struct AuthPageStrings {
    /// Title of the authentication page
    pub title: String,

    /// Heading text of the authentication page
    pub heading: String,

    /// Text of the "invalid credentials" error on the authentication page
    pub error: String,

    /// Label of the user name field on the authentication page
    pub username_label: String,

    /// Label of the password field on the authentication page
    pub password_label: String,

    /// Submit button text on the authentication page
    pub button_text: String,
}

impl Default for AuthPageStrings {
    fn default() -> Self {
        Self {
            title: "Access denied".to_owned(),
            heading: "Access is restricted, please log in.".to_owned(),
            error: "Invalid credentials, please try again.".to_owned(),
            username_label: "User name:".to_owned(),
            password_label: "Password:".to_owned(),
            button_text: "Log in".to_owned(),
        }
    }
}

fn deserialize_uri<'de, D>(deserializer: D) -> Result<Option<Uri>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let path = String::deserialize(deserializer)?;
    let uri = Uri::try_from(&path)
        .map_err(|_| D::Error::invalid_value(Unexpected::Str(&path), &"URI path"))?;
    Ok(Some(uri))
}

fn deserialize_hex<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let data = String::deserialize(deserializer)?;
    if data.len() % 2 != 0 {
        return Err(D::Error::invalid_value(
            Unexpected::Str(&data),
            &"hex-encoded string",
        ));
    }
    Ok(Some(
        (0..data.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&data[i..i + 2], 16))
            .collect::<Result<_, _>>()
            .map_err(|_| D::Error::invalid_value(Unexpected::Str(&data), &"hex-encoded string"))?,
    ))
}

fn deserialize_interval<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    let interval = String::deserialize(deserializer)?;
    let (interval, factor) = if let Some(interval) = interval.strip_suffix('h') {
        (interval, 60 * 60)
    } else if let Some(interval) = interval.strip_suffix('d') {
        (interval, 24 * 60 * 60)
    } else {
        (interval.as_str(), 24 * 60 * 60)
    };

    let interval = u64::from_str(interval)
        .map_err(|_| D::Error::invalid_value(Unexpected::Str(interval), &"number"))?;
    Ok(Duration::new(interval * factor, 0))
}

/// Session settings (page mode only)
#[derive(Debug, Clone, PartialEq, Eq, DeserializeMap)]
pub struct AuthPageSession {
    /// URI path of the page to be used for logging in instead of the default login page.
    #[pandora(deserialize_with = "deserialize_uri")]
    pub login_page: Option<Uri>,

    /// Hex-encoded token secret
    ///
    /// If missing, a random token secret will be generated at startup. A server restart will
    /// invalidate all active sessions then.
    #[pandora(deserialize_with = "deserialize_hex")]
    pub token_secret: Option<Vec<u8>>,

    /// Name of the cookie to store the JWT token
    pub cookie_name: String,

    /// Determines whether the `Secure` attribute should be set for the cookie, allowing it to be
    /// only sent via HTTPS protocol.
    ///
    /// By default, the attribute will be set if the server connection was an HTTPS connection.
    pub secure_cookie: Option<bool>,

    /// Authentication expiration interval
    ///
    /// In the configuration file this can be specified in days or in hours: `7d` (7 days), `2h`
    /// (2 hours).
    #[pandora(deserialize_with = "deserialize_interval")]
    pub session_expiration: Duration,
}

impl Default for AuthPageSession {
    fn default() -> Self {
        Self {
            login_page: None,
            token_secret: None,
            cookie_name: "token".to_owned(),
            secure_cookie: None,
            session_expiration: Duration::from_secs(7 * 24 * 60 * 60),
        }
    }
}

/// Authentication configuration
#[derive(Debug, Clone, PartialEq, Eq, DeserializeMap)]
pub struct AuthConf {
    /// If `true`, the credentials of failed login attempts will be displayed on the resulting
    /// 401 Unauthorized page.
    pub auth_display_hash: bool,

    /// Accepted credentials by user name
    pub auth_credentials: HashMap<String, String>,

    /// Login rate limits
    ///
    /// Note that in Basic HTTP mode each request is a “login”
    pub auth_rate_limits: AuthRateLimits,

    /// Authentication mode, either Basic HTTP authentication or web page
    pub auth_mode: AuthMode,

    /// Realm for the authentication challenge (Basic HTTP mode only)
    pub auth_realm: String,

    /// Texts used on the auth page
    pub auth_page_strings: AuthPageStrings,

    /// Session settings (page mode only)
    pub auth_page_session: AuthPageSession,
}

impl AuthConf {
    /// Merges the command line options into the current configuration. Command line options
    /// present overwrite existing settings, with the exception of `--auth-credentials` that adds
    /// to the existing ones.
    pub fn merge_with_opt(&mut self, opt: AuthOpt) {
        if opt.auth_display_hash {
            self.auth_display_hash = true;
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

        if let Some(auth_mode) = opt.auth_mode {
            self.auth_mode = auth_mode;
        }

        if let Some(auth_realm) = opt.auth_realm {
            self.auth_realm = auth_realm;
        }
    }
}

impl Default for AuthConf {
    fn default() -> Self {
        Self {
            auth_display_hash: false,
            auth_credentials: HashMap::new(),
            auth_rate_limits: Default::default(),
            auth_mode: AuthMode::Page,
            auth_realm: "Server authentication".to_owned(),
            auth_page_strings: Default::default(),
            auth_page_session: Default::default(),
        }
    }
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHandler {
    conf: AuthConf,
}

impl TryFrom<AuthConf> for AuthHandler {
    type Error = Box<Error>;

    fn try_from(mut conf: AuthConf) -> Result<Self, Self::Error> {
        if conf.auth_mode == AuthMode::Page && conf.auth_page_session.token_secret.is_none() {
            const TOKEN_LENGTH: usize = 16;
            let mut token = vec![0; TOKEN_LENGTH];
            if let Err(err) = getrandom::getrandom(&mut token) {
                return Err(Error::because(
                    ErrorType::InternalError,
                    "failed generating new random auth token",
                    err,
                ));
            }

            info!("No auth token in configuration, generated a random one. Server restart will invalidate existing sessions.");
            conf.auth_page_session.token_secret = Some(token);
        }

        Ok(Self { conf })
    }
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

        match self.conf.auth_mode {
            AuthMode::HTTP => basic_auth(&self.conf, session).await,
            AuthMode::Page => page_auth(&self.conf, session).await,
        }
    }
}
