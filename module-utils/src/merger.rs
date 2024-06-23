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

//! Rule/configuration merging to be performed prior to creating a router.

use enumset::{EnumSet, EnumSetType};
use serde::Deserialize;
use std::ops::{Deref, DerefMut};
use std::{collections::HashMap, fmt::Debug};

use crate::router::{Path, Router};

/// Result of a path matching operation
#[derive(Debug, EnumSetType)]
pub enum PathMatchFlags {
    /// The match applies to the fallback host
    Fallback,

    /// There is a match for the exact path
    Exact,

    /// There is a match for the files within the path
    Prefix,
}

/// Combination of various flags to be returned from `PathMatch::matches`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathMatchResult {
    inner: EnumSet<PathMatchFlags>,
}

impl PathMatchResult {
    /// A result without any flags set
    pub const EMPTY: Self = Self {
        inner: EnumSet::EMPTY,
    };

    /// Modifies the result to apply to the fallback host
    pub fn set_fallback(self) -> Self {
        Self {
            inner: self.inner | PathMatchFlags::Fallback,
        }
    }

    /// Modifies the result to match the exact path
    pub fn set_exact(self) -> Self {
        Self {
            inner: self.inner | PathMatchFlags::Exact,
        }
    }

    /// Modifies the result to match the files within the path
    pub fn set_prefix(self) -> Self {
        Self {
            inner: self.inner | PathMatchFlags::Prefix,
        }
    }

    /// Checks whether the match applies to the fallback host
    pub fn fallback(&self) -> bool {
        self.contains(PathMatchFlags::Fallback)
    }

    /// Checks whether there is a match
    pub fn any(&self) -> bool {
        self.exact() || self.prefix()
    }

    /// Checks whether there is a match for the exact path
    pub fn exact(&self) -> bool {
        self.contains(PathMatchFlags::Exact)
    }

    /// Checks whether there is a match for the files within the path
    pub fn prefix(&self) -> bool {
        self.contains(PathMatchFlags::Prefix)
    }
}

impl Deref for PathMatchResult {
    type Target = EnumSet<PathMatchFlags>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Encapsulates the logic determining which paths configuration should apply to.
pub trait PathMatch {
    /// Produces all host/path combinations where the result might change, both in positive and
    /// negative direction.
    fn iter(&self) -> Box<dyn Iterator<Item = (&[u8], &Path)> + '_>;

    /// Checks whether the configuration applies to the given path.
    ///
    /// If `force_prefix` is `true`, the check is meant to produce the result for some path
    /// *starting* with `path` but not actually equal to `path`.
    fn matches(&self, host: &[u8], path: &Path, force_prefix: bool) -> PathMatchResult;
}

/// A basic path matcher, applying to a single host/path combination
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(from = "String")]
pub struct HostPathMatcher {
    /// Host name that the matcher applies to
    pub host: Vec<u8>,

    /// Path that the matcher applies to
    pub path: Path,

    /// If `true`, only exact path matches are accepted, otherwise both exact and prefix matches.
    pub exact: bool,
}

impl Debug for HostPathMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.host))?;
        if !self.path.is_empty() || self.exact {
            f.write_str("/")?;
        }
        self.path.fmt(f)?;
        if !self.exact {
            f.write_str("/*")?;
        }
        Ok(())
    }
}

impl From<&str> for HostPathMatcher {
    /// Converts a string like `localhost/subdir/*` into a path matcher. The following input types
    /// are supported:
    ///
    /// * `host`: Applies to any paths within the given host
    /// * `host/path`: Applies to only the given path within the given host
    /// * `host/path/*`: Applies to the given path within the given host and any paths within this
    ///   directory.
    ///
    /// Both `host` and `path` can be empty, the former indicating the fallback host, the latter
    /// the root directory of the host.
    fn from(path: &str) -> Self {
        if path.contains('/') {
            let (path, exact) = if let Some(path) = path.strip_suffix("/*") {
                (path, false)
            } else {
                (path, true)
            };

            let (host, path) = path.split_once('/').unwrap_or((path, ""));
            Self {
                host: host.as_bytes().to_owned(),
                path: Path::new(path),
                exact,
            }
        } else {
            Self {
                host: path.as_bytes().to_owned(),
                path: Path::new(""),
                exact: false,
            }
        }
    }
}

impl From<String> for HostPathMatcher {
    fn from(value: String) -> Self {
        value.as_str().into()
    }
}

impl PathMatch for HostPathMatcher {
    fn iter(&self) -> Box<dyn Iterator<Item = (&[u8], &Path)> + '_> {
        Box::new(std::iter::once((self.host.as_slice(), &self.path)))
    }

    fn matches(&self, host: &[u8], path: &Path, _force_prefix: bool) -> PathMatchResult {
        let result = if self.host == host {
            PathMatchResult::EMPTY
        } else if self.host.is_empty() {
            PathMatchResult::EMPTY.set_fallback()
        } else {
            return PathMatchResult::EMPTY;
        };

        if &self.path == path {
            if self.exact {
                result.set_exact()
            } else {
                result.set_exact().set_prefix()
            }
        } else if !self.exact && self.path.is_prefix_of(path) {
            result.set_prefix()
        } else {
            PathMatchResult::EMPTY
        }
    }
}

/// A basic path matcher, applying to a single path on the empty host
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(from = "String")]
pub struct PathMatcher {
    /// Path that the matcher applies to
    pub path: Path,

    /// If `true`, only exact path matches are accepted, otherwise both exact and prefix matches.
    pub exact: bool,
}

impl Debug for PathMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.path.fmt(f)?;
        if !self.exact {
            f.write_str("/*")?;
        }
        Ok(())
    }
}

impl From<&str> for PathMatcher {
    /// Converts a string like `localhost/subdir/*` into a path matcher. The following input types
    /// are supported:
    ///
    /// * `host`: Applies to any paths within the given host
    /// * `host/path`: Applies to only the given path within the given host
    /// * `host/path/*`: Applies to the given path within the given host and any paths within this
    ///   directory.
    ///
    /// Both `host` and `path` can be empty, the former indicating the fallback host, the latter
    /// the root directory of the host.
    fn from(path: &str) -> Self {
        let (path, exact) = if let Some(path) = path.strip_suffix("/*") {
            (path, false)
        } else {
            (path, true)
        };

        Self {
            path: Path::new(path),
            exact,
        }
    }
}

impl From<String> for PathMatcher {
    fn from(value: String) -> Self {
        value.as_str().into()
    }
}

impl PathMatch for PathMatcher {
    fn iter(&self) -> Box<dyn Iterator<Item = (&[u8], &Path)> + '_> {
        Box::new(std::iter::once(([].as_slice(), &self.path)))
    }

    fn matches(&self, host: &[u8], path: &Path, _force_prefix: bool) -> PathMatchResult {
        let result = PathMatchResult::EMPTY;
        if !host.is_empty() {
            return result;
        }

        if &self.path == path {
            if self.exact {
                result.set_exact()
            } else {
                result.set_exact().set_prefix()
            }
        } else if !self.exact && self.path.is_prefix_of(path) {
            result.set_prefix()
        } else {
            result
        }
    }
}

/// This is almost identical to `HostPathMatcher` but won’t allow prefix rules to match on exact
/// path.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StrictHostPathMatcher {
    host: Vec<u8>,
    path: Path,
    exact: bool,
}
impl PathMatch for StrictHostPathMatcher {
    fn iter(&self) -> Box<dyn Iterator<Item = (&[u8], &Path)> + '_> {
        Box::new(std::iter::once((self.host.as_slice(), &self.path)))
    }

    fn matches(&self, host: &[u8], path: &Path, force_prefix: bool) -> PathMatchResult {
        let result = if self.host == host {
            PathMatchResult::EMPTY
        } else if self.host.is_empty() {
            PathMatchResult::EMPTY.set_fallback()
        } else {
            return PathMatchResult::EMPTY;
        };

        if &self.path == path {
            if self.exact {
                result.set_exact()
            } else if force_prefix {
                result.set_prefix()
            } else {
                PathMatchResult::EMPTY
            }
        } else if !self.exact && self.path.is_prefix_of(path) {
            result.set_prefix()
        } else {
            PathMatchResult::EMPTY
        }
    }
}

impl Debug for StrictHostPathMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("!")?;
        f.write_str(&String::from_utf8_lossy(&self.host))?;
        if !self.path.is_empty() || self.exact {
            f.write_str("/")?;
        }
        self.path.fmt(f)?;
        if !self.exact {
            f.write_str("/*")?;
        }
        Ok(())
    }
}

/// Intermediate node type used by `Merger`
#[derive(Debug, Clone, PartialEq, Eq)]
struct MergerEntry<Matcher, Conf> {
    matcher: Matcher,
    conf: Conf,
}

type MergerEntriesInner<Matcher, Conf> = (
    Path,
    Vec<MergerEntry<Matcher, Conf>>,
    Vec<MergerEntry<Matcher, Conf>>,
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct MergerEntries<Matcher, Conf> {
    inner: Vec<MergerEntriesInner<Matcher, Conf>>,
}

impl<Matcher, Conf> Deref for MergerEntries<Matcher, Conf> {
    type Target = Vec<MergerEntriesInner<Matcher, Conf>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<Matcher, Conf> DerefMut for MergerEntries<Matcher, Conf> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<Matcher, Conf> Default for MergerEntries<Matcher, Conf> {
    fn default() -> Self {
        Self { inner: Vec::new() }
    }
}

/// A type allowing a number of configurations with their specific path-based restrictions to be
/// merged, producing a single configuration for each relevant path.
///
/// ```rust
/// use module_utils::merger::{Merger, HostPathMatcher};
///
/// let mut merger = Merger::new();
/// merger.push(HostPathMatcher::from("localhost"), "a");
/// merger.push(HostPathMatcher::from("localhost/abc/"), "b");
/// merger.push(HostPathMatcher::from("example.com"), "c");
/// merger.push(HostPathMatcher::from(""), "d"); // fallback
///
/// // Merge configurations by joining them
/// let router = merger.merge(|values| values.copied().collect::<String>());
/// assert_eq!(*router.lookup("localhost", "/").unwrap(), "da");
/// assert_eq!(*router.lookup("localhost", "/abc").unwrap(), "dab");
/// assert_eq!(*router.lookup("localhost", "/abc/def").unwrap(), "da");
/// assert_eq!(*router.lookup("example.com", "/abc/def").unwrap(), "dc");
/// ```
///
/// Rather than directly producing a `Router` instance, `merge_into_merger` method can be called to
/// produce an intermediate `Merger`. Multiple mergers of the same type can be combined by calling
/// `extend` and turned into a `Router` instance then.
#[derive(Debug, Clone, Default)]
pub struct Merger<Matcher, Conf> {
    hosts: HashMap<Vec<u8>, MergerEntries<Matcher, Conf>>,
}

impl<Matcher, Conf> Merger<Matcher, Conf>
where
    Matcher: Clone + PathMatch,
    Conf: Clone,
{
    /// Creates a new `Merger` instance.
    pub fn new() -> Self {
        Self {
            hosts: HashMap::new(),
        }
    }

    fn ensure_host(&mut self, host: &[u8]) -> &mut MergerEntries<Matcher, Conf> {
        if !self.hosts.contains_key(host) {
            // Copy fallback host if it exists
            self.hosts.insert(
                host.to_owned(),
                self.hosts
                    .get(&Vec::new())
                    .map(|entries| {
                        let mut new_entries = Vec::new();
                        for (path, list_fallback, list_main) in entries.iter() {
                            new_entries.push((
                                path.clone(),
                                list_fallback.iter().chain(list_main).cloned().collect(),
                                Vec::new(),
                            ));
                        }
                        MergerEntries { inner: new_entries }
                    })
                    .unwrap_or_default(),
            );
        }

        self.hosts.get_mut(host).unwrap()
    }

    fn ensure_entry(entries: &mut MergerEntries<Matcher, Conf>, host: &[u8], path: &Path) {
        let index = match entries.binary_search_by_key(&path, |(path, _, _)| path) {
            Ok(_) => return,
            Err(index) => index,
        };

        let mut list_fallback = Vec::new();
        let mut list_main = Vec::new();

        // Walk backwards in the list to find parent entry
        for (parent_path, parent_fallback, parent_main) in entries[0..index].iter().rev() {
            if parent_path.is_prefix_of(path) {
                // Copy any configurations from parent that apply
                for entry in parent_fallback.iter().chain(parent_main.iter()) {
                    let result = entry.matcher.matches(host, path, false);
                    if result.any() {
                        let list = if result.fallback() {
                            &mut list_fallback
                        } else {
                            &mut list_main
                        };
                        list.push(entry.clone());
                    }
                }
                break;
            }
        }

        entries.insert(index, (path.clone(), list_fallback, list_main));
    }

    /// Adds a configuration to the merging pool, along with the matcher encapsulating its
    /// path-based restrictions.
    pub fn push(&mut self, matcher: Matcher, conf: Conf) {
        // Make sure entries for all relevant host/path combinations exist
        for (host, path) in matcher.iter() {
            Self::ensure_entry(self.ensure_host(host), host, path);

            if host.is_empty() {
                // Fallback entry applies to all hosts, make sure to add entries there.
                for (host, entries) in self.hosts.iter_mut() {
                    if !host.is_empty() {
                        Self::ensure_entry(entries, host, path);
                    }
                }
            }
        }

        // Add this conf to any entries it applies to
        let entry = MergerEntry { matcher, conf };
        for (host, entries) in self.hosts.iter_mut() {
            for (path, list_fallback, list_main) in entries.iter_mut() {
                let result = entry.matcher.matches(host, path, false);
                if result.any() {
                    let list = if result.fallback() {
                        list_fallback
                    } else {
                        list_main
                    };
                    list.push(entry.clone())
                }
            }
        }
    }

    fn merge_entry<C, M>(
        host: &[u8],
        path: &Path,
        list_fallback: Vec<MergerEntry<Matcher, Conf>>,
        list_main: Vec<MergerEntry<Matcher, Conf>>,
        callback: &C,
    ) -> (M, M)
    where
        C: for<'a> Fn(Box<dyn Iterator<Item = &'a Conf> + 'a>) -> M,
        M: Clone,
    {
        // Iterate over fallback entries first, so that regular entries take precedence.
        let value_exact = callback(Box::new(
            list_fallback
                .iter()
                .chain(list_main.iter())
                .filter(|entry| entry.matcher.matches(host, path, false).any())
                .map(|entry| &entry.conf),
        ));
        let value_prefix = callback(Box::new(
            list_fallback
                .iter()
                .chain(list_main.iter())
                .filter(|entry| entry.matcher.matches(host, path, true).prefix())
                .map(|entry| &entry.conf),
        ));
        (value_exact, value_prefix)
    }

    /// Merges the configurations using the given merging callback, producing a router.
    pub fn merge<C, M>(self, callback: C) -> Router<M>
    where
        C: for<'a> Fn(Box<dyn Iterator<Item = &'a Conf> + 'a>) -> M,
        M: Clone + Eq,
    {
        let mut builder = Router::builder();
        for (host, entries) in self.hosts {
            let mut values = Vec::new();
            for (path, list_fallback, list_main) in entries.inner {
                let (value_exact, value_prefix) =
                    Self::merge_entry(&host, &path, list_fallback, list_main, &callback);
                values.push((path, value_exact, value_prefix));
            }

            // Remove unnecessary states
            for i in (0..values.len()).rev() {
                let (path, value_exact, value_prefix) = &values[i];
                if value_exact != value_prefix {
                    // Exact and prefix configurations are different, this state is required
                    continue;
                }

                // Walk backwards to find the parent and compare with its configuration
                let mut redundant = false;
                for (parent_path, _, parent_value_prefix) in values[0..i].iter().rev() {
                    if parent_path.is_prefix_of(path) {
                        redundant = parent_value_prefix == value_prefix;
                        break;
                    }
                }

                if redundant {
                    values.remove(i);
                }
            }

            for (path, value_exact, value_prefix) in values {
                builder.push(&host, path.deref(), value_exact, Some(value_prefix));
            }
        }
        builder.build()
    }

    /// Merges the configurations using the given merging callback and produces a new merger.
    ///
    /// The result can be combined with other mergers of the same type and turned into a router
    /// then.
    ///
    /// *Note*: The resulting merger is not meant for additions of individual items.
    pub fn merge_into_merger<C, M>(self, callback: C) -> Merger<StrictHostPathMatcher, M>
    where
        C: for<'a> Fn(Box<dyn Iterator<Item = &'a Conf> + 'a>) -> M,
        M: Clone,
    {
        let mut new_hosts = HashMap::new();

        for (host, entries) in self.hosts {
            let mut new_entries = MergerEntries::default();
            for (path, list_fallback, list_main) in entries.inner {
                let (value_exact, value_prefix) =
                    Self::merge_entry(&host, &path, list_fallback, list_main, &callback);

                let entry_exact = MergerEntry {
                    matcher: StrictHostPathMatcher {
                        host: host.clone(),
                        path: path.clone(),
                        exact: true,
                    },
                    conf: value_exact,
                };
                let entry_prefix = MergerEntry {
                    matcher: StrictHostPathMatcher {
                        host: host.clone(),
                        path: path.clone(),
                        exact: false,
                    },
                    conf: value_prefix,
                };

                new_entries.push((path, Vec::new(), vec![entry_exact, entry_prefix]));
            }
            new_hosts.insert(host, new_entries);
        }

        Merger { hosts: new_hosts }
    }

    /// Combines the data in the two mergers.
    fn push_merger(&mut self, mut other: Self) {
        // Ensure `other` has all entries present in `self`
        for (host, entries) in &self.hosts {
            let other_entries = other.ensure_host(host);
            for (path, _, _) in entries.iter() {
                Self::ensure_entry(other_entries, host, path);
            }
        }

        // Ensure `self` has all entries present in `other`
        for (host, entries) in &other.hosts {
            let self_entries = self.ensure_host(host);
            for (path, _, _) in entries.iter() {
                Self::ensure_entry(self_entries, host, path);
            }
        }

        // Combine entries
        for (host, other_entries) in other.hosts.into_iter() {
            let self_entries = self.hosts.get_mut(&host).unwrap();
            for (self_entry, other_entry) in
                self_entries.iter_mut().zip(other_entries.inner.into_iter())
            {
                let (_, list_fallback, list_main) = self_entry;
                let (_, other_fallback, other_main) = other_entry;
                list_fallback.extend(other_fallback);
                list_main.extend(other_main);
            }
        }
    }
}

impl<Matcher, Conf> Extend<Merger<Matcher, Conf>> for Merger<Matcher, Conf>
where
    Matcher: Clone + PathMatch,
    Conf: Clone,
{
    fn extend<T: IntoIterator<Item = Merger<Matcher, Conf>>>(&mut self, iter: T) {
        for merger in iter {
            // Calling `self.extend_one` would make sense here but it’s unstable.
            self.push_merger(merger);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(router: &Router<String>, host: &str, path: &str) -> Option<String> {
        router.lookup(host, path).as_deref().cloned()
    }

    #[test]
    fn merge() {
        let mut merger = Merger::<HostPathMatcher, String>::new();
        merger.push("localhost".into(), "a".to_owned());
        merger.push("localhost/abc/".into(), "b".to_owned());
        merger.push("localhost/xyz/aaa/*".into(), "c".to_owned());
        merger.push("localhost/xyz/abc/*".into(), "d".to_owned());
        merger.push("example.com/abc/def/".into(), "e".to_owned());
        merger.push("example.com/x/*".into(), "f".to_owned());
        merger.push("/abc/*".into(), "g".to_owned());
        let router = merger.merge(|values| values.map(String::as_str).collect::<String>());

        assert_eq!(lookup(&router, "localhost", "/"), Some("a".to_owned()));

        assert_eq!(lookup(&router, "localhost", "/abc"), Some("gab".to_owned()));

        assert_eq!(
            lookup(&router, "localhost", "/abc/def"),
            Some("ga".to_owned())
        );

        assert_eq!(
            lookup(&router, "localhost", "/xyz/abc"),
            Some("ad".to_owned())
        );

        assert_eq!(
            lookup(&router, "example.com", "/abc/def"),
            Some("ge".to_owned())
        );

        assert_eq!(
            lookup(&router, "example.com", "/abc/def/g"),
            Some("g".to_owned())
        );
    }

    #[test]
    fn merge_into_merger() {
        let mut merger = Merger::<HostPathMatcher, String>::new();
        merger.push("localhost".into(), "a".to_owned());
        merger.push("localhost/abc/".into(), "b".to_owned());
        merger.push("localhost/xyz/aaa/*".into(), "c".to_owned());
        merger.push("localhost/xyz/abc/*".into(), "d".to_owned());
        merger.push("example.com/abc/def/".into(), "e".to_owned());
        merger.push("example.com/x/*".into(), "f".to_owned());
        merger.push("/abc/*".into(), "g".to_owned());

        let router = merger
            .merge_into_merger(|values| values.map(String::as_str).collect::<String>())
            .merge(|values| values.map(String::as_str).collect::<String>());

        assert_eq!(lookup(&router, "localhost", "/"), Some("a".to_owned()));

        assert_eq!(lookup(&router, "localhost", "/abc"), Some("gab".to_owned()));

        assert_eq!(
            lookup(&router, "localhost", "/abc/def"),
            Some("ga".to_owned())
        );

        assert_eq!(
            lookup(&router, "localhost", "/xyz/abc"),
            Some("ad".to_owned())
        );

        assert_eq!(
            lookup(&router, "example.com", "/abc/def"),
            Some("ge".to_owned())
        );

        assert_eq!(
            lookup(&router, "example.com", "/abc/def/g"),
            Some("g".to_owned())
        );
    }

    #[test]
    fn extend() {
        let mut merger1 = Merger::<HostPathMatcher, String>::new();
        merger1.push("localhost".into(), "a".to_owned());
        merger1.push("localhost/abc/".into(), "b".to_owned());
        merger1.push("localhost/xyz/aaa/*".into(), "c".to_owned());
        merger1.push("localhost/xyz/abc/*".into(), "d".to_owned());
        merger1.push("example.com/abc/def/".into(), "e".to_owned());
        merger1.push("example.com/x/*".into(), "f".to_owned());
        merger1.push("/abc/*".into(), "g".to_owned());

        let mut merger2 = Merger::<HostPathMatcher, String>::new();
        merger2.push("example.net".into(), "h".to_owned());
        merger2.push("example.net/abc/*".into(), "i".to_owned());
        merger2.push("localhost/abc/*".into(), "j".to_owned());
        merger2.push("/*".into(), "k".to_owned());
        merger2.push("/abc".into(), "l".to_owned());
        merger2.push("/abc/def/*".into(), "m".to_owned());

        let mut merger1 =
            merger1.merge_into_merger(|values| values.map(String::as_str).collect::<String>());
        let merger2 =
            merger2.merge_into_merger(|values| values.map(String::as_str).collect::<String>());
        merger1.extend([merger2]);
        let router = merger1.merge(|values| values.map(String::as_str).collect::<String>());

        assert_eq!(lookup(&router, "localhost", "/"), Some("ak".to_owned()));

        assert_eq!(lookup(&router, "localhost", "/x"), Some("ak".to_owned()));

        assert_eq!(
            lookup(&router, "localhost", "/abc"),
            Some("gabklj".to_owned())
        );

        assert_eq!(
            lookup(&router, "localhost", "/abc/x"),
            Some("gakj".to_owned())
        );

        assert_eq!(
            lookup(&router, "localhost", "/xyz/abc/x"),
            Some("adk".to_owned())
        );

        assert_eq!(lookup(&router, "example.com", "/"), Some("k".to_owned()));

        assert_eq!(
            lookup(&router, "example.com", "/abc/def"),
            Some("kmge".to_owned())
        );

        assert_eq!(
            lookup(&router, "example.com", "/abc/def/x"),
            Some("kmg".to_owned())
        );

        assert_eq!(lookup(&router, "example.net", "/"), Some("kh".to_owned()));

        assert_eq!(
            lookup(&router, "example.net", "/abc"),
            Some("gklhi".to_owned())
        );

        assert_eq!(
            lookup(&router, "example.net", "/abc/x"),
            Some("gkhi".to_owned())
        );

        assert_eq!(lookup(&router, "", "/"), Some("k".to_owned()));

        assert_eq!(lookup(&router, "", "/abc"), Some("gkl".to_owned()));

        assert_eq!(lookup(&router, "", "/abc/def"), Some("gkm".to_owned()));

        assert_eq!(lookup(&router, "", "/abc/def/x"), Some("gkm".to_owned()));
    }

    #[test]
    fn redundant_states() {
        #[derive(Debug, Clone)]
        struct CustomMatcher {
            paths: Vec<(Vec<u8>, Path)>,
        }
        impl CustomMatcher {
            fn new() -> Self {
                Self {
                    paths: vec![
                        (b"localhost".to_vec(), Path::new("")),
                        (b"localhost".to_vec(), Path::new("abc")),
                        (b"localhost".to_vec(), Path::new("abc/def")),
                        (b"localhost".to_vec(), Path::new("abc/def/xyz")),
                        (b"example.com".to_vec(), Path::new("")),
                        (b"example.com".to_vec(), Path::new("abc")),
                        (b"example.com".to_vec(), Path::new("abc/def")),
                        (b"example.com".to_vec(), Path::new("abc/def/xyz")),
                    ],
                }
            }
        }
        impl PathMatch for CustomMatcher {
            fn iter(&self) -> Box<dyn Iterator<Item = (&[u8], &Path)> + '_> {
                Box::new(
                    self.paths
                        .iter()
                        .map(|(host, path)| (host.as_slice(), path)),
                )
            }

            fn matches(&self, host: &[u8], path: &Path, _force_prefix: bool) -> PathMatchResult {
                if host == b"localhost" && Path::new("abc/def").is_prefix_of(path) {
                    PathMatchResult::EMPTY.set_prefix()
                } else {
                    PathMatchResult::EMPTY
                }
            }
        }

        let mut merger = Merger::new();
        merger.push(CustomMatcher::new(), "a".to_owned());
        let router = merger.merge(|values| values.map(String::as_str).collect::<String>());

        assert_eq!(lookup(&router, "localhost", "/"), Some("".to_owned()));
        assert_eq!(lookup(&router, "localhost", "/abc"), Some("".to_owned()));
        assert_eq!(
            lookup(&router, "localhost", "/abc/def"),
            Some("a".to_owned())
        );
        assert_eq!(
            lookup(&router, "localhost", "/abc/def/xyz"),
            Some("a".to_owned())
        );
    }

    #[test]
    fn fallback_behavior() {
        #[derive(Debug, Clone)]
        struct CustomMatcher {
            include: HostPathMatcher,
            exclude: HostPathMatcher,
        }

        impl CustomMatcher {
            fn new() -> Self {
                Self {
                    include: "/*".into(),
                    exclude: "example.com/subdir/*".into(),
                }
            }
        }

        impl PathMatch for CustomMatcher {
            fn iter(&self) -> Box<dyn Iterator<Item = (&[u8], &Path)> + '_> {
                Box::new(self.include.iter().chain(self.exclude.iter()))
            }

            fn matches(&self, host: &[u8], path: &Path, force_prefix: bool) -> PathMatchResult {
                let result = self.exclude.matches(host, path, force_prefix);
                if !result.any() {
                    self.include.matches(host, path, force_prefix)
                } else {
                    PathMatchResult::EMPTY
                }
            }
        }

        let mut merger = Merger::new();
        merger.push(CustomMatcher::new(), "match");
        let router = merger.merge(|values| *values.last().unwrap_or(&""));

        assert_eq!(router.lookup("", "").as_deref().copied(), Some("match"));
        assert_eq!(
            router.lookup("", "subdir").as_deref().copied(),
            Some("match")
        );
        assert_eq!(
            router.lookup("example.com", "").as_deref().copied(),
            Some("match")
        );
        assert_eq!(
            router.lookup("example.com", "subdir").as_deref().copied(),
            Some("")
        );
    }
}
