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

        let conf: Self = serde_yaml::from_reader(reader).map_err(|err| {
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
