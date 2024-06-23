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
use module_utils::merger::{HostPathMatcher, PathMatch, PathMatchResult};
use module_utils::router::{Path, EMPTY_PATH};
use module_utils::DeserializeMap;
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
                (PathMatchResult::EMPTY, None),
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
            // By default, this is a fallback rule matching everything
            let result = if host.is_empty() {
                PathMatchResult::EMPTY
            } else {
                PathMatchResult::EMPTY.set_fallback()
            };

            return if path.is_empty() {
                result.set_exact().set_prefix()
            } else {
                result.set_prefix()
            };
        }

        let (_, exclude) = find_match(&self.exclude, host, path, force_prefix);
        let (include_result, include) = find_match(&self.include, host, path, force_prefix);
        if let Some(exclude) = exclude {
            if include.is_some_and(|include| include > exclude) {
                include_result
            } else {
                PathMatchResult::EMPTY
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

macro_rules! impl_cache_control_conf {
    ($vis:vis struct $struct_name:ident { $($name:ident($header_name:literal, $($type:tt)+),)* }) => {
        /// Configuration for the Cache-Control header
        #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
        #[allow(missing_debug_implementations)]
            $vis struct $struct_name {
            $(
                #[doc = impl_cache_control_conf!(doc($header_name, $($type)+))]
                #[module_utils(rename = $header_name)]
                pub $name: $($type)+,
            )*
        }

        impl IntoHeaders for $struct_name {
            fn merge_with(&mut self, other: &Self) {
                $(
                    impl_cache_control_conf!(merge(self.$name, other.$name, $($type)+));
                )*
            }
            fn into_headers(self) -> Vec<Header> {
                let mut entries: Vec<Cow<'_, str>> = Vec::new();
                $(
                    impl_cache_control_conf!(push(entries, $header_name, self.$name, $($type)+));
                )*
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
    };
    (doc($header_name:literal, Option<usize>)) => {
        concat!("If set, ", $header_name, " option will be sent")
    };
    (doc($header_name:literal, bool)) => {
        concat!("If `true`, ", $header_name, " flag will be sent")
    };
    (merge($into:expr, $from:expr, Option<usize>)) => {
        if $from.is_some() {
            $into = $from;
        }
    };
    (merge($into:expr, $from:expr, bool)) => {
        if $from {
            $into = $from;
        }
    };
    (push($list:expr, $header_name:literal, $value:expr, Option<usize>)) => {
        if let Some(value) = $value {
            $list.push(format!(concat!($header_name, "={}"), value).into());
        }
    };
    (push($list:expr, $header_name:literal, $value:expr, bool)) => {
        if $value {
            $list.push($header_name.into());
        }
    };
}

impl_cache_control_conf! {
    pub struct CacheControlConf {
        max_age("max-age", Option<usize>),
        s_maxage("s-maxage", Option<usize>),
        no_cache("no-cache", bool),
        no_storage("no-storage", bool),
        no_transform("no-transform", bool),
        must_revalidate("must-revalidate", bool),
        proxy_revalidate("proxy-revalidate", bool),
        must_understand("must-understand", bool),
        private("private", bool),
        public("public", bool),
        immutable("immutable", bool),
        stale_while_revalidate("stale-while-revalidate", Option<usize>),
        stale_if_error("stale-if-error", Option<usize>),
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
