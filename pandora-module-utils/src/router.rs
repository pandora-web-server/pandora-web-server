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
//! will all match a rule defined for `/dir/`.
//!
//! A route can require an exact match (only the precise host/path combination is accepted), or it
//! can allow prefix matches: a host/path combination will also be considered a match if the host
//! matches and path points to a location within the rule’s directory. Note that `/dirabc` never
//! matches a route defined for `/dir`, only `/dir/abc` does.
//!
//! Empty host name is considered the fallback host, its values apply to all hosts but with a lower
//! priority than values designated to the host.
//!
//! Only the best match is returned. If rules exist for `/`, `/dir/` and `/dir/subdir/` for
//! example, the path `/dir/subdir/file` will match `/dir/subdir/`.

use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Deref;

pub use crate::trie::LookupResult;
use crate::trie::{common_prefix_length, Trie, SEPARATOR};

/// Empty path
pub const EMPTY_PATH: &Path = &Path { path: Vec::new() };

/// Encapsulates a router path
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    pub(crate) path: Vec<u8>,
}

impl Path {
    /// Creates a new router path for given host and path
    pub fn new(path: impl AsRef<[u8]>) -> Self {
        Self {
            path: Self::normalize(path),
        }
    }

    /// Normalizes the path by removing unnecessary separators
    fn normalize(path: impl AsRef<[u8]>) -> Vec<u8> {
        let mut had_separator = true;
        let mut path: Vec<u8> = path
            .as_ref()
            .iter()
            .copied()
            .filter(|b| {
                if *b == SEPARATOR {
                    if had_separator {
                        false
                    } else {
                        had_separator = true;
                        true
                    }
                } else {
                    had_separator = false;
                    true
                }
            })
            .collect();

        if path.ends_with(&[SEPARATOR]) {
            path.pop();
        }

        path
    }

    /// Checks whether this path is a parent of the other path
    pub fn is_prefix_of(&self, other: &Path) -> bool {
        common_prefix_length(&self.path, &other.path) == self.path.len()
    }

    /// If this path is a non-empty prefix of the given path, removes the prefix. Otherwise returns
    /// `None`.
    pub fn remove_prefix_from<'a>(&self, path: &'a impl AsRef<[u8]>) -> Option<&'a [u8]> {
        if self.path.is_empty() {
            return None;
        }

        let mut path = path.as_ref();
        for segment in self.path.split(|b| *b == SEPARATOR) {
            while let [SEPARATOR, rest @ ..] = path {
                path = rest;
            }

            if !path.starts_with(segment)
                || path.get(segment.len()).is_some_and(|b| *b != SEPARATOR)
            {
                return None;
            }

            path = &path[segment.len()..];
        }

        if path.is_empty() {
            Some(b"/")
        } else {
            Some(path)
        }
    }
}

impl Debug for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.path))
    }
}

impl Deref for Path {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

/// The router implementation.
///
/// A new instance can be created by calling [`Router::builder`]. You add the rules and call
/// [`RouterBuilder::build`] to compile an efficient routing data structure:
///
/// ```rust
/// use pandora_module_utils::router::Router;
///
/// let mut builder = Router::builder();
/// builder.push("localhost", "/", "Localhost root", Some("Within localhost"));
/// builder.push("localhost", "/dir/", "Localhost subdirectory", None);
/// builder.push("example.com", "/", "Website root", Some("Within website"));
/// builder.push("example.com", "/dir/", "Website subdirectory", Some("Within website subdirectory"));
///
/// let router = builder.build();
/// assert_eq!(*router.lookup("localhost", "/").unwrap(), "Localhost root");
/// assert_eq!(*router.lookup("localhost", "/dir/file").unwrap(), "Within localhost");
/// assert_eq!(*router.lookup("example.com", "/dir/file").unwrap(), "Within website subdirectory");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Router<Value> {
    trie: Trie<Value>,
    fallback: Trie<Value>,
}

impl<Value> Router<Value> {
    /// Returns a builder instance that can be used to set up a router.
    ///
    /// Once set up, the router data structure is read-only and can be queried without any memory
    /// copying or allocations.
    pub fn builder() -> RouterBuilder<Value>
    where
        Value: Clone + Eq,
    {
        RouterBuilder {
            entries: Default::default(),
            fallbacks: Default::default(),
        }
    }

    /// Looks up a host/path combination in the routing table, returns the matching value if any.
    pub fn lookup(
        &self,
        host: &(impl AsRef<[u8]> + ?Sized),
        path: &(impl AsRef<[u8]> + ?Sized),
    ) -> Option<LookupResult<'_, Value>> {
        if !host.as_ref().is_empty() {
            self.trie.lookup(make_key(host, path))
        } else {
            None
        }
        .or_else(|| self.fallback.lookup(make_key("", path)))
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

/// Intermediate entry stored in the router prior to merging
#[derive(Debug)]
struct RouterEntry<Value> {
    path: Path,
    value_exact: Value,
    value_prefix: Option<Value>,
}

/// The router builder used to set up a [`Router`] instance
#[derive(Debug)]
pub struct RouterBuilder<Value> {
    entries: HashMap<Vec<u8>, Vec<RouterEntry<Value>>>,
    fallbacks: Vec<RouterEntry<Value>>,
}

impl<Value: Clone + Eq> RouterBuilder<Value> {
    fn merge_value(
        existing: &mut Vec<RouterEntry<Value>>,
        path: Path,
        value_exact: Value,
        mut value_prefix: Option<Value>,
    ) {
        match existing.binary_search_by_key(&path.as_slice(), |entry| entry.path.as_slice()) {
            Ok(index) => {
                existing[index].value_exact = value_exact;
                if value_prefix.is_some() {
                    existing[index].value_prefix = value_prefix;
                }
            }
            Err(index) => {
                // Adding a new entry.
                if value_prefix.is_none() {
                    // Copy `value_prefix` from closest parent.
                    for parent in existing[0..index].iter_mut().rev() {
                        if parent.path.is_prefix_of(&path) && parent.value_prefix.is_some() {
                            value_prefix.clone_from(&parent.value_prefix);
                            break;
                        }
                    }
                }

                existing.insert(
                    index,
                    RouterEntry {
                        path,
                        value_exact,
                        value_prefix,
                    },
                )
            }
        }
    }

    /// Adds a host/path combination with the respective values to the routing table.
    ///
    /// The `value_exact` value is only used for exact path matches. For prefix matches where only
    /// part of the lookup path matched the `value_prefix` value will be used if present.
    pub fn push(
        &mut self,
        host: impl AsRef<[u8]>,
        path: impl AsRef<[u8]>,
        value_exact: Value,
        value_prefix: Option<Value>,
    ) {
        let path = Path::new(path);

        let existing = if host.as_ref().is_empty() {
            &mut self.fallbacks
        } else {
            self.entries.entry(host.as_ref().to_vec()).or_default()
        };

        Self::merge_value(existing, path, value_exact, value_prefix);
    }

    /// Translates all rules into a router instance while also merging values if multiple apply to
    /// the same location.
    pub fn build(self) -> Router<Value> {
        let mut builder = Trie::builder();
        for (host, entries) in self.entries {
            for entry in entries {
                let mut key = host.clone();
                if !entry.path.is_empty() {
                    key.push(SEPARATOR);
                    key.extend_from_slice(&entry.path);
                }
                builder.push(key, entry.value_exact, entry.value_prefix);
            }
        }

        let mut fallback_builder = Trie::builder();
        for entry in self.fallbacks {
            fallback_builder.push(entry.path.path, entry.value_exact, entry.value_prefix);
        }

        Router {
            trie: builder.build(),
            fallback: fallback_builder.build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::merger::HostPathMatcher;

    use super::*;

    #[test]
    fn path_normalization() {
        assert_eq!(&Path::new("").path, b"");
        assert_eq!(&Path::new("/").path, b"");
        assert_eq!(&Path::new("///").path, b"");
        assert_eq!(&Path::new("abc").path, b"abc");
        assert_eq!(&Path::new("//abc//").path, b"abc");
        assert_eq!(&Path::new("abc/def").path, b"abc/def");
        assert_eq!(&Path::new("//abc//def//").path, b"abc/def");
    }

    #[test]
    fn path_remove_prefix() {
        assert_eq!(Path::new("").remove_prefix_from(b"/"), None);
        assert_eq!(Path::new("").remove_prefix_from(b"/abc"), None);
        assert_eq!(Path::new("///").remove_prefix_from(b"/"), None);
        assert_eq!(Path::new("///").remove_prefix_from(b"/abc"), None);
        assert_eq!(
            Path::new("abc").remove_prefix_from(b"/abc"),
            Some("/".as_bytes())
        );
        assert_eq!(Path::new("abc").remove_prefix_from(b"/def"), None);
        assert_eq!(Path::new("abc").remove_prefix_from(b"/abcd"), None);
        assert_eq!(
            Path::new("abc").remove_prefix_from(b"/abc//d"),
            Some("//d".as_bytes())
        );
        assert_eq!(
            Path::new("/abc/def/").remove_prefix_from(b"/abc//def"),
            Some("/".as_bytes())
        );
        assert_eq!(
            Path::new("/abc/def/").remove_prefix_from(b"/abc//def\\xyz"),
            None
        );
        assert_eq!(
            Path::new("/abc/def/").remove_prefix_from(b"/abc//def/xyz"),
            Some("/xyz".as_bytes())
        );
    }

    #[test]
    fn host_path_matcher_parsing() {
        assert_eq!(
            HostPathMatcher::from(""),
            HostPathMatcher {
                host: b"".to_vec(),
                path: Path::new(""),
                exact: false,
            }
        );

        assert_eq!(
            HostPathMatcher::from("/*"),
            HostPathMatcher {
                host: b"".to_vec(),
                path: Path::new(""),
                exact: false,
            }
        );

        assert_eq!(
            HostPathMatcher::from("abc/*"),
            HostPathMatcher {
                host: b"abc".to_vec(),
                path: Path::new(""),
                exact: false,
            }
        );

        assert_eq!(
            HostPathMatcher::from("/abc/*"),
            HostPathMatcher {
                host: b"".to_vec(),
                path: Path::new("abc"),
                exact: false,
            }
        );

        assert_eq!(
            HostPathMatcher::from("abc"),
            HostPathMatcher {
                host: b"abc".to_vec(),
                path: Path::new(""),
                exact: false,
            }
        );

        assert_eq!(
            HostPathMatcher::from("/abc"),
            HostPathMatcher {
                host: b"".to_vec(),
                path: Path::new("abc"),
                exact: true,
            }
        );

        assert_eq!(
            HostPathMatcher::from("/abc*"),
            HostPathMatcher {
                host: b"".to_vec(),
                path: Path::new("abc*"),
                exact: true,
            }
        );

        assert_eq!(
            HostPathMatcher::from("localhost/"),
            HostPathMatcher {
                host: b"localhost".to_vec(),
                path: Path::new(""),
                exact: true,
            }
        );

        assert_eq!(
            HostPathMatcher::from("localhost/abc/*"),
            HostPathMatcher {
                host: b"localhost".to_vec(),
                path: Path::new("abc"),
                exact: false,
            }
        );

        assert_eq!(
            HostPathMatcher::from("localhost///abc///"),
            HostPathMatcher {
                host: b"localhost".to_vec(),
                path: Path::new("abc"),
                exact: true,
            }
        );
    }

    #[test]
    fn routing() {
        fn lookup(router: &Router<u8>, host: &str, path: &str) -> Option<u8> {
            router.lookup(host, path).as_deref().copied()
        }

        let mut builder = Router::builder();
        builder.push("localhost", "/", 1u8, Some(1));
        builder.push("localhost", "/abc", 2, Some(2));
        builder.push("localhost", "/xyz/abc/", 3, Some(3));
        builder.push("example.com", "", 4, Some(4));
        builder.push("example.com", "/abc/def/", 5, Some(5));
        builder.push("example.com", "/x", 6, Some(6));
        builder.push("", "/abc", 7, Some(7));
        let router = builder.build();

        assert_eq!(lookup(&router, "localhost", "/"), Some(1));
        assert_eq!(lookup(&router, "localhost", "/ab"), Some(1));
        assert_eq!(lookup(&router, "localhost", "/abc"), Some(2));
        assert_eq!(lookup(&router, "localhost", "/abc/"), Some(2));
        assert_eq!(lookup(&router, "localhost", "/abc/d"), Some(2));
        assert_eq!(lookup(&router, "localhost", "/abc/d/"), Some(2));
        assert_eq!(lookup(&router, "localhost", "/xyz"), Some(1));
        assert_eq!(lookup(&router, "localhost", "/xyz/"), Some(1));
        assert_eq!(lookup(&router, "localhost", "/xyz/abc"), Some(3));
        assert_eq!(lookup(&router, "example.com", "/"), Some(4));
        assert_eq!(lookup(&router, "example.com", "/abc"), Some(4));
        assert_eq!(lookup(&router, "example.com", "/abc/def"), Some(5));
        assert_eq!(lookup(&router, "example.com", "/x/"), Some(6));
        assert_eq!(lookup(&router, "example.com", "/xyz"), Some(4));
        assert_eq!(lookup(&router, "example.net", "/"), None);
        assert_eq!(lookup(&router, "example.net", "/abc"), Some(7));
        assert_eq!(lookup(&router, "", "/"), None);
        assert_eq!(lookup(&router, "", "/abc"), Some(7));
        assert_eq!(lookup(&router, "", "/abc/def"), Some(7));

        // A special case to keep in mind: slashes in host name will cause incorrect segmentation
        // of the path, essentially causing everything after the slash to be ignored. As such, this
        // is not an issue but it might become one as the implementation changes.
        assert_eq!(lookup(&router, "localhost/def", "/abc"), Some(2));
    }
}
