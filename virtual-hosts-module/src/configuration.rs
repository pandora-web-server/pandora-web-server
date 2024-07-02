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

use pandora_module_utils::serde::Deserialize;
use pandora_module_utils::{DeserializeMap, OneOrMany};
use std::collections::HashMap;

/// Determines which paths a configuration should apply to
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(crate = "pandora_module_utils::serde", from = "String")]
pub struct PathMatchRule {
    /// Path to match
    pub path: String,

    /// If `true`, only exact path matches will be accepted. Otherwise exact and prefix matches
    /// will be accepted.
    pub exact: bool,
}

impl From<&str> for PathMatchRule {
    fn from(path: &str) -> Self {
        if let Some(path) = path.strip_suffix("/*") {
            Self {
                path: path.to_owned(),
                exact: false,
            }
        } else {
            Self {
                path: path.to_owned(),
                exact: true,
            }
        }
    }
}

impl From<String> for PathMatchRule {
    fn from(value: String) -> Self {
        value.as_str().into()
    }
}

/// Configuration of a path within a virtual host
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct SubPathConf<C: Default> {
    /// If `true`, matched path will be removed from the URI before passing it on to the handler.
    pub strip_prefix: bool,
    /// Generic handler settings
    ///
    /// These settings are flattened and appear at the same level as `strip_prefix` in the
    /// configuration file.
    #[pandora(flatten)]
    pub config: C,
}

/// Virtual host configuration
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct VirtualHostConf<C: Default> {
    /// If true, this virtual host should be used as fallback when no other virtual host
    /// configuration applies
    pub default: bool,
    /// Maps virtual host's paths to their special configurations
    pub subpaths: HashMap<PathMatchRule, SubPathConf<C>>,
    /// Generic handler settings
    ///
    /// These settings are flattened and appear at the same level as `default` in the configuration
    /// file.
    #[pandora(flatten)]
    pub config: C,
}

/// Virtual hosts configuration
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct VirtualHostsConf<C: Default> {
    /// Maps virtual host names to their configuration
    pub vhosts: HashMap<OneOrMany<String>, VirtualHostConf<C>>,
}
