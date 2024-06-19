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

use http::HeaderValue;
use std::collections::HashMap;
use std::fmt::Debug;

use crate::configuration::{Header, IntoHeaders, MatchRule, Mergeable, WithMatchRules};

trait PathPrefixOf {
    fn path_prefix_of(&self, other: &Self) -> bool;
}

impl PathPrefixOf for str {
    fn path_prefix_of(&self, other: &Self) -> bool {
        if other.len() <= self.len() {
            return other == self;
        }

        if self.is_empty() {
            return true;
        }

        other.starts_with(self) && other.as_bytes()[self.len()] == b'/'
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergedConf {
    pub(crate) exact: Vec<Header>,
    pub(crate) prefix: Vec<Header>,
}

pub(crate) trait IntoMergedConf {
    fn into_merged(self) -> HashMap<(String, String), MergedConf>;
}

impl<C> IntoMergedConf for Vec<WithMatchRules<C>>
where
    C: Default + Eq + Clone + Mergeable + IntoHeaders,
{
    fn into_merged(self) -> HashMap<(String, String), MergedConf> {
        let mut configs = HashMap::new();

        // Compile the list of all host names
        let mut hosts = Vec::new();
        for rule in &self {
            for entry in rule.match_rules.iter() {
                if !entry.host.is_empty() && !hosts.contains(&&entry.host) {
                    hosts.push(&entry.host);
                }
            }
        }

        // Add all host/path combinations
        for rule in &self {
            for entry in rule.match_rules.iter() {
                configs.insert(
                    (entry.host.to_owned(), entry.path.to_owned()),
                    (Vec::<(&MatchRule, C)>::new(), Vec::<(&MatchRule, C)>::new()),
                );
                if entry.host.is_empty() {
                    // Default host, this rule applies to all hosts
                    for host in &hosts {
                        configs.insert(
                            ((*host).to_owned(), entry.path.to_owned()),
                            (Vec::<(&MatchRule, C)>::new(), Vec::<(&MatchRule, C)>::new()),
                        );
                    }
                }
            }
        }

        // Add all configuration applying to respective host/path combinations
        for rule in &self {
            for ((host, path), (list_exact, list_prefix)) in configs.iter_mut() {
                if let Some(entry) = rule.match_rules.matches(host, path, true) {
                    list_exact.push((entry, rule.conf.clone()));
                }
                if let Some(entry) = rule.match_rules.matches(host, path, false) {
                    list_prefix.push((entry, rule.conf.clone()));
                }
            }
        }

        // Merge multiple configurations for same host/path combination
        fn merge_list<R: Ord, C: Mergeable + IntoHeaders>(mut list: Vec<(R, C)>) -> Vec<Header> {
            // Make sure more specific rules are applied last and overwrite the headers defined
            // earlier
            list.sort_by(|(r1, _), (r2, _)| r1.cmp(r2));

            let mut iter = list.into_iter();
            if let Some((_, initial)) = iter.next() {
                iter.fold(initial, |mut result, (_, other)| {
                    result.merge_with(other);
                    result
                })
                .into_headers()
            } else {
                Vec::new()
            }
        }

        let mut configs: Vec<_> = configs
            .into_iter()
            .map(|(key, (list_exact, list_prefix))| {
                let mut exact = merge_list(list_exact);
                exact.sort_by(|(n1, v1), (n2, v2)| n1.as_str().cmp(n2.as_str()).then(v1.cmp(v2)));

                let mut prefix = merge_list(list_prefix);
                prefix.sort_by(|(n1, v1), (n2, v2)| n1.as_str().cmp(n2.as_str()).then(v1.cmp(v2)));

                (key, MergedConf { exact, prefix })
            })
            .collect();

        // Remove unnecessary configurations
        configs.sort_by(|(key1, _), (key2, _)| key1.cmp(key2));
        for i in (1..configs.len()).rev() {
            let ((host, path), conf) = &configs[i];
            if !path.is_empty() {
                if conf.exact != conf.prefix {
                    // Different exact and prefix conf, this node is required.
                    continue;
                }

                // Previous entry in the list might be the parent
                let ((parent_host, parent_path), parent_conf) = &configs[i - 1];
                if parent_host == host
                    && parent_path.path_prefix_of(path)
                    && parent_conf.prefix == conf.prefix
                {
                    // Parent's prefix configuration has the same effect as this node, remove.
                } else {
                    continue;
                }

                configs.remove(i);
            } else {
                // TODO? This might be unnecessary if the entire host configuration is identical to
                // the fallback configuration. That should be unlikely however.
            }
        }

        configs.into_iter().collect()
    }
}

fn find_closest_match<'a>(
    keys: &'a [&(String, String)],
    key: &(String, String),
) -> Option<&'a (String, String)> {
    match keys.binary_search(&key) {
        Ok(index) => {
            // Exact match
            return Some(keys[index]);
        }
        Err(mut index) => {
            // Key not found, go backwards in list to find closest same-host match.
            while index > 0 {
                index -= 1;
                let parent = &keys[index];
                if parent.0 != key.0 {
                    // Host mismatch
                    break;
                }

                if parent.1.path_prefix_of(&key.1) {
                    // Found a prefix match
                    return Some(parent);
                }
            }
        }
    }

    if !key.0.is_empty() {
        // Search for closest fallback match
        match keys.binary_search(&&(String::new(), key.1.clone())) {
            Ok(index) => {
                // Exact match
                return Some(keys[index]);
            }
            Err(mut index) => {
                // Key not found, go backwards in list to find closest fallback match. No need to
                // check host any more, it’s only fallback entries at the start of the list.
                while index > 0 {
                    index -= 1;
                    let parent = &keys[index];
                    if parent.1.path_prefix_of(&key.1) {
                        // Found a prefix match
                        return Some(parent);
                    }
                }
            }
        }
    }

    None
}

fn fill_up_keys(
    a: &mut HashMap<(String, String), MergedConf>,
    b: &HashMap<(String, String), MergedConf>,
) {
    let mut present_keys = a.keys().collect::<Vec<_>>();
    present_keys.sort();

    let mut added_entries = Vec::new();
    for key in b.keys() {
        if let Some(parent) = find_closest_match(&present_keys, key) {
            if parent == key {
                // Exact match, nothing to do
                continue;
            }

            // Copy prefix conf
            let conf = &a[parent];
            added_entries.push((
                key.to_owned(),
                MergedConf {
                    exact: conf.prefix.clone(),
                    prefix: conf.prefix.clone(),
                },
            ));
        } else {
            // No parent found, push an empty conf
            added_entries.push((
                key.to_owned(),
                MergedConf {
                    exact: Vec::new(),
                    prefix: Vec::new(),
                },
            ));
        }
    }

    a.extend(added_entries);
}

fn merge_headers(into: &mut Vec<Header>, from: Vec<Header>) {
    for (name, value) in from.into_iter() {
        if let Some(existing) = into.iter().position(|(n, _)| n == name) {
            // Combine duplicate headers
            // https://datatracker.ietf.org/doc/html/rfc7230#section-3.2.2
            let mut new_value = into[existing].1.as_bytes().to_vec();
            new_value.extend_from_slice(b", ");
            new_value.extend_from_slice(value.as_bytes());
            into[existing].1 = HeaderValue::from_bytes(&new_value).unwrap();
        } else {
            into.push((name, value))
        }
    }
}

impl IntoMergedConf
    for (
        HashMap<(String, String), MergedConf>,
        HashMap<(String, String), MergedConf>,
    )
{
    fn into_merged(self) -> HashMap<(String, String), MergedConf> {
        let (mut a, mut b) = self;

        // Make sure that A has all entries present in B and vice versa
        fill_up_keys(&mut a, &b);
        fill_up_keys(&mut b, &a);

        // Merge B into A. No point checking for duplicate headers, it doesn’t matter whether we
        // merge those or the client.
        for (key, value) in b.into_iter() {
            let conf = a.get_mut(&key).unwrap();
            merge_headers(&mut conf.exact, value.exact);
            merge_headers(&mut conf.prefix, value.prefix);
        }

        a
    }
}

#[cfg(test)]
mod tests {
    use http::header::HeaderName;

    use super::*;
    use crate::configuration::MatchRules;

    fn match_rules(
        include: impl AsRef<str>,
        exclude: impl AsRef<str>,
        name: impl TryInto<HeaderName, Error = impl Debug>,
        value: impl TryInto<HeaderValue, Error = impl Debug>,
    ) -> WithMatchRules<HashMap<HeaderName, HeaderValue>> {
        let include = include.as_ref().split(' ').map(MatchRule::from).collect();
        let exclude = exclude.as_ref().split(' ').map(MatchRule::from).collect();

        // https://github.com/rust-lang/rust-clippy/issues/9776
        #[allow(clippy::mutable_key_type)]
        let mut headers = HashMap::new();
        headers.insert(name.try_into().unwrap(), value.try_into().unwrap());
        WithMatchRules {
            match_rules: MatchRules { include, exclude },
            conf: headers,
        }
    }

    fn key(host: impl AsRef<str>, path: impl AsRef<str>) -> (String, String) {
        (host.as_ref().to_owned(), path.as_ref().to_owned())
    }

    fn merged_conf(exact: impl AsRef<str>, prefix: impl AsRef<str>) -> MergedConf {
        fn to_headers(headers: impl AsRef<str>) -> Vec<Header> {
            headers
                .as_ref()
                .split(',')
                .filter(|h| !h.is_empty())
                .map(|h| h.split_once(':').unwrap())
                .map(|(n, v)| (n.trim().try_into().unwrap(), v.trim().try_into().unwrap()))
                .collect()
        }

        let exact = to_headers(exact);
        let prefix = to_headers(prefix);
        MergedConf { exact, prefix }
    }

    #[test]
    fn into_merged() {
        let rules = vec![
            match_rules("/*", "example.com", "X-Test1", "1"),
            match_rules("example.com", "/*", "X-Test2", "2"),
            match_rules("/*", "/test", "X-Test3", "3"),
            match_rules("example.com/test/*", "/test", "X-Test4", "4"),
            match_rules(
                "localhost/ localhost/test/subdir/*",
                "localhost/test",
                "X-Test5",
                "5",
            ),
            match_rules("localhost/test/*", "/test/subdir", "X-Test6", "6"),
            match_rules("localhost:8000", "", "X-Test3", "3"),
        ];

        let merged = rules.into_merged();
        assert_eq!(merged.len(), 8);

        assert_eq!(
            merged[&key("", "")],
            merged_conf("X-Test1: 1, X-Test3: 3", "X-Test1: 1, X-Test3: 3")
        );
        assert_eq!(
            merged[&key("", "test")],
            merged_conf("X-Test1: 1", "X-Test1: 1, X-Test3: 3")
        );
        assert_eq!(
            merged[&key("example.com", "")],
            merged_conf("X-Test2: 2, X-Test3: 3", "X-Test2: 2, X-Test3: 3")
        );
        assert_eq!(
            merged[&key("example.com", "test")],
            merged_conf(
                "X-Test2: 2, X-Test4: 4",
                "X-Test2: 2, X-Test3: 3, X-Test4: 4"
            )
        );
        assert_eq!(
            merged[&key("localhost", "")],
            merged_conf(
                "X-Test1: 1, X-Test3: 3, X-Test5: 5",
                "X-Test1: 1, X-Test3: 3"
            )
        );
        assert_eq!(
            merged[&key("localhost", "test")],
            merged_conf(
                "X-Test1: 1, X-Test6: 6",
                "X-Test1: 1, X-Test3: 3, X-Test6: 6"
            )
        );
        assert_eq!(
            merged[&key("localhost", "test/subdir")],
            merged_conf(
                "X-Test1: 1, X-Test3: 3, X-Test5: 5, X-Test6: 6",
                "X-Test1: 1, X-Test3: 3, X-Test5: 5, X-Test6: 6"
            )
        );
        assert_eq!(
            merged[&key("localhost:8000", "")],
            merged_conf("X-Test1: 1, X-Test3: 3", "X-Test1: 1, X-Test3: 3")
        );
    }
}
