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

use bytes::Bytes;
use http::{header, status::StatusCode};
use pandora_module_utils::pingora::{Error, HttpTask, ResponseHeader, SessionWrapper};
use std::path::{Path, PathBuf};

use crate::compression_algorithm::{find_matches, CompressionAlgorithm};
use pingora_core::modules::http::compression::ResponseCompression;

/// Encapsulates the compression state for the current session.
pub(crate) struct Compression<'a> {
    precompressed: &'a [CompressionAlgorithm],
    precompressed_active: Option<CompressionAlgorithm>,
    dynamic: bool,
    dynamic_active: bool,
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
                .is_some_and(|rc| rc.is_enabled()),
            dynamic_active: false,
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
        session: &mut impl SessionWrapper,
        mut header: Box<ResponseHeader>,
    ) -> Result<Box<ResponseHeader>, Box<Error>> {
        let mut header = if header.status != StatusCode::OK
            && header.status != StatusCode::PARTIAL_CONTENT
        {
            // No actual content here, so no compression
            header
        } else if let Some(algorithm) = self.precompressed_active {
            // File is pre-compressed, only need to adjust header
            header.insert_header(header::CONTENT_ENCODING, algorithm.name())?;
            header
        } else if header.status == StatusCode::OK {
            // Delegate to Pingora's dynamic compression implementation
            self.dynamic_active = true;

            let raw_session = session.deref_mut();
            let req_hdr = raw_session.downstream_session.req_header();
            if let Some(rc) = raw_session
                .downstream_modules_ctx
                .get_mut::<ResponseCompression>()
            {
                rc.request_filter(req_hdr);
            }

            // Always pass false for end, even if no body follows. This will result in compression headers
            // on HEAD responses but that should be the right thing to do anyway.
            let mut task = HttpTask::Header(header, false);

            if let Some(rc) = raw_session
                .downstream_modules_ctx
                .get_mut::<ResponseCompression>()
            {
                rc.response_filter(&mut task);
            }

            if let HttpTask::Header(mut header, false) = task {
                if header.headers.get(header::CONTENT_ENCODING).is_some() {
                    // Response is compressed dynamically, no support for ranged requests.
                    // Ideally, pingora should do this: https://github.com/cloudflare/pingora/issues/229
                    let _ = header.insert_header(header::ACCEPT_RANGES, "none");
                }
                header
            } else {
                panic!("Unexpected: compression response filter replaced header task by {task:?}");
            }
        } else {
            // This is a Partial Content response, no dynamic compression here
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

    /// Applies the necessary modifications to the response body when dynamic compression is
    /// active. Returns the bytes to be sent to the client if any. The value `None` for `bytes`
    /// indicates end of body.
    pub(crate) fn transform_body(
        &self,
        session: &mut impl SessionWrapper,
        bytes: Option<Bytes>,
    ) -> Option<Bytes> {
        if !self.dynamic_active {
            // Nothing to do here if we are serving a precompressed file or handling an
            // uncompressed response.
            return bytes;
        }

        // Delegate to Pingora's dynamic compression implementation
        let mut task = if let Some(bytes) = bytes {
            HttpTask::Body(Some(bytes), false)
        } else {
            HttpTask::Done
        };

        if let Some(rc) = session
            .downstream_modules_ctx
            .get_mut::<ResponseCompression>()
        {
            rc.response_filter(&mut task);
        }
        if let HttpTask::Body(Some(bytes), _) = task {
            if bytes.is_empty() {
                None
            } else {
                Some(bytes)
            }
        } else {
            None
        }
    }
}
