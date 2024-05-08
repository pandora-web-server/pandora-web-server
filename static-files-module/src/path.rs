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

//! Path resolution logic

use percent_encoding::{percent_decode_str, percent_encode, AsciiSet, CONTROLS};
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

// This matches pingora logic, see https://github.com/cloudflare/pingora/blob/2501d4adb038d93613c0edbd7c1e3b3de9b415b1/pingora-core/src/protocols/http/v1/server.rs#L934
const URI_ESC_CHARSET: &AsciiSet = &CONTROLS.add(b' ').add(b'<').add(b'>').add(b'"');

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> &std::ffi::OsStr {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    OsStr::from_bytes(bytes)
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> String {
    // This should really be OsStr::from_encoded_bytes_unchecked() but it’s
    // unsafe. With this fallback non-Unicode file names will result in 404.
    String::from_utf8_lossy(bytes).into_owned()
}

/// Resolves the path from a URI against the path to a root directory.
///
/// This will return an error under the following conditions:
///
/// * Invalid path, not starting with a slash (/): results in [`ErrorKind::InvalidInput`]
/// * Resolved path outside the root directory: results in [`ErrorKind::InvalidData`]
/// * [`std::fs::canonicalize()`] failed: results in [`ErrorKind::NotFound`],
///   [`ErrorKind::PermissionDenied`] and other errors
pub fn resolve_uri(uri_path: &str, root: &Path) -> Result<PathBuf, Error> {
    let uri_path = uri_path.strip_prefix('/').ok_or(ErrorKind::InvalidInput)?;

    let uri_path = uri_path.strip_suffix('/').unwrap_or(uri_path);

    let mut path = root.to_path_buf();
    for component in uri_path.split('/') {
        let decoded = percent_decode_str(component).collect::<Vec<_>>();
        path.push(path_from_bytes(&decoded))
    }

    let path = path.canonicalize()?;

    if path.starts_with(root) {
        Ok(path)
    } else {
        Err(ErrorKind::InvalidData.into())
    }
}

/// Calculates the canonical URI path describing the path relative to a root directory.
///
/// This will return `None` for paths outside the root directory.
pub fn path_to_uri(path: &Path, root: &Path) -> Option<String> {
    let rel_path = path.strip_prefix(root).ok()?;

    let mut uri = String::from('/');
    for component in rel_path.components() {
        uri.push_str(
            &percent_encode(component.as_os_str().as_encoded_bytes(), URI_ESC_CHARSET).to_string(),
        );
        uri.push('/');
    }
    if !path.is_dir() && uri.len() > 1 {
        uri.pop();
    }
    Some(uri)
}
