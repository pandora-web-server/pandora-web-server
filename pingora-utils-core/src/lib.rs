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

//! # Helpers for Pingora handlers
//!
//! This crate contains some helpers that are useful when using `static-files-module` crate for
//! example.

use log::trace;
use pingora_core::{Error, ErrorType};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

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
/// ```rust
/// use pingora_core::server::configuration::Opt as ServerOpt;
/// use pingora_utils_core::merge_opt;
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
        struct $struct_name:ident {
            $(
                $field_vis:vis $field_name:ident: $field_type:ty,
            )*
        }
    ) => {
        #[derive(Debug, structopt::StructOpt)]
        $(#[$struct_attr])*
        struct Dummy {}

        $(#[$struct_attr])*
        #[derive(Debug, structopt::StructOpt)]
        struct $struct_name {
            $(
                #[structopt(flatten)]
                $field_vis $field_name: $field_type,
            )*

            // This is a work-around for a https://github.com/TeXitoi/structopt/issues/539 (this
            // bug won't be fixed).
            #[structopt(flatten)]
            _dummy: Dummy,
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
/// ```rust
/// use pingora_core::server::configuration::ServerConf;
/// use pingora_utils_core::{merge_conf, FromYaml};
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
        struct $struct_name:ident {
            $(
                $field_vis:vis $field_name:ident: $field_type:ty,
            )*
        }
    ) => {
        $(#[$struct_attr])*
        #[derive(Debug, Default, serde::Deserialize)]
        #[serde(default)]
        struct $struct_name {
            $(
                #[serde(flatten)]
                $field_vis $field_name: $field_type,
            )*
        }
    }
}
