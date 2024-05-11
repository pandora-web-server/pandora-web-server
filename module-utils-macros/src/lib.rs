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

//! # Macros for module-utils crate
//!
//! You normally shouldn’t use this crate directly but the `module-utils` crate instead.

mod derive_request_filter;
mod merge_conf;
mod merge_opt;

use proc_macro::TokenStream;

/// This attribute macro merges the command-line arguments from all structs identified as field of
/// the current struct. The result will implement `structopt::StructOpt` and `Debug` automatically.
/// All field types are required to implement `structopt::StructOpt` and `Debug`.
///
/// ```rust
/// use pingora_core::server::configuration::Opt as ServerOpt;
/// use module_utils::merge_opt;
/// use static_files_module::StaticFilesOpt;
/// use structopt::StructOpt;
///
/// #[derive(Debug, StructOpt)]
/// struct MyAppOpt {
///     /// IP address and port for the server to listen on
///     #[structopt(long, default_value = "127.0.0.1:8080")]
///     listen: String,
/// }
///
/// /// Starts my great application.
/// ///
/// /// Additional application description just to make a structopt bug work-around work.
/// #[merge_opt]
/// struct Opt {
///     app: MyAppOpt,
///     server: ServerOpt,
///     static_files: StaticFilesOpt,
/// }
///
/// let opt = Opt::from_args();
/// println!("Application options: {:?}", opt.app);
/// println!("Pingora server options: {:?}", opt.server);
/// println!("Static files options: {:?}", opt.static_files);
/// ```
#[proc_macro_attribute]
pub fn merge_opt(_args: TokenStream, input: TokenStream) -> TokenStream {
    merge_opt::merge_opt(input).unwrap_or_else(|err| err.into_compile_error().into())
}

/// This attribute macro merges the configuration settings from all structs identified as field of
/// the current struct. The result will implement [`serde::Deserialize`], [`Debug`] and [`Default`]
/// automatically. All field types are required to implement `serde::Deserialize`, `Debug` and
/// `Default`.
///
/// ```rust
/// use pingora_core::server::configuration::ServerConf;
/// use module_utils::{merge_conf, FromYaml};
/// use static_files_module::StaticFilesConf;
/// use serde::Deserialize;
///
/// #[derive(Debug, Default, Deserialize)]
/// struct MyAppConf {
///     /// IP address and port for the server to listen on
///     listen: String,
/// }
///
/// #[merge_conf]
/// struct Conf {
///     app: MyAppConf,
///     server: ServerConf,
///     static_files: StaticFilesConf,
/// }
///
/// let conf = Conf::load_from_yaml("test.yaml").ok().unwrap_or_else(Conf::default);
/// println!("Application settings: {:?}", conf.app);
/// println!("Pingora server settings: {:?}", conf.server);
/// println!("Static files settings: {:?}", conf.static_files);
/// ```
#[proc_macro_attribute]
pub fn merge_conf(_args: TokenStream, input: TokenStream) -> TokenStream {
    merge_conf::merge_conf(input).unwrap_or_else(|err| err.into_compile_error().into())
}

/// This macro will automatically implement [`RequestFilter`] by chaining the handlers identified
/// in the struct’s fields.
///
/// Each handler has to implement [`RequestFilter`] trait. The handlers will be called in the order
/// in which they are listed. Each handler can prevent the subsequent handlers from being called by
/// returning [`RequestFilterResult::ResponseSent`] or [`RequestFilterResult::Handled`].
///
/// The configuration and context for the struct will be implemented implicitly. These will have
/// the configuration/context of the respective handler in a field with the same name as the
/// handler in this struct.
///
/// ```rust,no_run
/// use module_utils::{FromYaml, RequestFilter};
/// use compression_module::CompressionHandler;
/// use static_files_module::StaticFilesHandler;
///
/// #[derive(Debug, RequestFilter)]
/// struct Handler {
///     compression: CompressionHandler,
///     static_files: StaticFilesHandler,
/// }
///
/// type Conf = <Handler as RequestFilter>::Conf;
///
/// let conf = Conf::load_from_yaml("test.yaml").ok().unwrap_or_else(Conf::default);
/// let handler: Handler = conf.try_into().unwrap();
/// ```
#[proc_macro_derive(RequestFilter)]
pub fn derive_request_filter(input: TokenStream) -> TokenStream {
    derive_request_filter::derive_request_filter(input)
        .unwrap_or_else(|err| err.into_compile_error().into())
}
