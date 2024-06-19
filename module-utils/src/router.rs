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

pub use crate::trie::LookupResult;
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
/// assert!(router.lookup("localhost", "/").is_some_and(|(value, _)| *value == "Localhost root"));
/// assert!(router.lookup("example.com", "/dir/file").is_some_and(|(value, _)| *value == "Website subdirectory"));
/// ```
#[derive(Debug)]
pub struct Router<Value> {
    trie: Trie<Value>,
}

impl<Value> Router<Value> {
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
    /// *Note*: Tail path will always start with a slash. For example, matching request path `/dir`
    /// and `/dir/` against the rule `/dir/` will both result in the tail path `/` being returned.
    pub fn lookup<'a>(
        &self,
        host: &(impl AsRef<[u8]> + ?Sized),
        path: &'a (impl AsRef<[u8]> + ?Sized),
    ) -> Option<(LookupResult<'_, Value>, Option<impl AsRef<[u8]> + 'a>)> {
        let path = path.as_ref();
        let (value, matched_segments) = self.trie.lookup(make_key(host, path))?;
        let host_segments = if host.as_ref().is_empty() { 0 } else { 1 };
        let tail = if matched_segments > host_segments {
            Some(PathTail {
                path,
                skip_segments: matched_segments - host_segments,
            })
        } else {
            None
        };

        Some((value, tail))
    }

    /// Retrieves the value from a previous lookup by its index
    pub fn retrieve(&self, index: usize) -> Option<&Value> {
        self.trie.retrieve(index)
    }
}

fn make_key<'a>(
    host: &'a (impl AsRef<[u8]> + ?Sized),
    path: &'a (impl AsRef<[u8]> + ?Sized),
) -> Box<dyn Iterator<Item = &'a [u8]> + 'a> {
    let path_iter = path
        .as_ref()
        .split(|c| *c == SEPARATOR)
        .filter(|s| !s.is_empty());

    let host = host.as_ref();
    if host.is_empty() {
        Box::new(path_iter)
    } else {
        Box::new(std::iter::once(host).chain(path_iter))
    }
}

struct PathTail<'a> {
    path: &'a [u8],
    skip_segments: usize,
}

impl<'a> AsRef<[u8]> for PathTail<'a> {
    fn as_ref(&self) -> &'a [u8] {
        let mut tail_start = 0;
        let mut segments = self.skip_segments;
        let mut expect_separator = true;

        while segments > 0 && tail_start < self.path.len() {
            if self.path[tail_start] != SEPARATOR {
                expect_separator = false;
            } else if !expect_separator {
                expect_separator = true;
                segments -= 1;
            }

            if segments > 0 {
                tail_start += 1;
            }
        }

        if tail_start < self.path.len() {
            &self.path[tail_start..]
        } else {
            b"/"
        }
    }
}

/// The router builder used to set up a [`Router`] instance
#[derive(Debug)]
pub struct RouterBuilder<Value> {
    inner: TrieBuilder<Value>,
}

impl<Value> RouterBuilder<Value> {
    /// Adds a host/path combination with the respective value to the routing table.
    ///
    /// While it is possible to use an empty host name, it is advisable to keep entries with an
    /// empty host name and those with non-empty host names in separate routers. Otherwise lookup
    /// might confuse host names and first segments of the path name.
    pub fn push(&mut self, host: impl AsRef<[u8]>, path: impl AsRef<[u8]>, value: Value)
    where
        Value: Debug,
    {
        let key = make_key(&host, &path).fold(Vec::new(), |mut result, segment| {
            if !result.is_empty() {
                result.push(SEPARATOR);
            }
            result.extend_from_slice(segment);
            result
        });
        if self.inner.push(key, None, value) {
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
        let (value, tail) = router.lookup(host, path)?;
        let tail = if let Some(tail) = tail {
            String::from_utf8_lossy(tail.as_ref()).to_string()
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
        builder.push("", "/abc", 7);
        let router = builder.build();

        assert_eq!(lookup(&router, "localhost", "/"), Some((1, "/".into())));
        assert_eq!(lookup(&router, "localhost", "/ab"), Some((1, "/ab".into())));
        assert_eq!(lookup(&router, "localhost", "/abc"), Some((2, "/".into())));
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
            Some((3, "/".into()))
        );
        assert_eq!(lookup(&router, "example.com", "/"), Some((4, "/".into())));
        assert_eq!(
            lookup(&router, "example.com", "/abc"),
            Some((4, "/abc".into()))
        );
        assert_eq!(
            lookup(&router, "example.com", "/abc/def"),
            Some((5, "/".into()))
        );
        assert_eq!(lookup(&router, "example.com", "/x/"), Some((6, "/".into())));
        assert_eq!(
            lookup(&router, "example.com", "/xyz"),
            Some((4, "/xyz".into()))
        );
        assert_eq!(lookup(&router, "example.net", "/"), None);
        assert_eq!(lookup(&router, "example.net", "/abc"), None);
        assert_eq!(lookup(&router, "", "/"), None);
        assert_eq!(lookup(&router, "", "/abc"), Some((7, "/".into())));
        assert_eq!(lookup(&router, "", "/abc/def"), Some((7, "/def".into())));

        // A special case to keep in mind: slashes in host name will cause incorrect segmentation
        // of the path, essentially causing everything after the slash to be ignored. As such, this
        // is not an issue but it might become one as the implementation changes.
        assert_eq!(
            lookup(&router, "localhost/def", "/abc"),
            Some((2, "/".into()))
        );
    }
}
