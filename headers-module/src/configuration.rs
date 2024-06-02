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

//! Structures required to deserialize Headers Module configuration from YAML configuration files.

// https://github.com/rust-lang/rust-clippy/issues/9776
#![allow(clippy::mutable_key_type)]

use http::header::{HeaderName, HeaderValue};
use module_utils::DeserializeMap;
use serde::{
    de::{Deserializer, Error},
    Deserialize,
};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Debug;

/// A single match rule within `match_rules.include` or `match_rules.exclude`
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct MatchRule {
    /// The host name to match
    ///
    /// If empty, this will match all host names. Otherwise an exact match is required.
    pub host: String,

    /// The path to match
    ///
    /// This should be a *normalized* path, without leading or trailing slashes and with exactly
    /// one slash character used as separator. The normalization is normally performed by YAML
    /// deserialization.
    pub path: String,

    /// If `true`, the path will match the exact directory and any files/directories within.
    /// Otherwise an exact match is required.
    pub prefix: bool,
}

impl MatchRule {
    /// Checks whether the rule matches the host/path combination.
    ///
    /// The path given should be normalized (no leading or trailing slashes, exactly one slash
    /// character as separator).
    ///
    /// Matches that only apply to the exact path and none of its children will only be considered
    /// if `allow_exact` is `true`.
    pub(crate) fn matches(&self, host: &str, path: &str, allow_exact: bool) -> bool {
        if !self.host.is_empty() && self.host != host {
            return false;
        }

        if self.prefix {
            path.starts_with(&self.path)
                && (path.len() == self.path.len()
                    || self.path.is_empty()
                    || path.as_bytes()[self.path.len()] == b'/')
        } else {
            allow_exact && self.path == path
        }
    }
}

/// Normalizes a path by removing leading and trailing slashes as well as collapsing multiple
/// separating slashes into one.
fn normalize_path(path: &str) -> String {
    let mut had_slash = true;
    let mut path: String = path
        .chars()
        .filter(|c| {
            if *c == '/' {
                if had_slash {
                    false
                } else {
                    had_slash = true;
                    true
                }
            } else {
                had_slash = false;
                true
            }
        })
        .collect();

    if path.ends_with('/') {
        path.pop();
    }

    path
}

impl<T: AsRef<str>> From<T> for MatchRule {
    fn from(value: T) -> Self {
        let mut rule = value.as_ref().trim();
        let prefix = if let Some(r) = rule.strip_suffix("/*") {
            rule = r;
            true
        } else {
            !rule.contains('/')
        };

        let (host, path) = if rule.starts_with('/') {
            ("", rule)
        } else if let Some((host, path)) = rule.split_once('/') {
            (host, path)
        } else {
            (rule, "")
        };

        Self {
            host: host.to_owned(),
            path: normalize_path(path),
            prefix,
        }
    }
}

impl<'de> Deserialize<'de> for MatchRule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(String::deserialize(deserializer)?.into())
    }
}

impl PartialOrd for MatchRule {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MatchRule {
    fn cmp(&self, other: &Self) -> Ordering {
        let result = self.host.cmp(&other.host);
        if result == Ordering::Equal {
            let result = self.path.cmp(&other.path);
            if result == Ordering::Equal {
                // Prefix matches go before exact matches
                other.prefix.cmp(&self.prefix)
            } else {
                result
            }
        } else {
            result
        }
    }
}

/// Include and exclude rules applying to a configuration entry
///
/// When deciding which rule applies, the “closest” rule to the host/path combination is selected:
///
/// * If a rule like `example.com/dir` applies to this exact host/path combination, that rule is
///   selected.
/// * If a prefix rule like `example.com/dir/*` applies to this host/path combination, it applies
///   if all similar rules match a shorter path.
/// * Fallback rules like `/dir/*` apply only if no host-specific rule matches the host/path
///   combination. When multiple matching fallback rules exist, one is selected using the criteria
///   above.
///
/// The configuration entry is only applied to a host/path configuration if there is a matching
/// rule and that rule is an include rule.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MatchRules {
    /// Rules determining the locations where the configuration entry should apply
    pub include: Vec<MatchRule>,
    /// Rules determining the locations where the configuration entry should not apply
    pub exclude: Vec<MatchRule>,
}

impl MatchRules {
    /// Produces all rules, both include and exclude
    pub(crate) fn iter(&self) -> Box<impl Iterator<Item = &MatchRule>> {
        Box::new(self.include.iter().chain(self.exclude.iter()))
    }

    /// Checks whether the rules match the given host/path combination. If there is a matching rule
    /// and it’s an include rule, that rule is returned.
    ///
    /// The path given should be normalized (no leading or trailing slashes, exactly one slash
    /// character as separator).
    ///
    /// Matches that only apply to the exact path and none of its children will only be considered
    /// if `allow_exact` is `true`.
    pub(crate) fn matches(&self, host: &str, path: &str, allow_exact: bool) -> Option<&MatchRule> {
        fn find_match<'a>(
            rules: &'a [MatchRule],
            host: &str,
            path: &str,
            allow_exact: bool,
        ) -> Option<&'a MatchRule> {
            rules.iter().fold(None, |previous, current| {
                if current.matches(host, path, allow_exact) {
                    if previous.is_some_and(|previous| previous > current) {
                        previous
                    } else {
                        Some(current)
                    }
                } else {
                    previous
                }
            })
        }

        let exclude = find_match(&self.exclude, host, path, allow_exact);
        let include = find_match(&self.include, host, path, allow_exact);
        if let Some(exclude) = exclude {
            if include.is_some_and(|include| include > exclude) {
                include
            } else {
                None
            }
        } else {
            include
        }
    }
}

impl Default for MatchRules {
    fn default() -> Self {
        // By default, match everything
        Self {
            include: vec![MatchRule {
                host: String::new(),
                path: String::new(),
                prefix: true,
            }],
            exclude: Vec::new(),
        }
    }
}

pub(crate) trait Mergeable {
    /// Merges two configurations, with conflicting settings from `other` being prioritized.
    fn merge_with(&mut self, other: Self);
}

pub(crate) type Header = (HeaderName, HeaderValue);

pub(crate) trait IntoHeaders {
    /// Translates the configuration into a list of HTTP headers.
    fn into_headers(self) -> Vec<Header>;
}

/// Combines a given configuration with match rules determining what host/path combinations it
/// should apply to.
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct WithMatchRules<C: Debug + PartialEq + Eq> {
    /// The match rules
    #[serde(flatten, default)]
    pub match_rules: MatchRules,

    /// The actual configuration
    #[serde(flatten)]
    pub conf: C,
}

/// Deserializes a `HeaderName`/`HeaderValue` map.
fn deserialize_headers<'de, D>(
    deserializer: D,
) -> Result<HashMap<HeaderName, HeaderValue>, D::Error>
where
    D: Deserializer<'de>,
{
    let map = HashMap::<String, String>::deserialize(deserializer)?;
    map.into_iter()
        .map(|(name, value)| -> Result<Header, D::Error> {
            Ok((
                HeaderName::try_from(name).map_err(|_| D::Error::custom("Invalid header name"))?,
                HeaderValue::try_from(value)
                    .map_err(|_| D::Error::custom("Invalid header value"))?,
            ))
        })
        .collect()
}

/// Configuration for custom headers
#[derive(Debug, Default, PartialEq, Eq, Clone, Deserialize)]
pub struct CustomHeadersConf {
    /// Map of header names to their respective values
    #[serde(deserialize_with = "deserialize_headers")]
    pub headers: HashMap<HeaderName, HeaderValue>,
}

impl Mergeable for CustomHeadersConf {
    fn merge_with(&mut self, other: Self) {
        self.headers.extend(other.headers);
    }
}

impl IntoHeaders for CustomHeadersConf {
    fn into_headers(self) -> Vec<Header> {
        self.headers.into_iter().collect()
    }
}

/// Configuration file settings of the headers module
#[derive(Debug, Default, PartialEq, Eq, DeserializeMap)]
pub struct HeadersConf {
    /// Custom headers, headers configures as name => value map here
    pub custom_headers: Vec<WithMatchRules<CustomHeadersConf>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_normalization() {
        assert_eq!(normalize_path(""), "");
        assert_eq!(normalize_path("/"), "");
        assert_eq!(normalize_path("///"), "");
        assert_eq!(normalize_path("abc"), "abc");
        assert_eq!(normalize_path("//abc//"), "abc");
        assert_eq!(normalize_path("abc/def"), "abc/def");
        assert_eq!(normalize_path("//abc//def//"), "abc/def");
    }

    #[test]
    fn match_rule_parsing() {
        assert_eq!(
            MatchRule::from(""),
            MatchRule {
                host: "".to_owned(),
                path: "".to_owned(),
                prefix: true,
            }
        );

        assert_eq!(
            MatchRule::from("localhost"),
            MatchRule {
                host: "localhost".to_owned(),
                path: "".to_owned(),
                prefix: true,
            }
        );

        assert_eq!(
            MatchRule::from("localhost/"),
            MatchRule {
                host: "localhost".to_owned(),
                path: "".to_owned(),
                prefix: false,
            }
        );

        assert_eq!(
            MatchRule::from("localhost/*"),
            MatchRule {
                host: "localhost".to_owned(),
                path: "".to_owned(),
                prefix: true,
            }
        );

        assert_eq!(
            MatchRule::from("localhost/dir"),
            MatchRule {
                host: "localhost".to_owned(),
                path: "dir".to_owned(),
                prefix: false,
            }
        );

        assert_eq!(
            MatchRule::from("localhost/dir/*"),
            MatchRule {
                host: "localhost".to_owned(),
                path: "dir".to_owned(),
                prefix: true,
            }
        );

        assert_eq!(
            MatchRule::from("localhost///dir///*"),
            MatchRule {
                host: "localhost".to_owned(),
                path: "dir".to_owned(),
                prefix: true,
            }
        );
    }

    #[test]
    fn match_rule_ordering() {
        // Identical
        assert_eq!(
            MatchRule::from("example.com/dir/*").cmp(&MatchRule::from("example.com//dir//*")),
            Ordering::Equal
        );

        // Host name specificity
        assert_eq!(
            MatchRule::from("example.com/dir/*").cmp(&MatchRule::from("/dir/subdir/*")),
            Ordering::Greater
        );

        // Path length
        assert_eq!(
            MatchRule::from("example.com/dir/*").cmp(&MatchRule::from("example.com/dir/subdir/*")),
            Ordering::Less
        );

        // Exact matches sorted last
        assert_eq!(
            MatchRule::from("example.com/dir/").cmp(&MatchRule::from("example.com/dir/*")),
            Ordering::Greater
        );
    }
}
