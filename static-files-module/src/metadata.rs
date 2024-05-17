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

//! File metadata handling

use http::{header, status::StatusCode};
use httpdate::fmt_http_date;
use mime_guess::MimeGuess;
use module_utils::pingora::{ResponseHeader, SessionWrapper};
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::time::SystemTime;

/// Helper wrapping file metadata information
#[derive(Debug)]
pub struct Metadata {
    /// Guessed MIME types (if any) for the file
    pub mime: MimeGuess,
    /// File size in bytes
    pub size: u64,
    /// Last modified time of the file in the format `Fri, 15 May 2015 15:34:21 GMT` if the time
    /// can be retrieved
    pub modified: Option<String>,
    /// ETag header for the file, encoding last modified time and file size
    pub etag: String,
}

impl Metadata {
    /// Collects the metadata for a file. If `orig_path` is present, it will be used to determine
    /// the MIME type instead of `path`.
    ///
    /// This method will return any errors produced by [`std::fs::metadata()`]. It will also result
    /// in a [`ErrorKind::InvalidInput`] error if the path given doesn’t point to a regular file.
    pub fn from_path<P: AsRef<Path> + ?Sized>(
        path: &P,
        orig_path: Option<&P>,
    ) -> Result<Self, Error> {
        let meta = path.as_ref().metadata()?;

        if !meta.is_file() {
            return Err(ErrorKind::InvalidInput.into());
        }

        let mime = mime_guess::from_path(orig_path.unwrap_or(path));
        let size = meta.len();
        let modified = meta.modified().ok().map(fmt_http_date);
        let etag = format!(
            "\"{:x}-{:x}\"",
            meta.modified()
                .ok()
                .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_secs()),
            meta.len()
        );

        Ok(Self {
            mime,
            size,
            modified,
            etag,
        })
    }

    /// Checks `If-Match` and `If-Unmodified-Since` headers of the request to determine whether
    /// a `412 Precondition Failed` response should be produced.
    pub fn has_failed_precondition(&self, session: &impl SessionWrapper) -> bool {
        let headers = &session.req_header().headers;
        if let Some(value) = headers
            .get(header::IF_MATCH)
            .and_then(|value| value.to_str().ok())
        {
            value != "*"
                && value
                    .split(',')
                    .map(str::trim)
                    .all(|value| value != self.etag)
        } else if let Some(value) = headers
            .get(header::IF_UNMODIFIED_SINCE)
            .and_then(|value| value.to_str().ok())
        {
            self.modified
                .as_ref()
                .is_some_and(|modified| modified != value)
        } else {
            false
        }
    }

    /// Checks `If-None-Match` and `If-Modified-Since` headers of the request to determine whether
    /// a `304 Not Modified` response should be produced.
    pub fn is_not_modified(&self, session: &impl SessionWrapper) -> bool {
        let headers = &session.req_header().headers;
        if let Some(value) = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
        {
            value == "*"
                || value
                    .split(',')
                    .map(str::trim)
                    .any(|value| value == self.etag)
        } else if let Some(value) = headers
            .get(header::IF_MODIFIED_SINCE)
            .and_then(|value| value.to_str().ok())
        {
            self.modified
                .as_ref()
                .is_some_and(|modified| modified == value)
        } else {
            false
        }
    }

    #[inline(always)]
    fn add_common_headers(
        &self,
        header: &mut ResponseHeader,
    ) -> Result<(), Box<module_utils::pingora::Error>> {
        header.append_header(
            header::CONTENT_TYPE,
            self.mime.first_or_octet_stream().as_ref(),
        )?;
        if let Some(modified) = &self.modified {
            header.append_header(header::LAST_MODIFIED, modified)?;
        }
        header.append_header(header::ETAG, &self.etag)?;
        Ok(())
    }

    /// Produces a `200 OK` response and adds headers according to file metadata.
    pub(crate) fn to_response_header(
        &self,
    ) -> Result<Box<ResponseHeader>, Box<module_utils::pingora::Error>> {
        let mut header = ResponseHeader::build(StatusCode::OK, Some(8))?;
        header.append_header(header::CONTENT_LENGTH, self.size.to_string())?;
        header.append_header(header::ACCEPT_RANGES, "bytes")?;
        self.add_common_headers(&mut header)?;
        Ok(Box::new(header))
    }

    /// Produces a `206 Partial Content` response and adds headers according to file metadata.
    pub(crate) fn to_partial_content_header(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Box<ResponseHeader>, Box<module_utils::pingora::Error>> {
        let mut header = ResponseHeader::build(StatusCode::PARTIAL_CONTENT, Some(8))?;
        header.append_header(header::CONTENT_LENGTH, (end - start + 1).to_string())?;
        header.append_header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{}", self.size),
        )?;
        self.add_common_headers(&mut header)?;
        Ok(Box::new(header))
    }

    /// Produces a response with specified status code and no response body (all headers added
    /// except `Content-Length``).
    pub(crate) fn to_custom_header(
        &self,
        status: StatusCode,
    ) -> Result<Box<ResponseHeader>, Box<module_utils::pingora::Error>> {
        let mut header = ResponseHeader::build(status, Some(4))?;
        self.add_common_headers(&mut header)?;
        Ok(Box::new(header))
    }
}
