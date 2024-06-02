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

//! Deserialization helpers allowing efficient merging of multiple structs without the downsides of
//! #[serde(flatten)]

use pingora_core::server::configuration::ServerConf;
use serde::{
    de::{Deserializer, Error},
    Deserialize,
};

/// Used to efficiently deserialize merged configurations
pub trait DeserializeMap<'de>: Deserialize<'de> {
    /// The visitor type used to deserialize this configuration
    type Visitor: MapVisitor<'de, Value = Self>;

    /// Creates a [`MapVisitor`] instance that can be used to deserialize the current type.
    fn visitor(self) -> Self::Visitor;
}

/// A special visitor type used by [`DeserializeMap`]
pub trait MapVisitor<'de> {
    /// Type produced by this visitor
    type Value;

    /// Should return `true` if the visitor can handle the given key
    fn accepts_field(field: &str) -> bool;

    /// Adds the supported fields of this type to the list
    fn list_fields(list: &mut Vec<&'static str>);

    /// Deserializes and stores the value for the given key
    fn visit_field<D>(&mut self, field: &str, deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>;

    /// Turns collected data into the value
    fn finalize<E>(self) -> Result<Self::Value, E>
    where
        E: Error;
}

macro_rules! impl_deserialize_map {
    {$name:ty {$($field:ident)*}} => {
        const FIELDS: &[&str] = &[
            $(
                stringify!($field),
            )*
        ];

        #[derive(Debug)]
        pub struct Visitor {
            inner: $name,
        }

        impl<'de> MapVisitor<'de> for Visitor {
            type Value = $name;
            fn accepts_field(field: &str) -> bool {
                FIELDS.contains(&field)
            }
            fn list_fields(list: &mut Vec<&'static str>) {
                list.extend_from_slice(FIELDS);
            }
            fn visit_field<D>(&mut self, field: &str, deserializer: D) -> Result<(), D::Error>
            where
                D: Deserializer<'de>
            {
                match field {
                    $(
                        stringify!($field) => {
                            self.inner.$field = Deserialize::deserialize(deserializer)?;
                            Ok(())
                        }
                    )*
                    other => {
                        Err(D::Error::unknown_field(other, FIELDS))
                    }
                }
            }
            fn finalize<E>(self) -> Result<Self::Value, E>
            where
                E: Error
            {
                Ok(self.inner)
            }
        }

        impl DeserializeMap<'_> for $name {
            type Visitor = Visitor;

            fn visitor(self) -> Self::Visitor {
                Visitor {
                    inner: self,
                }
            }
        }
    };
}

impl_deserialize_map!(ServerConf {
    version
    daemon
    error_log
    pid_file
    upgrade_sock
    user
    group
    threads
    work_stealing
    ca_file
    grace_period_seconds
    graceful_shutdown_timeout_seconds
    client_bind_to_ipv4
    client_bind_to_ipv6
    upstream_keepalive_pool_size
    upstream_connect_offload_threadpools
    upstream_connect_offload_thread_per_pool
});
