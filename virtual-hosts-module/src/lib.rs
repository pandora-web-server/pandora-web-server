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

//! # Virtual Hosts Module for Pingora
//!
//! This module simplifies dealing with virtual hosts. It wraps any handler implementing
//! [`module_utils::RequestFilter`] and its configuration, allowing to supply a different
//! configuration for that handler for each virtual host. For example, if Static Files Module is
//! the wrapped handler, the configuration file might look like this:
//!
//! ```yaml
//! vhosts:
//!     localhost:8000:
//!         aliases:
//!             - 127.0.0.1:8000
//!             - "[::1]:8000"
//!         root: ./local-debug-root
//!     example.com:
//!         aliases:
//!             - www.example.com
//!         default: true
//!         root: ./production-root
//! ```
//!
//! This module adds two configuration settings to the configuration of the wrapped handler:
//!
//! * `aliases` lists additional host names that should share the same configuration.
//! * `default` can be set to `true` to indicate that this configuration should apply to all host
//!   names not listed explicitly.
//!
//! If no default host entry is present and a request is made for an unknown host name, this
//! handler will leave the request unhandled. Otherwise the handling is delegated to the wrapped
//! handler.
//!
//! ## Code example
//!
//! Usually, the virtual hosts configuration will be read from a configuration file and used to
//! instantiate the corresponding handler. This is how it would be done:
//!
//! ```rust
//! use pingora_core::server::configuration::{Opt, ServerConf};
//! use module_utils::{FromYaml, merge_conf};
//! use static_files_module::{StaticFilesConf, StaticFilesHandler};
//! use structopt::StructOpt;
//! use virtual_hosts_module::{VirtualHostsConf, VirtualHostsHandler};
//!
//! // Combine Pingora server configuration with virtual hosts wrapping static files configuration.
//! #[merge_conf]
//! struct Conf {
//!     server: ServerConf,
//!     virtual_hosts: VirtualHostsConf<StaticFilesConf>,
//! }
//!
//! // Read command line options and configuration file.
//! let opt = Opt::from_args();
//! let conf = opt
//!     .conf
//!     .as_ref()
//!     .and_then(|path| Conf::load_from_yaml(path).ok())
//!     .unwrap_or_else(Conf::default);
//!
//! // Create handler from configuration
//! let handler: VirtualHostsHandler<StaticFilesHandler> = conf.virtual_hosts.try_into().unwrap();
//! ```
//!
//! You can then use that handler in your server implementation:
//!
//! ```rust
//! use async_trait::async_trait;
//! use pingora_core::upstreams::peer::HttpPeer;
//! use pingora_core::Error;
//! use pingora_proxy::{ProxyHttp, Session};
//! use module_utils::RequestFilter;
//! use static_files_module::StaticFilesHandler;
//! use virtual_hosts_module::VirtualHostsHandler;
//!
//! pub struct MyServer {
//!     handler: VirtualHostsHandler<StaticFilesHandler>,
//! }
//!
//! #[async_trait]
//! impl ProxyHttp for MyServer {
//!     type CTX = <VirtualHostsHandler<StaticFilesHandler> as RequestFilter>::CTX;
//!     fn new_ctx(&self) -> Self::CTX {
//!         VirtualHostsHandler::<StaticFilesHandler>::new_ctx()
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
//!         _session: &mut Session,
//!         _ctx: &mut Self::CTX,
//!     ) -> Result<Box<HttpPeer>, Box<Error>> {
//!         // Virtual hosts handler didn't handle the request, meaning no matching virtual host in
//!         // configuration. Delegate to upstream peer.
//!         Ok(Box::new(HttpPeer::new(
//!             "example.com:443",
//!             true,
//!             "example.com".to_owned(),
//!         )))
//!     }
//! }
//! ```
//!
//! For complete and more comprehensive code see `virtual-hosts` example in the repository.

mod configuration;
mod handler;

pub use configuration::{HostConfig, VirtualHostConf, VirtualHostsConf};
pub use handler::VirtualHostsHandler;
