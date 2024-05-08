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

//! Handler for the `request_filter` phase.

use log::{debug, info, warn};
use pingora_core::{Error, ErrorType};
use pingora_http::{Method, StatusCode};
use pingora_proxy::Session;
use std::io::ErrorKind;

use crate::compression::Compression;
use crate::configuration::StaticFilesConf;
use crate::file_writer::file_response;
use crate::metadata::Metadata;
use crate::path::{path_to_uri, resolve_uri};
use crate::range::{extract_range, Range};
use crate::standard_response::{error_response, redirect_response};

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug)]
pub struct StaticFilesHandler {
    conf: StaticFilesConf,
}

impl StaticFilesHandler {
    /// Creates a new handler with given configuration. This will canonicalize the path to the root
    /// directory and might result in an error if that path isn’t accessible.
    pub fn new(mut conf: StaticFilesConf) -> Result<Self, Box<Error>> {
        conf.root = conf.root.canonicalize().map_err(|err| {
            Error::because(
                ErrorType::InternalError,
                format!("Failed accessing root path {:?}", conf.root),
                err,
            )
        })?;

        debug!("Initialized static files handler, settings: {conf:#?}");
        Ok(Self { conf })
    }

    /// Provides read-only access to the handler’s configuration.
    pub fn conf(&self) -> &StaticFilesConf {
        &self.conf
    }

    /// Provides read-write access to the handler’s configuration.
    pub fn conf_mut(&mut self) -> &mut StaticFilesConf {
        &mut self.conf
    }

    /// Handles the current request. Will typically return `true` (request handled), an error
    /// should only be returned in exceptional cases.
    ///
    /// *Note*: This handler will handle all requests. It never returns `false`.
    pub async fn handle(&self, session: &mut Session) -> Result<bool, Box<Error>> {
        let uri = &session.req_header().uri;
        debug!("received URI path {}", uri.path());

        let mut path = match resolve_uri(uri.path(), &self.conf.root) {
            Ok(path) => path,
            Err(err) => {
                let status = match err.kind() {
                    ErrorKind::InvalidInput => {
                        warn!("rejecting invalid path {}", uri.path());
                        StatusCode::BAD_REQUEST
                    }
                    ErrorKind::InvalidData => {
                        warn!("Requested path outside root directory: {}", uri.path());
                        StatusCode::BAD_REQUEST
                    }
                    ErrorKind::NotFound => {
                        debug!("canonicalizing resulted in NotFound error");
                        StatusCode::NOT_FOUND
                    }
                    ErrorKind::PermissionDenied => {
                        debug!("canonicalizing resulted in PermissionDenied error");
                        StatusCode::FORBIDDEN
                    }
                    _ => {
                        warn!("failed canonicalizing the path {}: {err}", uri.path());
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                error_response(session, status).await?;
                return Ok(true);
            }
        };

        debug!("translated into file path {path:?}");

        if self.conf.canonicalize_uri {
            if let Some(mut canonical) = path_to_uri(&path, &self.conf.root) {
                if canonical != uri.path() {
                    if let Some(query) = uri.query() {
                        canonical.push('?');
                        canonical.push_str(query);
                    }
                    info!("redirecting to canonical URI: {canonical}");
                    redirect_response(session, StatusCode::PERMANENT_REDIRECT, &canonical).await?;
                    return Ok(true);
                }
            }
        }

        if path.is_dir() {
            for filename in &self.conf.index_file {
                let candidate = path.join(filename);
                if candidate.is_file() {
                    debug!("using directory index file {filename}");
                    path = candidate;
                }
            }
        }

        info!("successfully resolved request path: {path:?}");

        match session.req_header().method {
            Method::GET | Method::HEAD => {
                // Allowed
            }
            _ => {
                warn!("Denying method {}", session.req_header().method);
                error_response(session, StatusCode::METHOD_NOT_ALLOWED).await?;
                return Ok(true);
            }
        }

        let mut compression = Compression::new(session, &self.conf.precompressed);

        let (path, orig_path) =
            if let Some(precompressed_path) = compression.rewrite_path(session, &path) {
                (precompressed_path, Some(path))
            } else {
                (path, None)
            };

        let meta = match Metadata::from_path(&path, orig_path.as_ref()) {
            Ok(meta) => meta,
            Err(err) if err.kind() == ErrorKind::InvalidInput => {
                warn!("Path {path:?} is not a regular file, denying access");
                error_response(session, StatusCode::FORBIDDEN).await?;
                return Ok(true);
            }
            Err(err) => {
                warn!("failed retrieving metadata for path {path:?}: {err}");
                error_response(session, StatusCode::INTERNAL_SERVER_ERROR).await?;
                return Ok(true);
            }
        };

        if meta.has_failed_precondition(session) {
            debug!("If-Match/If-Unmodified-Since precondition failed");
            let header = meta.to_custom_header(StatusCode::PRECONDITION_FAILED)?;
            let header = compression.transform_header(session, header)?;
            session.write_response_header(header).await?;
            return Ok(true);
        }

        if meta.is_not_modified(session) {
            debug!("If-None-Match/If-Modified-Since check resulted in Not Modified");
            let header = meta.to_custom_header(StatusCode::NOT_MODIFIED)?;
            let header = compression.transform_header(session, header)?;
            session.write_response_header(header).await?;
            return Ok(true);
        }

        let (header, start, end) = match extract_range(session, &meta) {
            Some(Range::Valid(start, end)) => {
                debug!("bytes range requested: {start}-{end}");
                let header = meta.to_partial_content_header(start, end)?;
                let header = compression.transform_header(session, header)?;
                (header, start, end)
            }
            Some(Range::OutOfBounds) => {
                debug!("requested bytes range is out of bounds");
                let header = meta.to_custom_header(StatusCode::RANGE_NOT_SATISFIABLE)?;
                let header = compression.transform_header(session, header)?;
                session.write_response_header(header).await?;
                return Ok(true);
            }
            None => {
                // Range is either missing or cannot be parsed, produce the entire file.
                let header = meta.to_response_header()?;
                let header = compression.transform_header(session, header)?;
                (header, 0, meta.size - 1)
            }
        };

        session.write_response_header(header).await?;

        if session.req_header().method == Method::GET {
            // sendfile would be nice but not currently possible within pingora-proxy (see
            // https://github.com/cloudflare/pingora/issues/160)
            file_response(session, &path, start, end, &compression).await?;
        }
        Ok(true)
    }
}
