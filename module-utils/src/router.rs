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

//! Implements efficient prefix routing based on the trie data structure.
//!
//! As far as the routing is concerned, a path is a list of file/directory names separated by
//! slashes. The number of separating slashes is irrelevant, so that `/dir`, `/dir/` and `//dir/`
//! will all match the rule defined for `/dir/`.
//!
//! Only the best match is returned. If rules exist for `/`, `/dir/` and `/dir/subdir/`, the path
//! `/dir/subdir/file` will match `/dir/subdir/`.
//!
//! Host name matches are always exact. If the requested host doesn’t exist in the rules, no match
//! will be returned.

use log::warn;
use std::fmt::Debug;

use crate::trie::{Trie, TrieBuilder, SEPARATOR};

/// The router implementation.
///
/// A new instance can be created by calling [`Router::builder`]. You add the rules and call
/// [`RouterBuilder::build`] to compile an efficient routing data structure:
///
/// ```rust
/// use module_utils::router::Router;
///
/// let mut builder = Router::builder();
/// builder.push("localhost", "/", "Localhost root");
/// builder.push("localhost", "/dir/", "Localhost subdirectory");
/// builder.push("example.com", "/", "Website root");
/// builder.push("example.com", "/dir/", "Website subdirectory");
///
/// let router = builder.build();
/// assert!(matches!(router.lookup("localhost".as_bytes(), "/".as_bytes()), Some((&"Localhost root", _))));
/// assert!(matches!(router.lookup("example.com".as_bytes(), "/dir/file".as_bytes()), Some((&"Website subdirectory", _))));
/// ```
#[derive(Debug)]
pub struct Router<Value: Debug> {
    trie: Trie<Value>,
}

impl<Value: Debug> Router<Value> {
    /// Returns a builder instance that can be used to set up a router.
    ///
    /// Once set up, the router data structure is read-only and can be queried without any memory
    /// copying or allocations.
    pub fn builder() -> RouterBuilder<Value> {
        RouterBuilder {
            inner: Trie::builder(),
        }
    }

    /// Looks up a host/path combination in the routing table, returns the matching value if any.
    ///
    /// The second part of the return value is the tail path. It is only present if part of the
    /// path matched the rule’s path. The iterator will produce the remaining part of the path
    /// then.
    ///
    /// *Note*: While the tail path will usually start with a slash, it could also be empty. For
    /// example, matching request path `/dir/` and `/dir/file` against the rule `/dir/` will result
    /// in the tail path `/` and `/file` respectively, but matching `/dir` will result in an empty
    /// tail path.
    pub fn lookup<'a>(
        &self,
        host: &'a [u8],
        path: &'a [u8],
    ) -> Option<(&Value, Option<impl Iterator<Item = u8> + 'a>)> {
        let (value, matched_segments) = self.trie.lookup(make_key(host, path))?;
        let tail = if matched_segments > 1 {
            let mut skip_segments = matched_segments - 1;
            let mut expect_separator = true;
            Some(path.iter().copied().filter(move |c| {
                if skip_segments > 0 {
                    if *c != SEPARATOR {
                        expect_separator = false;
                    } else if !expect_separator {
                        expect_separator = true;
                        skip_segments -= 1;
                    }
                }

                skip_segments == 0
            }))
        } else {
            None
        };

        Some((value, tail))
    }
}

fn make_key<'a>(host: &'a [u8], path: &'a [u8]) -> impl Iterator<Item = &'a [u8]> {
    std::iter::once(host).chain(path.split(|c| *c == SEPARATOR).filter(|s| !s.is_empty()))
}

/// The router builder used to set up a [`Router`] instance
#[derive(Debug)]
pub struct RouterBuilder<Value: Debug> {
    inner: TrieBuilder<Value>,
}

impl<Value: Debug> RouterBuilder<Value> {
    /// Adds a host/path combination with the respective value to the routing table.
    pub fn push<H: AsRef<[u8]>, P: AsRef<[u8]>>(&mut self, host: H, path: P, value: Value) {
        let key = make_key(host.as_ref(), path.as_ref()).fold(Vec::new(), |mut result, segment| {
            if !result.is_empty() {
                result.push(SEPARATOR);
            }
            result.extend_from_slice(segment);
            result
        });
        if self.inner.push(key, value) {
            warn!(
                "Multiple routing entries for host {} and path {}, only considering one",
                String::from_utf8_lossy(host.as_ref()),
                String::from_utf8_lossy(path.as_ref())
            );
        }
    }

    /// Translates all rules into a router instance.
    pub fn build(self) -> Router<Value> {
        Router {
            trie: self.inner.build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(router: &Router<u8>, host: &str, path: &str) -> Option<(u8, String)> {
        let (value, tail) = router.lookup(host.as_bytes(), path.as_bytes())?;
        let tail = if let Some(tail) = tail {
            let tail: Vec<_> = tail.collect();
            String::from_utf8_lossy(&tail).to_string()
        } else {
            path.to_owned()
        };
        Some((*value, tail))
    }

    #[test]
    fn routing() {
        let mut builder = Router::builder();
        builder.push("localhost", "/", 1u8);
        builder.push("localhost", "/abc", 2);
        builder.push("localhost", "/xyz/abc/", 3);
        builder.push("example.com", "", 4);
        builder.push("example.com", "/abc/def/", 5);
        builder.push("example.com", "/x", 6);
        let router = builder.build();

        assert_eq!(lookup(&router, "localhost", "/"), Some((1, "/".into())));
        assert_eq!(lookup(&router, "localhost", "/ab"), Some((1, "/ab".into())));
        assert_eq!(lookup(&router, "localhost", "/abc"), Some((2, "".into())));
        assert_eq!(lookup(&router, "localhost", "/abc/"), Some((2, "/".into())));
        assert_eq!(
            lookup(&router, "localhost", "/abc/d"),
            Some((2, "/d".into()))
        );
        assert_eq!(
            lookup(&router, "localhost", "/abc/d/"),
            Some((2, "/d/".into()))
        );
        assert_eq!(
            lookup(&router, "localhost", "/xyz"),
            Some((1, "/xyz".into()))
        );
        assert_eq!(
            lookup(&router, "localhost", "/xyz/"),
            Some((1, "/xyz/".into()))
        );
        assert_eq!(
            lookup(&router, "localhost", "/xyz/abc"),
            Some((3, "".into()))
        );
        assert_eq!(lookup(&router, "example.com", "/"), Some((4, "/".into())));
        assert_eq!(
            lookup(&router, "example.com", "/abc"),
            Some((4, "/abc".into()))
        );
        assert_eq!(
            lookup(&router, "example.com", "/abc/def"),
            Some((5, "".into()))
        );
        assert_eq!(lookup(&router, "example.com", "/x/"), Some((6, "/".into())));
        assert_eq!(
            lookup(&router, "example.com", "/xyz"),
            Some((4, "/xyz".into()))
        );
        assert_eq!(lookup(&router, "example.net", "/"), None);
        assert_eq!(lookup(&router, "example.net", "/abc"), None);

        // A special case to keep in mind: slashes in host name will cause incorrect segmentation
        // of the path, essentially causing everything after the slash to be ignored. As such, this
        // is not an issue but it might become one as the implementation changes.
        assert_eq!(
            lookup(&router, "localhost/def", "/abc"),
            Some((2, "".into()))
        );
    }
}
