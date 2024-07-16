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

//! Matching MIME types against a list

use mime_guess::Mime;
use std::collections::HashSet;

use crate::configuration::MimeMatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MimeMatcher {
    exact: HashSet<Mime>,
    type_: HashSet<String>,
    prefix: Vec<String>,
    suffix: Vec<String>,
}

impl MimeMatcher {
    pub(crate) fn new() -> Self {
        Self {
            exact: HashSet::new(),
            type_: HashSet::new(),
            prefix: Vec::new(),
            suffix: Vec::new(),
        }
    }

    pub(crate) fn add(&mut self, mime: MimeMatch) {
        match mime {
            MimeMatch::Exact(mime) => {
                self.exact.insert(mime);
            }
            MimeMatch::Type(type_) => {
                self.type_.insert(type_);
            }
            MimeMatch::Prefix(prefix) => self.prefix.push(prefix),
            MimeMatch::Suffix(suffix) => self.suffix.push(suffix),
        }
    }

    pub(crate) fn matches(&self, mime: &Mime) -> bool {
        self.exact.contains(mime)
            || self.type_.contains(mime.type_().as_str())
            || self
                .prefix
                .iter()
                .any(|prefix| mime.as_ref().starts_with(prefix))
            || self
                .suffix
                .iter()
                .any(|suffix| mime.as_ref().ends_with(suffix))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use test_log::test;

    #[test]
    fn matching() {
        let mut matcher = MimeMatcher::new();
        matcher.add(MimeMatch::try_from("text/*").unwrap());
        matcher.add(MimeMatch::try_from("application/javascript").unwrap());
        matcher.add(MimeMatch::try_from("*+xml").unwrap());
        matcher.add(MimeMatch::try_from("dummy*").unwrap());

        assert!(matcher.matches(&"text/html".parse().unwrap()));
        assert!(matcher.matches(&"text/xml".parse().unwrap()));
        assert!(!matcher.matches(&"text2/xml".parse().unwrap()));
        assert!(matcher.matches(&"text2/anything+xml".parse().unwrap()));
        assert!(matcher.matches(&"application/javascript".parse().unwrap()));
        assert!(!matcher.matches(&"application/javascript+png".parse().unwrap()));
        assert!(matcher.matches(&"dummy/html".parse().unwrap()));
        assert!(matcher.matches(&"dummys/html".parse().unwrap()));
        assert!(!matcher.matches(&"application/dummy".parse().unwrap()));
    }
}
