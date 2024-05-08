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

//! Handles various compression algorithms allowed in `Accept-Encoding` and `Content-Encoding` HTTP
//! headers.

use serde::Deserialize;
use std::fmt::Display;
use std::str::FromStr;

/// Represents a compression algorithm choice.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
pub enum CompressionAlgorithm {
    /// gzip compression
    #[serde(rename = "gz")]
    Gzip,
    /// deflate (zlib) compression
    #[serde(rename = "zz")]
    Deflate,
    /// compress compression
    #[serde(rename = "z")]
    Compress,
    /// Brotli compression
    #[serde(rename = "br")]
    Brotli,
    /// Zstandard compression
    #[serde(rename = "zst")]
    Zstandard,
}

impl CompressionAlgorithm {
    /// Returns the file extension corresponding to the algorithm.
    pub fn ext(&self) -> &'static str {
        match self {
            Self::Gzip => "gz",
            Self::Deflate => "zz",
            Self::Compress => "z",
            Self::Brotli => "br",
            Self::Zstandard => "zst",
        }
    }

    /// Determines the algorithm corresponding to the file extension if any.
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "gz" => Some(Self::Gzip),
            "zz" => Some(Self::Deflate),
            "z" => Some(Self::Compress),
            "br" => Some(Self::Brotli),
            "zst" => Some(Self::Zstandard),
            _ => None,
        }
    }

    /// Returns the algorithm name as used in `Accept-Encoding` HTTP header.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Deflate => "deflate",
            Self::Compress => "compress",
            Self::Brotli => "br",
            Self::Zstandard => "zstd",
        }
    }

    /// Determines the algorithm corresponding to a name from `Accept-Encoding` HTTP header.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "gzip" => Some(Self::Gzip),
            "deflate" => Some(Self::Deflate),
            "compress" => Some(Self::Compress),
            "br" => Some(Self::Brotli),
            "zstd" => Some(Self::Zstandard),
            _ => None,
        }
    }
}

impl FromStr for CompressionAlgorithm {
    type Err = UnsupportedCompressionAlgorithm;

    /// Coverts a file extension into a compression algorithm.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CompressionAlgorithm::from_ext(s).ok_or(UnsupportedCompressionAlgorithm(s.to_owned()))
    }
}

impl Display for CompressionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.name())
    }
}

/// The error type returned by `CompressionAlgorithm::from_str()`
#[derive(Debug, PartialEq, Eq)]
pub struct UnsupportedCompressionAlgorithm(String);

impl Display for UnsupportedCompressionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "Unsupported compression algorithm: {}", self.0)
    }
}

/// Parses an encoding specifier from `Accept-Encoding` HTTP header into an
/// algorithm/quality pair.
fn parse_encoding(encoding: &str) -> Option<(&str, u16)> {
    let mut params = encoding.split(';');
    let algorithm = params.next()?.trim();
    let mut quality = 1000;
    for param in params {
        if let Some((name, value)) = param.split_once('=') {
            if name.trim() == "q" {
                if let Ok(value) = f64::from_str(value.trim()) {
                    quality = (value * 1000.0) as u16;
                }
            }
        }
    }
    Some((algorithm, quality))
}

/// Compares the requested encodings from `Accept-Encoding` HTTP header with a list of supported
/// algorithms and returns any matches, sorted by the respective quality value.
pub(crate) fn find_matches(
    requested: &str,
    supported: &[CompressionAlgorithm],
) -> Vec<CompressionAlgorithm> {
    let mut requested = requested
        .split(',')
        .filter_map(parse_encoding)
        .collect::<Vec<_>>();
    requested.sort_by_key(|(_, quality)| -(*quality as i32));

    let mut result = Vec::new();
    for (algorithm, _) in requested {
        if algorithm == "*" {
            for algorithm in supported {
                if !result.contains(algorithm) {
                    result.push(*algorithm);
                }
            }
            break;
        } else if let Some(algorithm) = CompressionAlgorithm::from_name(algorithm) {
            if supported.contains(&algorithm) && !result.contains(&algorithm) {
                result.push(algorithm);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matches() {
        assert_eq!(
            find_matches(
                "",
                &[CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli]
            ),
            Vec::new()
        );

        assert_eq!(
            find_matches(
                "identity",
                &[CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli]
            ),
            Vec::new()
        );

        assert_eq!(
            find_matches(
                "*",
                &[CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli]
            ),
            vec![CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli]
        );

        assert_eq!(
            find_matches(
                "br, *",
                &[CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli]
            ),
            vec![CompressionAlgorithm::Brotli, CompressionAlgorithm::Gzip]
        );

        assert_eq!(
            find_matches(
                "br;q=0.9, *",
                &[CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli]
            ),
            vec![CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli]
        );

        assert_eq!(
            find_matches(
                "deflate;q=0.7, gzip;q=0.9, zstd;q=0.8, br;q=1.0, compress;q=0.5",
                &[
                    CompressionAlgorithm::Deflate,
                    CompressionAlgorithm::Gzip,
                    CompressionAlgorithm::Compress,
                    CompressionAlgorithm::Brotli,
                    CompressionAlgorithm::Zstandard,
                ]
            ),
            vec![
                CompressionAlgorithm::Brotli,
                CompressionAlgorithm::Gzip,
                CompressionAlgorithm::Zstandard,
                CompressionAlgorithm::Deflate,
                CompressionAlgorithm::Compress,
            ]
        );

        assert_eq!(
            find_matches(
                "deflate;q=0.7, zstd;q=0.8, br;q=1.0",
                &[
                    CompressionAlgorithm::Deflate,
                    CompressionAlgorithm::Gzip,
                    CompressionAlgorithm::Brotli,
                    CompressionAlgorithm::Zstandard,
                ]
            ),
            vec![
                CompressionAlgorithm::Brotli,
                CompressionAlgorithm::Zstandard,
                CompressionAlgorithm::Deflate,
            ]
        );
    }
}
