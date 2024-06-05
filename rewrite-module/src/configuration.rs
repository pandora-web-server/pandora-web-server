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

//! Structures required to deserialize Rewrite Module configuration from YAML configuration files.

use module_utils::DeserializeMap;
use regex::Regex;
use serde::{
    de::{Deserializer, Error},
    Deserialize,
};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::default::Default;

/// A parsed representation of the `from` field of the rewrite rule
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PathMatch {
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

impl PathMatch {
    /// Checks whether a particular path is matched
    ///
    /// Note: The `path` parameter should be a *normalized* path, without leading or trailing
    /// slashes and with exactly one slash character used as separator.
    pub(crate) fn matches(&self, path: &str) -> bool {
        if self.prefix {
            path.starts_with(&self.path)
                && (path.len() == self.path.len()
                    || self.path.is_empty()
                    || path.as_bytes()[self.path.len()] == b'/')
        } else {
            self.path == path
        }
    }
}

impl<'a> From<Cow<'a, str>> for PathMatch {
    fn from(value: Cow<'a, str>) -> Self {
        let (path, prefix) = if let Some(path) = value.strip_suffix("/*") {
            (Cow::from(path), true)
        } else {
            (value, false)
        };
        Self {
            path: normalize_path(&path),
            prefix,
        }
    }
}

impl From<String> for PathMatch {
    fn from(value: String) -> Self {
        Cow::from(value).into()
    }
}

impl<'a> From<&'a str> for PathMatch {
    fn from(value: &'a str) -> Self {
        Cow::from(value).into()
    }
}

impl<'de> Deserialize<'de> for PathMatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(String::deserialize(deserializer)?.into())
    }
}

impl Ord for PathMatch {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then(other.prefix.cmp(&self.prefix))
    }
}

impl PartialOrd for PathMatch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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

#[derive(Debug, PartialEq, Eq, Clone)]
enum VariableInterpolationPart {
    Literal(Vec<u8>),
    Variable(String),
}

/// Parsed representation of a string with variable interpolation like the `to` field of the
/// rewrite rule
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct VariableInterpolation {
    parts: Vec<VariableInterpolationPart>,
}

impl From<&str> for VariableInterpolation {
    fn from(mut value: &str) -> Self {
        trait FindAt {
            fn find_at(&self, pattern: &str, start: usize) -> Option<usize>;
        }
        impl FindAt for str {
            fn find_at(&self, pattern: &str, start: usize) -> Option<usize> {
                self[start..].find(pattern).map(|index| index + start)
            }
        }

        let mut parts = Vec::new();
        while !value.is_empty() {
            let mut search_start = 0;
            loop {
                let variable_start = value.find_at(Self::VARIABLE_PREFIX, search_start);
                let variable_end =
                    variable_start.and_then(|start| value.find_at(Self::VARIABLE_SUFFIX, start));

                if let (Some(start), Some(end)) = (variable_start, variable_end) {
                    // Found variable start and end, check whether name is alphanumeric
                    let name = &value[start + Self::VARIABLE_PREFIX.len()..end];
                    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        if start > 0 {
                            parts.push(VariableInterpolationPart::Literal(
                                value[0..start].as_bytes().to_vec(),
                            ));
                        }
                        parts.push(VariableInterpolationPart::Variable(name.to_owned()));
                        value = &value[end + Self::VARIABLE_SUFFIX.len()..];
                        break;
                    }

                    // This variable name is invalid, look for another variable start further ahead
                    search_start = start + Self::VARIABLE_PREFIX.len();
                } else {
                    // No variable found, take the entire value as literal
                    parts.push(VariableInterpolationPart::Literal(
                        value.as_bytes().to_vec(),
                    ));
                    value = "";
                    break;
                }
            }
        }
        Self { parts }
    }
}

impl<'de> Deserialize<'de> for VariableInterpolation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(String::deserialize(deserializer)?.as_str().into())
    }
}

impl VariableInterpolation {
    const VARIABLE_PREFIX: &'static str = "${";
    const VARIABLE_SUFFIX: &'static str = "}";

    pub(crate) fn interpolate<'a, L>(&self, lookup: L) -> Vec<u8>
    where
        L: Fn(&str) -> Option<&'a [u8]>,
    {
        let mut result = Vec::new();
        for part in &self.parts {
            match &part {
                VariableInterpolationPart::Literal(value) => result.extend_from_slice(value),
                VariableInterpolationPart::Variable(name) => {
                    if let Some(value) = lookup(name) {
                        result.extend_from_slice(value);
                    } else {
                        result.extend_from_slice(Self::VARIABLE_PREFIX.as_bytes());
                        result.extend_from_slice(name.as_bytes());
                        result.extend_from_slice(Self::VARIABLE_SUFFIX.as_bytes());
                    }
                }
            }
        }
        result
    }
}

/// URI rewriting type
#[derive(Debug, PartialEq, Eq, Deserialize, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub enum RewriteType {
    /// An internal rewrite, URI change for internal processing only
    Internal,
    /// A 307 Temporary Redirect response
    Redirect,
    /// A 308 Permanent Redirect response
    Permanent,
}

/// A parsed representation of a field like `from_regex` of the rewrite rule
#[derive(Debug, Clone)]
pub struct RegexMatch {
    /// Regular expression to apply to the value
    pub regex: Regex,
    /// If `true`, the result should be negated
    pub negate: bool,
}

impl RegexMatch {
    /// Checks whether the given value is matched
    pub(crate) fn matches(&self, value: &str) -> bool {
        let result = self.regex.is_match(value);
        if self.negate {
            !result
        } else {
            result
        }
    }
}

impl PartialEq for RegexMatch {
    fn eq(&self, other: &Self) -> bool {
        self.regex.as_str() == other.regex.as_str() && self.negate == other.negate
    }
}

impl Eq for RegexMatch {}

impl TryFrom<&str> for RegexMatch {
    type Error = regex::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let (regex, negate) = if let Some(regex) = value.strip_prefix('!') {
            (regex, true)
        } else {
            (value, false)
        };
        Ok(Self {
            regex: Regex::new(regex)?,
            negate,
        })
    }
}

impl<'de> Deserialize<'de> for RegexMatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .as_str()
            .try_into()
            .map_err(D::Error::custom)
    }
}

/// A rewrite rule resulting in either request URI change or redirect
#[derive(Debug, PartialEq, Eq, DeserializeMap)]
pub struct RewriteRule {
    /// Path or a set of paths to rewrite
    ///
    /// By default, an exact path match is required. A value like `/path/*` indicates a prefix
    /// match, both `/path/` and `/path/subdir/file.txt` will be matched.
    ///
    /// When multiple rules potentially apply to a location, the closest matches will be evaluated
    /// first meaning. Rules with a longer path are considered closer matches than shorter paths.
    /// Exact matches are considered closer matches than prefix matches for the same path.
    pub from: PathMatch,

    /// Additional regular expression to further restrict matching paths, e.g. `\.png$` to match
    /// only PNG files. Prefixing the regular expression with `!` will negate its effect, e.g.
    /// `!\.png` will match all files but PNG files.
    ///
    /// Note that restricting the path as much as possible via `from` setting first is recommended
    /// for reasons of performance.
    pub from_regex: Option<RegexMatch>,

    /// Additional regular expression to restrict matches to particular query strings only. For
    /// example `file=` will only match queries containing a `file` parameter. Prefixing the
    /// regular expression with `!` will negate its effect, e.g. `!file=` will match all queries
    /// but those containing a `file` parameter.
    pub query_regex: Option<RegexMatch>,

    /// New URI to be set on match
    ///
    /// The following variables will be resolved:
    ///
    /// * `${tail}`: Only valid when matching a path prefix. This will be replaced by the path part
    ///   matched by `*`. For example, if `from` is `/dir/*`, `to` is `/another/${tail}` and the
    ///   actual path matched is `/dir/file.txt`, then the URI will be rewritten into
    ///   `/another/file.txt`.
    /// * `${query}`: This allows considering the original query which is removed by default. For
    ///   example, if `from` is `/file.txt` and `to` is `/file.html?${query}` then a request to
    ///   `/file.txt?a=b` will be rewritten into `/file.html?a=b`.
    /// * `${http_<header>}`: This allows inserting arbitrary HTTP headers into the redirect
    ///   target.
    pub to: VariableInterpolation,

    /// Rewriting type, one of `internal` (default), `redirect` or `permanent`
    pub r#type: RewriteType,
}

impl Default for RewriteRule {
    fn default() -> Self {
        Self {
            from: "/*".into(),
            from_regex: None,
            query_regex: None,
            to: "/".into(),
            r#type: RewriteType::Internal,
        }
    }
}

/// Configuration file settings of the rewrite module
#[derive(Debug, Default, PartialEq, Eq, DeserializeMap)]
pub struct RewriteConf {
    /// A list of rewrite rules
    pub rewrite_rules: Vec<RewriteRule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use test_log::test;

    #[test]
    fn path_match() {
        let path_match = PathMatch::from("/abc");
        assert!(path_match.matches("abc"));
        assert!(!path_match.matches("abcdef"));
        assert!(!path_match.matches("abc/xyz"));

        let path_match = PathMatch::from("/abc/*");
        assert!(path_match.matches("abc"));
        assert!(!path_match.matches("abcdef"));
        assert!(path_match.matches("abc/xyz"));
        assert!(path_match.matches("abc/xyz/file.txt"));

        let path_match = PathMatch::from("//abc//xyz//*");
        assert!(path_match.matches("abc/xyz"));
        assert!(!path_match.matches("abcd/xyz"));
        assert!(path_match.matches("abc/xyz/file.txt"));
    }

    #[test]
    fn variable_interpolation() {
        assert_eq!(
            VariableInterpolation::from("abcd").interpolate(|_| panic!("Unexpected lookup call")),
            b"abcd".to_vec()
        );

        assert_eq!(
            VariableInterpolation::from("ab${xyz}cd").interpolate(|_| None),
            b"ab${xyz}cd".to_vec()
        );

        assert_eq!(
            VariableInterpolation::from("ab${xyz}cd").interpolate(|name| {
                if name == "xyz" {
                    Some(b"resolved")
                } else {
                    None
                }
            }),
            b"abresolvedcd".to_vec()
        );

        assert_eq!(
            VariableInterpolation::from("a${x}${y}bc${z}d").interpolate(|name| {
                if name == "x" {
                    Some(b"x resolved")
                } else if name == "z" {
                    Some(b"z resolved")
                } else {
                    None
                }
            }),
            b"ax resolved${y}bcz resolvedd".to_vec()
        );

        assert_eq!(
            VariableInterpolation::from("${a${x}").interpolate(|name| {
                if name == "x" {
                    Some(b"resolved")
                } else {
                    None
                }
            }),
            b"${aresolved".to_vec()
        );
    }

    #[test]
    fn regex_match() {
        let regex_match = RegexMatch::try_from("abc").unwrap();
        assert!(regex_match.matches("abc"));
        assert!(regex_match.matches("aabcc"));
        assert!(!regex_match.matches("ab"));
        assert!(!regex_match.matches("bc"));

        let regex_match = RegexMatch::try_from("^abc$").unwrap();
        assert!(regex_match.matches("abc"));
        assert!(!regex_match.matches("aabcc"));
        assert!(!regex_match.matches("ab"));
        assert!(!regex_match.matches("bc"));

        let regex_match = RegexMatch::try_from("!abc").unwrap();
        assert!(!regex_match.matches("abc"));
        assert!(!regex_match.matches("aabcc"));
        assert!(regex_match.matches("ab"));
        assert!(regex_match.matches("bc"));

        let regex_match = RegexMatch::try_from("!^abc$").unwrap();
        assert!(!regex_match.matches("abc"));
        assert!(regex_match.matches("aabcc"));
        assert!(regex_match.matches("ab"));
        assert!(regex_match.matches("bc"));
    }
}
