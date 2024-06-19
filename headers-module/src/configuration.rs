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

use http::{
    header,
    header::{HeaderName, HeaderValue},
};
use module_utils::DeserializeMap;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Debug;

use crate::deserialize::{
    deserialize_custom_headers, deserialize_match_rule_list, deserialize_with_match_rules,
};

/// A single match rule within `match_rules.include` or `match_rules.exclude`
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct MatchRules {
    /// Rules determining the locations where the configuration entry should apply
    #[module_utils(deserialize_with_seed = "deserialize_match_rule_list")]
    pub include: Vec<MatchRule>,
    /// Rules determining the locations where the configuration entry should not apply
    #[module_utils(deserialize_with_seed = "deserialize_match_rule_list")]
    pub exclude: Vec<MatchRule>,
}

impl MatchRules {
    const RULE_DEFAULT: &'static MatchRule = &MatchRule {
        host: String::new(),
        path: String::new(),
        prefix: true,
    };

    /// Produces all rules, both include and exclude
    pub(crate) fn iter(&self) -> Box<dyn Iterator<Item = &MatchRule> + '_> {
        if self.include.is_empty() && self.exclude.is_empty() {
            Box::new(std::iter::once(Self::RULE_DEFAULT))
        } else {
            Box::new(self.include.iter().chain(self.exclude.iter()))
        }
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

        if self.include.is_empty() && self.exclude.is_empty() {
            // Match everything by default
            return Some(Self::RULE_DEFAULT);
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithMatchRules<C: Clone + PartialEq + Eq> {
    /// The match rules
    pub match_rules: MatchRules,

    /// The actual configuration
    pub conf: C,
}

/// Configuration for the Cache-Control header
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct CacheControlConf {
    /// If set, max-age option will be sent
    #[module_utils(rename = "max-age")]
    max_age: Option<usize>,
    /// If set, s-max-age option will be sent
    #[module_utils(rename = "s-maxage")]
    s_maxage: Option<usize>,
    /// If `true`, no-cache flag will be sent
    #[module_utils(rename = "no-cache")]
    no_cache: bool,
    /// If `true`, no-storage flag will be sent
    #[module_utils(rename = "no-storage")]
    no_storage: bool,
    /// If `true`, no-transform flag will be sent
    #[module_utils(rename = "no-transform")]
    no_transform: bool,
    /// If `true`, must-revalidate flag will be sent
    #[module_utils(rename = "must-revalidate")]
    must_revalidate: bool,
    /// If `true`, proxy-revalidate flag will be sent
    #[module_utils(rename = "proxy-revalidate")]
    proxy_revalidate: bool,
    /// If `true`, must-understand flag will be sent
    #[module_utils(rename = "must-understand")]
    must_understand: bool,
    /// If `true`, private flag will be sent
    private: bool,
    /// If `true`, public flag will be sent
    public: bool,
    /// If `true`, immutable flag will be sent
    immutable: bool,
    /// If set, stale-while-revalidate option will be sent
    #[module_utils(rename = "stale-while-revalidate")]
    stale_while_revalidate: Option<usize>,
    /// If set, stale-if-error option will be sent
    #[module_utils(rename = "stale-if-error")]
    stale_if_error: Option<usize>,
}

impl Mergeable for CacheControlConf {
    fn merge_with(&mut self, other: Self) {
        macro_rules! merge_option {
            ($name: ident) => {
                if other.$name.is_some() {
                    self.$name = other.$name;
                }
            };
        }
        macro_rules! merge_bool {
            ($name: ident) => {
                if other.$name {
                    self.$name = other.$name;
                }
            };
        }

        merge_option!(max_age);
        merge_option!(s_maxage);
        merge_bool!(no_cache);
        merge_bool!(no_storage);
        merge_bool!(no_transform);
        merge_bool!(must_revalidate);
        merge_bool!(proxy_revalidate);
        merge_bool!(must_understand);
        merge_bool!(private);
        merge_bool!(public);
        merge_bool!(immutable);
        merge_option!(stale_while_revalidate);
        merge_option!(stale_if_error);
    }
}

impl IntoHeaders for CacheControlConf {
    fn into_headers(self) -> Vec<Header> {
        let mut entries: Vec<Cow<'_, str>> = Vec::new();
        if let Some(max_age) = self.max_age {
            entries.push(format!("max-age={max_age}").into());
        }
        if let Some(s_maxage) = self.s_maxage {
            entries.push(format!("s-maxage={s_maxage}").into());
        }
        if self.no_cache {
            entries.push("no-cache".into());
        }
        if self.no_storage {
            entries.push("no-storage".into());
        }
        if self.no_transform {
            entries.push("no-transform".into());
        }
        if self.must_revalidate {
            entries.push("must-revalidate".into());
        }
        if self.proxy_revalidate {
            entries.push("proxy-revalidate".into());
        }
        if self.must_understand {
            entries.push("must-understand".into());
        }
        if self.private {
            entries.push("private".into());
        }
        if self.public {
            entries.push("public".into());
        }
        if self.immutable {
            entries.push("immutable".into());
        }

        if entries.is_empty() {
            Vec::new()
        } else {
            vec![(
                header::CACHE_CONTROL,
                HeaderValue::from_str(&entries.join(", ")).unwrap(),
            )]
        }
    }
}

impl Mergeable for HashMap<HeaderName, HeaderValue> {
    fn merge_with(&mut self, other: Self) {
        self.extend(other);
    }
}

impl IntoHeaders for HashMap<HeaderName, HeaderValue> {
    fn into_headers(self) -> Vec<Header> {
        self.into_iter().collect()
    }
}

/// Various settings to configure HTTP response headers
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct HeadersInnerConf {
    /// Cache-Control header
    #[module_utils(deserialize_with_seed = "deserialize_with_match_rules")]
    pub cache_control: Vec<WithMatchRules<CacheControlConf>>,

    /// Custom headers, headers configures as name => value map here
    #[module_utils(deserialize_with_seed = "deserialize_custom_headers")]
    pub custom: Vec<WithMatchRules<HashMap<HeaderName, HeaderValue>>>,
}

/// Configuration file settings of the headers module
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct HeadersConf {
    /// Various settings to configure HTTP response headers
    pub response_headers: HeadersInnerConf,
}

#[cfg(test)]
mod tests {
    use super::*;

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
