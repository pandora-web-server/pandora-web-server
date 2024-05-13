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

//! Writing files to Pingora session.

use bytes::BytesMut;
use http::status::StatusCode;
use log::error;
use module_utils::pingora::{Error, ErrorType, Session};
use std::cmp::min;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::compression::Compression;

const BUFFER_SIZE: usize = 64 * 1024;

/// Writes a chunk of a file as a Pingora session response. The data will be passed through the
/// compression handler first in case dynamic compression is enabled.
pub(crate) async fn file_response(
    session: &mut Session,
    path: &Path,
    start: u64,
    end: u64,
    compression: &Compression<'_>,
) -> Result<(), Box<Error>> {
    let mut file = File::open(path).map_err(|err| {
        error!("failed opening file {path:?}: {err}");
        Error::new(ErrorType::HTTPStatus(
            StatusCode::INTERNAL_SERVER_ERROR.into(),
        ))
    })?;

    if start != 0 {
        file.seek(SeekFrom::Start(start)).map_err(|err| {
            error!("failed seeking in file {path:?}: {err}");
            Error::new(ErrorType::HTTPStatus(
                StatusCode::INTERNAL_SERVER_ERROR.into(),
            ))
        })?;
    }

    let mut remaining = (end - start + 1) as usize;
    while remaining > 0 {
        let mut buf = BytesMut::zeroed(min(remaining, BUFFER_SIZE));
        let len = file.read(buf.as_mut()).map_err(|err| {
            error!("failed reading data from {path:?}: {err}");
            Error::new(ErrorType::HTTPStatus(
                StatusCode::INTERNAL_SERVER_ERROR.into(),
            ))
        })?;

        if len == 0 {
            error!("file ended with {remaining} bytes left to be written");
            return Err(Error::new(ErrorType::ReadError));
        }

        buf.truncate(len);
        if let Some(bytes) = compression.transform_body(session, Some(buf.into())) {
            session.write_response_body(bytes).await?;
        }
        remaining -= len;
    }

    if let Some(bytes) = compression.transform_body(session, None) {
        session.write_response_body(bytes).await?;
    }

    Ok(())
}
