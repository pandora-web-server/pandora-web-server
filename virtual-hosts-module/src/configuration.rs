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

use module_utils::merge_conf;
use serde::Deserialize;
use std::collections::HashMap;

/// Additional configuration settings for a subdirectory
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SubDirConf {
    /// If `true`, subdirectory will be removed from the URI before passing it on to the handler.
    pub strip_prefix: bool,
}

/// Combined configuration structure for virtual hosts
///
/// This merges the settings from both member fields via `serde(flatten)`.
#[merge_conf]
pub struct SubDirCombined<C: Default> {
    /// Subdirectory specific settings
    pub subdir: SubDirConf,
    /// Generic handler settings
    pub config: C,
}

/// Additional configuration settings for a virtual host
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VirtualHostConf<C: Default> {
    /// List of additional names for the virtual host
    pub aliases: Vec<String>,
    /// If true, this virtual host should be used as fallback when no other virtual host
    /// configuration applies
    pub default: bool,
    /// Maps virtual host's subdirectories to their special configurations
    pub subdirs: HashMap<String, SubDirCombined<C>>,
}

/// Combined configuration structure for virtual hosts
///
/// This merges the settings from both member fields via `serde(flatten)`.
#[merge_conf]
pub struct VirtualHostCombined<C: Default> {
    /// Virtual host specific settings
    pub host: VirtualHostConf<C>,
    /// Generic handler settings
    pub config: C,
}

/// Virtual hosts configuration
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VirtualHostsConf<C: Default> {
    /// Maps virtual host names to their configuration
    pub vhosts: HashMap<String, VirtualHostCombined<C>>,
}
