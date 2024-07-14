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

//! Handles compression for a Pingora session, both static (precompressed files) and dynamic.

use http::{header, status::StatusCode};
use pandora_module_utils::pingora::{Error, ResponseCompression, ResponseHeader, SessionWrapper};
use std::path::{Path, PathBuf};

use crate::compression_algorithm::{find_matches, CompressionAlgorithm};

/// Encapsulates the compression state for the current session.
pub(crate) struct Compression<'a> {
    precompressed: &'a [CompressionAlgorithm],
    precompressed_active: Option<CompressionAlgorithm>,
    dynamic: bool,
}

impl<'a> Compression<'a> {
    /// Creates a new compression state supporting the given compression algorithms for
    /// pre-compressed files. *Note*: Dynamic compression is determined by the Pingora session.
    pub(crate) fn new(
        session: &impl SessionWrapper,
        precompressed: &'a [CompressionAlgorithm],
    ) -> Self {
        Self {
            precompressed,
            precompressed_active: None,
            // Remember this now, later on request header check might flip this flag
            dynamic: session
                .downstream_modules_ctx
                .get::<ResponseCompression>()
                .is_some_and(|compression| compression.is_enabled()),
        }
    }

    /// Checks whether the given path should be rewritten to a pre-compressed version of the file.
    pub(crate) fn rewrite_path(
        &mut self,
        session: &impl SessionWrapper,
        path: &Path,
    ) -> Option<PathBuf> {
        if self.precompressed.is_empty() {
            return None;
        }

        let filename = path.file_name()?;
        let requested = session.req_header().headers.get(header::ACCEPT_ENCODING)?;
        let overlap = find_matches(requested.to_str().ok()?, self.precompressed);

        for algorithm in overlap {
            let mut candidate_name = filename.to_os_string();
            candidate_name.push(".");
            candidate_name.push(algorithm.ext());

            let mut candidate_path = path.to_path_buf();
            candidate_path.set_file_name(candidate_name);
            if candidate_path.is_file() {
                self.precompressed_active = Some(algorithm);
                return Some(candidate_path);
            }
        }

        None
    }

    /// Applies the necessary modification to the HTTP response if compression is active. This will
    /// add `Content-Encoding` HTTP header among other thins.
    pub(crate) fn transform_header(
        &mut self,
        _session: &mut impl SessionWrapper,
        mut header: Box<ResponseHeader>,
    ) -> Result<Box<ResponseHeader>, Box<Error>> {
        let mut header =
            if header.status != StatusCode::OK && header.status != StatusCode::PARTIAL_CONTENT {
                // No actual content here, so no compression
                header
            } else if let Some(algorithm) = self.precompressed_active {
                // File is pre-compressed, only need to adjust header
                header.insert_header(header::CONTENT_ENCODING, algorithm.name())?;
                header
            } else {
                // Pingora’s dynamic compression will take care of this if necessary
                header
            };

        if !self.precompressed.is_empty() || self.dynamic {
            // If compression is enabled, we might produce different responses based on
            // Accept-Encoding header. Make sure to let the client know regardless of whether
            // compression is active right now.
            //
            // Note: This should not be necessary for dynamic compression. Pingora won't currently
            // do it however, see https://github.com/cloudflare/pingora/issues/233
            header.insert_header(header::VARY, "Accept-Encoding")?;
        }
        Ok(header)
    }
}
