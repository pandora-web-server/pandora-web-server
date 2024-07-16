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

//! Data structures required for `StaticFilesHandler` configuration

use clap::Parser;
use mime_guess::mime::FromStrError;
use mime_guess::Mime;
use pandora_module_utils::{DeserializeMap, OneOrMany};
use serde::Deserialize;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::compression_algorithm::CompressionAlgorithm;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum MimeMatch {
    Exact(Mime),
    Type(String),
    Prefix(String),
    Suffix(String),
}

impl TryFrom<&str> for MimeMatch {
    type Error = FromStrError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(if let Some(prefix) = value.strip_suffix('*') {
            if let Some(type_) = prefix.strip_suffix('/') {
                Self::Type(type_.to_owned())
            } else {
                Self::Prefix(prefix.to_owned())
            }
        } else if let Some(suffix) = value.strip_prefix('*') {
            Self::Suffix(suffix.to_owned())
        } else {
            Self::Exact(value.parse()?)
        })
    }
}

impl TryFrom<String> for MimeMatch {
    type Error = FromStrError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.as_str().try_into()
    }
}

/// Command line options of the static files module
#[derive(Debug, Default, Parser)]
pub struct StaticFilesOpt {
    /// The root directory.
    #[clap(short, long, value_parser = clap::value_parser!(OsString))]
    pub root: Option<PathBuf>,

    /// Redirect /file%2e.txt to /file.txt and /dir to /dir/.
    #[clap(long)]
    pub canonicalize_uri: Option<bool>,

    /// Index file to look for when displaying a directory. This command line flag can be specified
    /// multiple times.
    #[clap(long)]
    pub index_file: Option<Vec<String>>,

    /// URI path of the page to display instead of the default Not Found page, e.g. /404.html
    #[clap(long)]
    pub page_404: Option<String>,

    /// File extension to check when looking for pre-compressed versions of a file. This command
    /// line flag can be specified multiple times. Supported file extensions are gz (gzip),
    /// zz (zlib deflate), z (compress), br (Brotli), zst (Zstandard).
    #[clap(long, value_parser = clap::value_parser!(String))]
    pub precompressed: Option<Vec<CompressionAlgorithm>>,

    /// The character set to declare for text files.
    #[clap(long)]
    pub declare_charset: Option<String>,

    /// MIME type that the `declare_charset` setting should apply to. This command line flag can be
    /// specified multiple times.
    #[clap(long, value_parser = clap::value_parser!(String))]
    pub declare_charset_types: Option<Vec<MimeMatch>>,
}

/// Configuration file settings of the static files module
#[derive(Debug, Clone, PartialEq, Eq, DeserializeMap)]
pub struct StaticFilesConf {
    /// The root directory.
    pub root: Option<PathBuf>,

    /// Redirect /file%2e.txt to /file.txt and /dir to /dir/.
    pub canonicalize_uri: bool,

    /// List of index files to look for in a directory.
    pub index_file: OneOrMany<String>,

    /// URI path of the page to display instead of the default Not Found page, e.g. /404.html
    pub page_404: Option<String>,

    /// List of file extensions to check when looking for pre-compressed versions of a file.
    /// Supported file extensions are gz (gzip), zz (zlib deflate), z (compress), br (Brotli),
    /// zst (Zstandard).
    pub precompressed: OneOrMany<CompressionAlgorithm>,

    /// The character set to declare for text files.
    pub declare_charset: String,

    /// List of MIME types that the `declare_charset` setting should apply to.
    pub declare_charset_types: OneOrMany<MimeMatch>,
}

impl StaticFilesConf {
    /// Merges the command line options into the current configuration. Any command line options
    /// present overwrite existing settings.
    pub fn merge_with_opt(&mut self, opt: StaticFilesOpt) {
        if opt.root.is_some() {
            self.root = opt.root;
        }

        if let Some(canonicalize_uri) = opt.canonicalize_uri {
            self.canonicalize_uri = canonicalize_uri;
        }

        if let Some(index_file) = opt.index_file {
            self.index_file = index_file.into();
        }

        if opt.page_404.is_some() {
            self.page_404 = opt.page_404;
        }

        if let Some(precompressed) = opt.precompressed {
            self.precompressed = precompressed.into();
        }

        if let Some(declare_charset) = opt.declare_charset {
            self.declare_charset = declare_charset;
        }

        if let Some(declare_charset_types) = opt.declare_charset_types {
            self.declare_charset_types = declare_charset_types.into();
        }
    }
}

impl Default for StaticFilesConf {
    fn default() -> Self {
        Self {
            root: None,
            canonicalize_uri: true,
            index_file: Default::default(),
            page_404: None,
            precompressed: Default::default(),
            declare_charset: "utf-8".to_owned(),
            declare_charset_types: Default::default(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use test_log::test;

    #[test]
    fn mime_match_parsing() {
        assert_eq!(
            MimeMatch::try_from("*").unwrap(),
            MimeMatch::Prefix("".to_owned())
        );

        assert_eq!(
            MimeMatch::try_from("text*").unwrap(),
            MimeMatch::Prefix("text".to_owned())
        );

        assert_eq!(
            MimeMatch::try_from("text/*").unwrap(),
            MimeMatch::Type("text".to_owned())
        );

        assert_eq!(
            MimeMatch::try_from("*+xml").unwrap(),
            MimeMatch::Suffix("+xml".to_owned())
        );

        assert_eq!(
            MimeMatch::try_from("text/xml").unwrap(),
            MimeMatch::Exact("text/xml".parse().unwrap())
        );
    }
}
