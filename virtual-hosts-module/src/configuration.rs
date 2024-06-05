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

use module_utils::DeserializeMap;
use std::collections::HashMap;

/// Subdirectory configuration
#[derive(Debug, Default, DeserializeMap)]
pub struct SubDirConf<C: Default> {
    /// If `true`, subdirectory will be removed from the URI before passing it on to the handler.
    pub strip_prefix: bool,
    /// Generic handler settings
    ///
    /// These settings are flattened and appear at the same level as `strip_prefix` in the
    /// configuration file.
    #[module_utils(flatten)]
    pub config: C,
}

/// Virtual host configuration
#[derive(Debug, Default, DeserializeMap)]
pub struct VirtualHostConf<C: Default> {
    /// List of additional names for the virtual host
    pub aliases: Vec<String>,
    /// If true, this virtual host should be used as fallback when no other virtual host
    /// configuration applies
    pub default: bool,
    /// Maps virtual host's subdirectories to their special configurations
    pub subdirs: HashMap<String, SubDirConf<C>>,
    /// Generic handler settings
    ///
    /// These settings are flattened and appear at the same level as `default` in the configuration
    /// file.
    #[module_utils(flatten)]
    pub config: C,
}

/// Virtual hosts configuration
#[derive(Debug, Default, DeserializeMap)]
pub struct VirtualHostsConf<C: Default> {
    /// Maps virtual host names to their configuration
    pub vhosts: HashMap<String, VirtualHostConf<C>>,
}
