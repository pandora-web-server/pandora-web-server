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

use async_trait::async_trait;
use log::trace;
use pingora_core::{Error, ErrorType};
use pingora_proxy::Session;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

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
pub trait RequestFilter {
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

    /// Handles the current request.
    ///
    /// This is essentially identical to the `request_filter` method but is supposed to be called
    /// when there is only a single handler. Consequently, its result can be returned directly.
    async fn handle(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool, Box<Error>>
    where
        Self::CTX: Send,
    {
        let result = self.request_filter(session, ctx).await?;
        Ok(result == RequestFilterResult::ResponseSent)
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
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>>;
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

/// This macro merges multiple structures implementing `structopt::StructOpt` into a structure
/// containing all of them while making certain that all command line flags can be used.
///
/// *Note*: Support for `struct` syntax is limited when it comes to generics.
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
/// merge_opt!{
///     /// Starts my great application.
///     ///
///     /// Additional application description just to make a structopt bug work-around work.
///     struct Opt {
///         app: MyAppOpt,
///         server: ServerOpt,
///         static_files: StaticFilesOpt,
///     }
/// }
///
/// let opt = Opt::from_args();
/// println!("Application options: {:?}", opt.app);
/// println!("Pingora server options: {:?}", opt.server);
/// println!("Static files options: {:?}", opt.static_files);
/// ```
#[macro_export]
macro_rules! merge_opt {
    (
        $(#[$struct_attr:meta])*
        $struct_vis:vis struct $struct_name:ident $(<$($generic:ident $(: $bound:path)?),+>)?
        $(
            where
                $(
                    $where_type:ty: $where_bounds:path
                ),+
        )?
        {
            $(
                $(#[$field_attr:meta])*
                $field_vis:vis $field_name:ident: $field_type:ty,
            )*
        }
    ) => {
        #[derive(::std::fmt::Debug, ::structopt::StructOpt)]
        #[doc(hidden)]
        $(#[$struct_attr])*
        struct __Dummy {}

        $(#[$struct_attr])*
        #[derive(::std::fmt::Debug, ::structopt::StructOpt)]
        $struct_vis struct $struct_name $(<$($generic $(: $bound)?),+>)?
        $(
            where
                $(
                    $where_type: $where_bounds
                ),+
        )?
        {
            $(
                #[structopt(flatten)]
                $(#[$field_attr])*
                $field_vis $field_name: $field_type,
            )*

            // This is a work-around for a https://github.com/TeXitoi/structopt/issues/539 (this
            // bug won't be fixed).
            #[structopt(flatten)]
            _dummy: __Dummy,
        }
    }
}

/// This macro merges multiple structures implementing [`serde::Deserialize`] and [`Default`] into
/// a structure containing all of them.
///
/// The structure of the expected configuration file is
/// flattened, so that the configuration settings from each component are still expected to be
/// found on the top level.
///
/// *Note*: Support for `struct` syntax is limited when it comes to generics.
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
/// merge_conf!{
///     struct Conf {
///         app: MyAppConf,
///         server: ServerConf,
///         static_files: StaticFilesConf,
///     }
/// }
///
/// let conf = Conf::load_from_yaml("test.yaml").ok().unwrap_or_else(Conf::default);
/// println!("Application settings: {:?}", conf.app);
/// println!("Pingora server settings: {:?}", conf.server);
/// println!("Static files settings: {:?}", conf.static_files);
/// ```
#[macro_export]
macro_rules! merge_conf {
    (
        $(#[$struct_attr:meta])*
        $struct_vis:vis struct $struct_name:ident $(<$($generic:ident $(: $bound:path)?),+>)?
        $(
            where
                $(
                    $where_type:ty: $where_bounds:path
                ),+
        )?
        {
            $(
                $(#[$field_attr:meta])*
                $field_vis:vis $field_name:ident: $field_type:ty,
            )*
        }
    ) => {
        $(#[$struct_attr])*
        #[derive(::std::fmt::Debug, ::std::default::Default, ::serde::Deserialize)]
        #[serde(default)]
        $struct_vis struct $struct_name $(<$($generic $(: $bound)?),+>)?
        $(
            where
                $(
                    $where_type: $where_bounds
                ),+
        )?
        {
            $(
                #[serde(flatten)]
                $(#[$field_attr])*
                $field_vis $field_name: $field_type,
            )*
        }
    }
}

/// This macro chains multiple handlers implementing [`RequestFilter`] and merges their respective
/// configurations.
///
/// The handlers will be called in the order in which they are listed. Each handler can prevent the
/// subsequent handlers from being called by returning [`RequestFilterResult::ResponseSent`] or
/// [`RequestFilterResult::Handled`].
///
/// *Note*: Support for `struct` syntax is limited when it comes to generics.
///
/// ```rust,no_run
/// use module_utils::{chain_handlers, FromYaml, RequestFilter};
/// use compression_module::CompressionHandler;
/// use static_files_module::StaticFilesHandler;
///
/// chain_handlers!{
///     struct Handler {
///         compression: CompressionHandler,
///         static_files: StaticFilesHandler,
///     }
/// }
///
/// type Conf = <Handler as RequestFilter>::Conf;
///
/// let conf = Conf::load_from_yaml("test.yaml").ok().unwrap_or_else(Conf::default);
/// let handler: Handler = conf.try_into().unwrap();
/// ```
#[macro_export]
macro_rules! chain_handlers {
    (
        $(#[$struct_attr:meta])*
        $struct_vis:vis struct $struct_name:ident $(<$($generic:ident $(: $bound:path)?),+>)?
        $(
            where
                $(
                    $where_type:ty: $where_bounds:path
                ),+
        )?
        {
            $(
                $(#[$field_attr:meta])*
                $field_vis:vis $field_name:ident: $field_type:ty,
            )*
        }
    ) => {
        $(#[$struct_attr])*
        #[derive(::std::fmt::Debug)]
        $struct_vis struct $struct_name $(<$($generic $(: $bound)?),+>)?
        $(
            where
                $(
                    $where_type: $where_bounds
                ),+
        )?
        {
            $(
                $(#[$field_attr])*
                $field_vis $field_name: $field_type,
            )*
        }

        /// Merged handler configuration
        ::module_utils::merge_conf!{
            $struct_vis struct __Conf $(<$($generic $(: $bound)?),+>)?
            $(
                where
                    $(
                        $where_type: $where_bounds
                    ),+
            )?
            {
                $(
                    $field_vis $field_name: <$field_type as ::module_utils::RequestFilter>::Conf,
                )*
            }
        }

        /// Merged handler context
        $struct_vis struct __CTX $(<$($generic $(: $bound)?),+>)?
        $(
            where
                $(
                    $where_type: $where_bounds
                ),+
        )?
        {
            $(
                $field_vis $field_name: <$field_type as ::module_utils::RequestFilter>::CTX,
            )*
        }

        impl ::std::convert::TryFrom<__Conf> for $struct_name {
            type Error = ::std::boxed::Box<::pingora_core::Error>;

            fn try_from(conf: __Conf) -> ::std::result::Result<Self, Self::Error> {
                $(
                    let $field_name = <$field_type>::try_from(conf.$field_name)?;
                )*
                ::std::result::Result::Ok(Self {
                    $(
                        $field_name,
                    )*
                })
            }
        }

        #[async_trait::async_trait]
        impl ::module_utils::RequestFilter for $struct_name {
            type Conf = __Conf;
            type CTX = __CTX;

            fn new_ctx() -> Self::CTX {
                $(
                    let $field_name = <$field_type>::new_ctx();
                )*
                Self::CTX {
                    $(
                        $field_name,
                    )*
                }
            }

            async fn request_filter(
                &self,
                _session: &mut ::pingora_proxy::Session,
                _ctx: &mut Self::CTX,
            ) -> ::std::result::Result<::module_utils::RequestFilterResult, ::std::boxed::Box<::pingora_core::Error>> {
                $(
                    let result = self.$field_name.request_filter(_session, &mut _ctx.$field_name).await?;
                    if result != ::module_utils::RequestFilterResult::Unhandled {
                        return ::std::result::Result::Ok(result);
                    }
                )*
                ::std::result::Result::Ok(module_utils::RequestFilterResult::Unhandled)
            }
        }
    }
}
