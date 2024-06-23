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
use module_utils::{
    merger::{HostPathMatcher, PathMatch, PathMatchResult},
    router::{Path, EMPTY_PATH},
    DeserializeMap,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Debug;

use crate::deserialize::{
    deserialize_custom_headers, deserialize_match_rule_list, deserialize_with_match_rules,
};

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
    pub include: Vec<HostPathMatcher>,
    /// Rules determining the locations where the configuration entry should not apply
    #[module_utils(deserialize_with_seed = "deserialize_match_rule_list")]
    pub exclude: Vec<HostPathMatcher>,
}

impl PathMatch for MatchRules {
    fn iter(&self) -> Box<dyn Iterator<Item = (&[u8], &Path)> + '_> {
        if self.include.is_empty() && self.exclude.is_empty() {
            Box::new(std::iter::once(("".as_bytes(), EMPTY_PATH)))
        } else {
            Box::new(
                self.include
                    .iter()
                    .chain(self.exclude.iter())
                    .flat_map(|matcher| matcher.iter()),
            )
        }
    }

    fn matches(&self, host: &[u8], path: &Path, force_prefix: bool) -> PathMatchResult {
        fn find_match<'a>(
            rules: &'a [HostPathMatcher],
            host: &[u8],
            path: &Path,
            force_prefix: bool,
        ) -> (PathMatchResult, Option<&'a HostPathMatcher>) {
            rules.iter().fold(
                (PathMatchResult::NoMatch, None),
                |(previous_result, previous), current| {
                    let result = current.matches(host, path, force_prefix);
                    if result.any() {
                        if previous.is_some_and(|previous| previous > current) {
                            (previous_result, previous)
                        } else {
                            (result, Some(current))
                        }
                    } else {
                        (previous_result, previous)
                    }
                },
            )
        }

        if self.include.is_empty() && self.exclude.is_empty() {
            // By default, this is a fallback rule matching everything on fallback host.
            if host.is_empty() {
                return if path.is_empty() {
                    PathMatchResult::MatchesBoth
                } else {
                    PathMatchResult::MatchesPrefix
                };
            } else {
                return PathMatchResult::NoMatch;
            }
        }

        let (_, exclude) = find_match(&self.exclude, host, path, force_prefix);
        let (include_result, include) = find_match(&self.include, host, path, force_prefix);
        if let Some(exclude) = exclude {
            if include.is_some_and(|include| include > exclude) {
                include_result
            } else {
                PathMatchResult::NoMatch
            }
        } else {
            include_result
        }
    }
}

pub(crate) type Header = (HeaderName, HeaderValue);

pub(crate) trait IntoHeaders {
    /// Merges two configurations, with conflicting settings from `other` being prioritized.
    fn merge_with(&mut self, other: &Self);

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
#[module_utils(rename_all = "kebab-case")]
pub struct CacheControlConf {
    /// If set, max-age option will be sent
    pub max_age: Option<usize>,
    /// If set, s-max-age option will be sent
    pub s_maxage: Option<usize>,
    /// If `true`, no-cache flag will be sent
    pub no_cache: bool,
    /// If `true`, no-storage flag will be sent
    pub no_storage: bool,
    /// If `true`, no-transform flag will be sent
    pub no_transform: bool,
    /// If `true`, must-revalidate flag will be sent
    pub must_revalidate: bool,
    /// If `true`, proxy-revalidate flag will be sent
    pub proxy_revalidate: bool,
    /// If `true`, must-understand flag will be sent
    pub must_understand: bool,
    /// If `true`, private flag will be sent
    pub private: bool,
    /// If `true`, public flag will be sent
    pub public: bool,
    /// If `true`, immutable flag will be sent
    pub immutable: bool,
    /// If set, stale-while-revalidate option will be sent
    pub stale_while_revalidate: Option<usize>,
    /// If set, stale-if-error option will be sent
    pub stale_if_error: Option<usize>,
}

impl IntoHeaders for CacheControlConf {
    fn merge_with(&mut self, other: &Self) {
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

impl IntoHeaders for HashMap<HeaderName, HeaderValue> {
    fn merge_with(&mut self, other: &Self) {
        self.extend(
            other
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }

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
