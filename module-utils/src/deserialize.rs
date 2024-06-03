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
    fn visit_field<D>(self, field: &str, deserializer: D) -> Result<Self, D::Error>
    where
        Self: Sized,
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
            fn visit_field<D>(mut self, field: &str, deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>
            {
                match field {
                    $(
                        stringify!($field) => {
                            self.inner.$field = Deserialize::deserialize(deserializer)?;
                            Ok(self)
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

#[doc(hidden)]
pub mod _private {
    //! This is a hack meant to make configuration merging possible even with types that don’t
    //! implement `DeserializeSeed` and where `DeserializedSeed` cannot be implemented (foreign
    //! types). Normally, we would use specialization: use `DeserializeSeed` where available,
    //! implement special handling for types like `HashMap` and `Vec` and fall back to overwriting
    //! values everywhere else. But since specialization isn’t stable, we use this work-around
    //! instead:
    //! <https://lukaskalbertodt.github.io/2019/12/05/generalized-autoref-based-specialization.html>

    use serde::{
        de::{DeserializeSeed, MapAccess, Visitor},
        Deserialize, Deserializer,
    };
    use std::{collections::HashMap, fmt::Formatter, hash::Hash, marker::PhantomData};

    pub trait DeserializeMerge<'de, T> {
        fn deserialize_merge<D>(&self, initial: T, deserializer: D) -> Result<T, D::Error>
        where
            T: Sized,
            D: Deserializer<'de>;
    }

    // Last deref level: if nothing else works, fall back to the regular `Deserialize`
    // implementation, replacing existing value by new one.
    impl<'de, T> DeserializeMerge<'de, T> for PhantomData<T>
    where
        T: Deserialize<'de>,
    {
        fn deserialize_merge<D>(&self, _initial: T, deserializer: D) -> Result<T, D::Error>
        where
            D: Deserializer<'de>,
        {
            T::deserialize(deserializer)
        }
    }

    // Regular `HashMap` and `Vec` support: fill up old value with data from new one.
    impl<'de, T, I> DeserializeMerge<'de, T> for &PhantomData<T>
    where
        T: Deserialize<'de> + Extend<I> + IntoIterator<Item = I>,
    {
        fn deserialize_merge<D>(&self, mut initial: T, deserializer: D) -> Result<T, D::Error>
        where
            D: Deserializer<'de>,
        {
            initial.extend(T::deserialize(deserializer)?);
            Ok(initial)
        }
    }

    // `HashMap` with type supporting `DeserializeSeed`: for existing keys, merge the values.
    impl<'de, K, V> DeserializeMerge<'de, HashMap<K, V>> for &&PhantomData<HashMap<K, V>>
    where
        K: Deserialize<'de> + Eq + Hash,
        V: DeserializeSeed<'de, Value = V> + Default,
    {
        fn deserialize_merge<D>(
            &self,
            initial: HashMap<K, V>,
            deserializer: D,
        ) -> Result<HashMap<K, V>, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct HashVisitor<K, V> {
                inner: HashMap<K, V>,
            }

            impl<'de, K, V> Visitor<'de> for HashVisitor<K, V>
            where
                K: Deserialize<'de> + Eq + Hash,
                V: DeserializeSeed<'de, Value = V> + Default,
            {
                type Value = HashMap<K, V>;

                fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("a map")
                }

                fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    while let Some(key) = map.next_key()? {
                        let value = self.inner.remove(&key).unwrap_or_default();
                        self.inner.insert(key, map.next_value_seed(value)?);
                    }
                    Ok(self.inner)
                }
            }

            deserializer.deserialize_map(HashVisitor { inner: initial })
        }
    }

    // First deref level: use the type’s own `DeserializeSeed` implementation.
    impl<'de, T> DeserializeMerge<'de, T> for &&&PhantomData<T>
    where
        T: DeserializeSeed<'de, Value = T>,
    {
        fn deserialize_merge<D>(&self, initial: T, deserializer: D) -> Result<T, D::Error>
        where
            D: Deserializer<'de>,
        {
            initial.deserialize(deserializer)
        }
    }
}
