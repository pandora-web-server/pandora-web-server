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

//! Byte range processing (`Range` HTTP header)

use http::header;
use pandora_module_utils::pingora::SessionWrapper;
use std::str::FromStr;

use crate::metadata::Metadata;

/// Represents the result of parsing the `Range` HTTP header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Range {
    /// A valid range with the given start and end bounds
    Valid(u64, u64),
    /// A range that is outside of the file’s boundaries
    OutOfBounds,
}

impl Range {
    /// Parses the value of a `Range` HTTP header. The file size is required to resolve ranges
    /// specified relative to the end of file and to recognize out of bounds ranges. Ranges that
    /// cannot be parsed (unexpected format) will result in `None`.
    pub fn parse(range: &str, file_size: u64) -> Option<Self> {
        let (units, range) = range.split_once('=')?;
        if units != "bytes" {
            return None;
        }

        let (start, end) = range.trim().split_once('-')?;
        let (start, end) = if start.is_empty() {
            let len = u64::from_str(end.trim()).ok()?;
            if len > file_size {
                return Some(Self::OutOfBounds);
            }
            (file_size - len, file_size - 1)
        } else if end.is_empty() {
            (u64::from_str(start.trim()).ok()?, file_size - 1)
        } else {
            (
                u64::from_str(start.trim()).ok()?,
                u64::from_str(end.trim()).ok()?,
            )
        };

        if end >= file_size || start > end {
            Some(Self::OutOfBounds)
        } else {
            Some(Self::Valid(start, end))
        }
    }
}

/// This processes the `Range` and `If-Range` request headers to produce the requested byte range
/// if any.
///
/// `Range` header missing, using some unsupported format or overruled by `If-Range` header will
/// all result in `None` being returned.
///
/// Note: Multiple ranges are not supported.
pub fn extract_range(session: &impl SessionWrapper, meta: &Metadata) -> Option<Range> {
    let headers = &session.req_header().headers;
    if let Some(value) = headers
        .get(header::IF_RANGE)
        .and_then(|value| value.to_str().ok())
    {
        if value != meta.etag
            && !meta
                .modified
                .as_ref()
                .is_some_and(|modified| modified == value)
        {
            return None;
        }
    }

    let value = headers.get(header::RANGE)?;
    let value = value.to_str().ok()?;

    Range::parse(value, meta.size)
}

#[cfg(test)]
mod tests {
    use super::*;

    use mime_guess::MimeGuess;
    use pandora_module_utils::{
        pingora::{create_test_session, RequestHeader, Session},
        RequestFilter,
    };
    use startup_module::{AppResult, DefaultApp};
    use test_log::test;

    #[derive(Debug, Clone, PartialEq, Eq, RequestFilter)]
    struct Handler {}

    fn metadata() -> Metadata {
        Metadata {
            mime: MimeGuess::from_ext("txt").first_or_octet_stream(),
            size: 1000,
            modified: Some("Fri, 15 May 2015 15:34:21 GMT".into()),
            etag: "\"abc\"".into(),
        }
    }

    async fn make_session(range: &str) -> Session {
        let mut header = RequestHeader::build("GET", b"/", None).unwrap();

        if !range.is_empty() {
            header.insert_header("Range", range).unwrap();
        }

        create_test_session(header).await
    }

    async fn process_session(session: Session) -> AppResult {
        DefaultApp::new(Handler {}).handle_request(session).await
    }

    #[test(tokio::test)]
    async fn no_range() {
        let session = make_session("").await;
        let mut result = process_session(session).await;
        assert_eq!(extract_range(&result.session(), &metadata()), None);
    }

    #[test(tokio::test)]
    async fn valid_range() {
        let session = make_session("bytes=0-499").await;
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::Valid(0, 499))
        );
    }

    #[test(tokio::test)]
    async fn unknown_units() {
        let session = make_session("eur=0-499").await;
        let mut result = process_session(session).await;
        assert_eq!(extract_range(&result.session(), &metadata()), None);
    }

    #[test(tokio::test)]
    async fn open_range() {
        let session = make_session("bytes=500-").await;
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::Valid(500, 999))
        );
    }

    #[test(tokio::test)]
    async fn end_range() {
        let session = make_session("bytes=-10").await;
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::Valid(990, 999))
        );
    }

    #[test(tokio::test)]
    async fn out_of_bounds_ranges() {
        let session = make_session("bytes=-2000").await;
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::OutOfBounds)
        );

        let session = make_session("bytes=23-22").await;
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::OutOfBounds)
        );

        let session = make_session("bytes=1000-").await;
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::OutOfBounds)
        );
    }

    #[test(tokio::test)]
    async fn multiple_ranges() {
        // Multiple ranges are unsupported, should be treated like no Range header.
        let session = make_session("bytes=1-2,3-4").await;
        let mut result = process_session(session).await;
        assert_eq!(extract_range(&result.session(), &metadata()), None);
    }

    #[test(tokio::test)]
    async fn if_range() {
        let mut session = make_session("bytes=0-499").await;
        session
            .req_header_mut()
            .insert_header("If-Range", "\"abc\"")
            .unwrap();
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::Valid(0, 499))
        );

        let mut session = make_session("bytes=0-499").await;
        session
            .req_header_mut()
            .insert_header("If-Range", "\"xyz\"")
            .unwrap();
        let mut result = process_session(session).await;
        assert_eq!(extract_range(&result.session(), &metadata()), None);

        let mut session = make_session("bytes=0-499").await;
        session
            .req_header_mut()
            .insert_header("If-Range", "Fri, 15 May 2015 15:34:21 GMT")
            .unwrap();
        let mut result = process_session(session).await;
        assert_eq!(
            extract_range(&result.session(), &metadata()),
            Some(Range::Valid(0, 499))
        );

        let mut session = make_session("bytes=0-499").await;
        session
            .req_header_mut()
            .insert_header("If-Range", "Thu, 01 Jan 1970 00:00:00 GMT")
            .unwrap();
        let mut result = process_session(session).await;
        assert_eq!(extract_range(&result.session(), &metadata()), None);

        let mut session = make_session("bytes=0-499").await;
        session
            .req_header_mut()
            .insert_header("If-Range", "bogus")
            .unwrap();
        let mut result = process_session(session).await;
        assert_eq!(extract_range(&result.session(), &metadata()), None);
    }
}
