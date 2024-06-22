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
//! By default, only the best match is returned. If rules exist for `/`, `/dir/` and `/dir/subdir/`
//! for example, the path `/dir/subdir/file` will match `/dir/subdir/`. To change that behavior,
//! you can give `RouterBuilder` a custom `Merger` type. Then the result will be a merged value,
//! with the more distant matches merged in first.

use std::fmt::Debug;
use std::ops::Deref;
use std::{collections::HashMap, marker::PhantomData};

pub use crate::trie::LookupResult;
use crate::trie::{common_prefix_length, Trie, SEPARATOR};

/// Empty path
pub const EMPTY_PATH: &Path = &Path { path: Vec::new() };

/// Encapsulates a router path
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    path: Vec<u8>,
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

    /// Calculates the number of path segments in this path
    fn num_segments(&self) -> usize {
        if self.path.is_empty() {
            0
        } else {
            self.path.iter().filter(|b| **b == SEPARATOR).count() + 1
        }
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
/// use module_utils::router::Router;
///
/// let mut builder = Router::builder();
/// builder.push("localhost", "/", "Localhost root", Some("Localhost root"));
/// builder.push("localhost", "/dir/", "Localhost subdirectory", None);
/// builder.push("example.com", "/", "Website root", Some("Website root"));
/// builder.push("example.com", "/dir/", "Website subdirectory", Some("Website subdirectory"));
///
/// let router = builder.build();
/// assert!(router.lookup("localhost", "/").is_some_and(|(value, _)| *value == "Localhost root"));
/// assert!(router.lookup("localhost", "/dir/file").is_some_and(|(value, _)| *value == "Localhost root"));
/// assert!(router.lookup("example.com", "/dir/file").is_some_and(|(value, _)| *value == "Website subdirectory"));
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
        Default::default()
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
        let key = make_key(host, path);
        let path = path.as_ref();
        let (value, matched_segments) = if host.as_ref().is_empty() {
            self.fallback.lookup(key)?
        } else {
            self.trie.lookup(key)?
        };
        let tail = if matched_segments > 0 {
            Some(PathTail {
                path,
                skip_segments: matched_segments,
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

/// Intermediate entry stored in the router prior to merging
#[derive(Debug)]
struct RouterEntry<Value> {
    path: Path,
    value_exact: (Value, usize),
    value_prefix: Option<(Value, usize)>,
}

/// Trait allowing to merge two router values
pub trait Merge<Value> {
    /// Merges the value `new` into the existing value `current`.
    fn merge(current: &mut Value, new: Value);
}

/// Default merging implementation, replaces existing value by the incoming one.
#[derive(Debug)]
pub struct DefaultMerger;
impl<Value> Merge<Value> for DefaultMerger {
    fn merge(current: &mut Value, new: Value) {
        *current = new;
    }
}

/// The router builder used to set up a [`Router`] instance
#[derive(Debug)]
pub struct RouterBuilder<Value, Merger: Merge<Value> = DefaultMerger> {
    entries: HashMap<Vec<u8>, Vec<RouterEntry<Value>>>,
    fallbacks: Vec<RouterEntry<Value>>,
    marker: PhantomData<Merger>,
}

impl<Value, Merger: Merge<Value>> Default for RouterBuilder<Value, Merger> {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            fallbacks: Default::default(),
            marker: Default::default(),
        }
    }
}

impl<Value: Clone + Eq, Merger: Merge<Value>> RouterBuilder<Value, Merger> {
    fn merge_into(
        current: &mut RouterEntry<Value>,
        mut new_exact: (Value, usize),
        new_prefix: Option<(Value, usize)>,
        prefer_existing: bool,
    ) {
        if prefer_existing {
            std::mem::swap(&mut current.value_exact, &mut new_exact);
        }
        Merger::merge(&mut current.value_exact.0, new_exact.0);
        current.value_exact.1 = new_exact.1;

        if let Some(mut new_prefix) = new_prefix {
            if let Some(ref mut value_prefix) = &mut current.value_prefix {
                if prefer_existing {
                    std::mem::swap(value_prefix, &mut new_prefix);
                }
                Merger::merge(&mut value_prefix.0, new_prefix.0);
                value_prefix.1 = new_prefix.1;
            } else {
                current.value_prefix = Some(new_prefix);
            }
        }
    }

    fn merge_from(
        current_prefix: &(Value, usize),
        new_exact: &mut (Value, usize),
        new_prefix: &mut Option<(Value, usize)>,
        prefer_existing: bool,
    ) {
        let mut current = current_prefix.clone();
        if !prefer_existing {
            std::mem::swap(new_exact, &mut current);
        }
        Merger::merge(&mut new_exact.0, current.0);
        new_exact.1 = current.1;

        let mut current = current_prefix.clone();
        if let Some(new_prefix) = new_prefix {
            if !prefer_existing {
                std::mem::swap(new_prefix, &mut current);
            }
            Merger::merge(&mut new_prefix.0, current.0);
            new_prefix.1 = current.1;
        } else {
            *new_prefix = Some(current);
        }
    }

    fn merge_value(
        existing: &mut Vec<RouterEntry<Value>>,
        path: Path,
        mut value_exact: (Value, usize),
        mut value_prefix: Option<(Value, usize)>,
        prefer_existing: bool,
    ) {
        match existing.binary_search_by_key(&path.as_slice(), |entry| entry.path.as_slice()) {
            Ok(index) => {
                // Matching entry already exists, merge with it.
                Self::merge_into(
                    &mut existing[index],
                    value_exact,
                    value_prefix,
                    prefer_existing,
                );
            }
            Err(index) => {
                // Adding a new entry. Go backwards to find its closest parent.
                for parent in existing[0..index].iter_mut().rev() {
                    if common_prefix_length(&parent.path, &path) != parent.path.len() {
                        continue;
                    }

                    // Merge the new value with its parent.
                    if let Some(existing) = &parent.value_prefix {
                        Self::merge_from(
                            existing,
                            &mut value_exact,
                            &mut value_prefix,
                            prefer_existing,
                        );
                    }
                    break;
                }

                // Merge the new value into all its children.
                if let Some(value_prefix) = &value_prefix {
                    for child in &mut existing[index..] {
                        if common_prefix_length(&child.path, &path) != path.len() {
                            break;
                        }

                        Self::merge_into(
                            child,
                            value_prefix.clone(),
                            Some(value_prefix.clone()),
                            true,
                        );
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
        let context = path.num_segments();

        let existing = if host.as_ref().is_empty() {
            &mut self.fallbacks
        } else {
            self.entries.entry(host.as_ref().to_vec()).or_default()
        };

        Self::merge_value(
            existing,
            path,
            (value_exact, context),
            value_prefix.map(|v| (v, context)),
            false,
        );
    }

    /// Translates all rules into a router instance while also merging values if multiple apply to
    /// the same location.
    pub fn build(mut self) -> Router<Value> {
        // Push the fallback routes as defaults for all hosts
        for fallback_entry in &self.fallbacks {
            for entries in self.entries.values_mut() {
                Self::merge_value(
                    entries,
                    fallback_entry.path.clone(),
                    fallback_entry.value_exact.clone(),
                    fallback_entry.value_prefix.clone(),
                    true,
                );
            }
        }

        // Remove unnecessary states
        for entries in self.entries.values_mut() {
            for i in (1..entries.len()).rev() {
                let entry = &entries[i];
                if entry
                    .value_prefix
                    .as_ref()
                    .is_some_and(|value_prefix| value_prefix != &entry.value_exact)
                {
                    // States with different exact and prefix values are never redundant.
                    continue;
                }

                // Walk backwards in the list to find the parent
                let mut redundant = false;
                for parent in entries[0..i].iter().rev() {
                    if common_prefix_length(&parent.path, &entry.path) != parent.path.len() {
                        continue;
                    }

                    // We remove the state if its value matches the parent’s
                    redundant = parent.value_prefix == entry.value_prefix;
                    break;
                }

                if redundant {
                    entries.remove(i);
                }
            }
        }

        // Feed the trie builders
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
        fn lookup(router: &Router<u8>, host: &str, path: &str) -> Option<(u8, String)> {
            let (value, tail) = router.lookup(host, path)?;
            let tail = if let Some(tail) = tail {
                String::from_utf8_lossy(tail.as_ref()).to_string()
            } else {
                path.to_owned()
            };
            Some((*value, tail))
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

    #[test]
    fn merging() {
        fn lookup(router: &Router<String>, host: &str, path: &str) -> Option<(String, String)> {
            let (value, tail) = router.lookup(host, path)?;
            let tail = if let Some(tail) = tail {
                String::from_utf8_lossy(tail.as_ref()).to_string()
            } else {
                path.to_owned()
            };
            Some((value.to_string(), tail))
        }

        struct StringMerge;
        impl Merge<String> for StringMerge {
            fn merge(current: &mut String, new: String) {
                current.push_str(&new);
            }
        }

        let mut builder: RouterBuilder<String, StringMerge> = Default::default();
        builder.push("localhost", "/", "a".to_owned(), Some("a".to_owned()));
        builder.push("localhost", "/abc", "b".to_owned(), None);
        builder.push(
            "localhost",
            "/xyz/aaa",
            "c".to_owned(),
            Some("c".to_owned()),
        );
        builder.push(
            "localhost",
            "/xyz/abc/",
            "d".to_owned(),
            Some("d".to_owned()),
        );
        builder.push("example.com", "/abc/def/", "e".to_owned(), None);
        builder.push("example.com", "/x", "f".to_owned(), Some("f".to_owned()));
        builder.push("", "/abc", "g".to_owned(), Some("g".to_owned()));
        let router = builder.build();

        assert_eq!(
            lookup(&router, "localhost", "/"),
            Some(("a".to_owned(), "/".to_owned()))
        );

        assert_eq!(
            lookup(&router, "localhost", "/abc"),
            Some(("gab".to_owned(), "/".to_owned()))
        );

        assert_eq!(
            lookup(&router, "localhost", "/abc/def"),
            // localhost/* takes priority over /abc/* here, so tail refers to it
            Some(("ga".to_owned(), "/abc/def".to_owned()))
        );

        assert_eq!(
            lookup(&router, "localhost", "/xyz/abc"),
            Some(("ad".to_owned(), "/".to_owned()))
        );

        assert_eq!(
            lookup(&router, "example.com", "/abc/def"),
            Some(("ge".to_owned(), "/".to_owned()))
        );

        assert_eq!(
            lookup(&router, "example.com", "/abc/def/g"),
            Some(("g".to_owned(), "/def/g".to_owned()))
        );
    }
}
