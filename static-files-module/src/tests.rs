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

use crate::handler::StaticFilesHandler;
use crate::metadata::Metadata;

use compression_module::CompressionHandler;
use const_format::{concatcp, str_repeat};
use http::status::StatusCode;
use pandora_module_utils::pingora::{
    create_test_session, ErrorType, RequestHeader, Session, SessionWrapper,
};
use pandora_module_utils::standard_response::response_text;
use pandora_module_utils::{FromYaml, RequestFilter};
use rewrite_module::RewriteHandler;
use startup_module::{AppResult, DefaultApp};
use std::path::PathBuf;
use test_log::test;

#[derive(Debug, Clone, PartialEq, Eq, RequestFilter)]
struct Handler {
    compression: CompressionHandler,
    rewrite: RewriteHandler,
    static_files: StaticFilesHandler,
}

fn root_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("testdata");
    path.push("root");
    if !filename.is_empty() {
        path.push(filename);
    }
    path
}

fn default_conf() -> String {
    format!(
        "root: {}",
        root_path("").into_os_string().into_string().unwrap()
    )
}

fn extended_conf(conf_str: impl AsRef<str>) -> String {
    format!("{}\n{}", default_conf(), conf_str.as_ref())
}

fn make_app(conf_str: impl AsRef<str>) -> DefaultApp<Handler> {
    DefaultApp::new(
        <Handler as RequestFilter>::Conf::from_yaml(conf_str)
            .unwrap()
            .try_into()
            .unwrap(),
    )
}

async fn make_session(method: &str, path: &str) -> Session {
    let header = RequestHeader::build(method, path.as_bytes(), None).unwrap();

    create_test_session(header).await
}

fn assert_status(result: &mut AppResult, expected: u16) {
    assert_eq!(
        result.session().response_written().unwrap().status.as_u16(),
        expected
    );
}

fn assert_headers(result: &mut AppResult, expected: Vec<(&str, &str)>) {
    let mut headers: Vec<_> = result
        .session()
        .response_written()
        .unwrap()
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_ascii_lowercase(),
                value.to_str().unwrap().to_owned(),
            )
        })
        .filter(|(name, _)| name != "connection" && name != "date")
        .collect();
    headers.sort();

    let mut expected: Vec<_> = expected
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
        .collect();
    expected.sort();

    assert_eq!(headers, expected);
}

fn assert_body(result: &AppResult, expected: &str) {
    assert_eq!(result.body_str(), expected);
}

#[test(tokio::test)]
async fn unconfigured() {
    let mut app = make_app("root:");

    let session = make_session("GET", "/file.txt").await;
    let mut result = app.handle_request(session).await;
    assert_eq!(
        result.err().as_ref().map(|err| &err.etype),
        Some(&ErrorType::HTTPStatus(404))
    );
    assert!(result.session().response_written().is_none());
    assert_body(&result, "");
}

#[test(tokio::test)]
async fn text_file() {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let session = make_session("GET", "/file.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();
    let session = make_session("GET", "/large.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, concatcp!(str_repeat!("0123456789", 10000), "\n"));
}

#[test(tokio::test)]
async fn dir_index() {
    let meta = Metadata::from_path(&root_path("index.html"), None).unwrap();

    let mut app = make_app(extended_conf("index_file: [index.html]"));
    let session = make_session("GET", "/").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/html;charset=utf-8"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "<html>Hi!</html>\n");

    // Without matching directory index this should produce Forbidden response.
    let mut app = make_app(default_conf());

    let text = response_text(StatusCode::FORBIDDEN);
    let session = make_session("GET", "/").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 403);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn no_trailing_slash() {
    let mut app = make_app(default_conf());
    let text = response_text(StatusCode::PERMANENT_REDIRECT);

    let session = make_session("GET", "/subdir?xyz").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 308);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
            ("location", "/subdir/?xyz"),
        ],
    );
    assert_body(&result, &text);

    // Scenario where prefix has been stripped from URI
    let mut app = make_app(extended_conf(
        "rewrite_rules: {from: /static/*, to: '${tail}${query}'}",
    ));

    let session = make_session("GET", "/static/subdir?xyz").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 308);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
            ("location", "/static/subdir/?xyz"),
        ],
    );
    assert_body(&result, &text);

    // Without canonicalize_uri this should just produce the response
    // (Forbidden because no index file).
    let mut app = make_app(extended_conf("canonicalize_uri: false"));

    let text = response_text(StatusCode::FORBIDDEN);
    let session = make_session("GET", "/subdir").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 403);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn unnecessary_percent_encoding() {
    let mut app = make_app(default_conf());
    let text = response_text(StatusCode::PERMANENT_REDIRECT);

    let session = make_session("GET", "/file%2Etxt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 308);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
            ("location", "/file.txt"),
        ],
    );
    assert_body(&result, &text);

    // Scenario where prefix has been stripped from URI
    let mut app = make_app(extended_conf(
        "rewrite_rules: {from: /static/*, to: '${tail}'}",
    ));

    let session = make_session("GET", "/static/file%2Etxt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 308);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
            ("location", "/static/file.txt"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn complex_path() {
    let mut app = make_app(default_conf());
    let text = response_text(StatusCode::PERMANENT_REDIRECT);

    let session = make_session("GET", "/.//subdir/../file.txt?file%2Etxt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 308);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
            ("location", "/file.txt?file%2Etxt"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn utf8_path() {
    let meta = Metadata::from_path(&root_path("subdir/файл söndärzeichen.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let session = make_session(
        "GET",
        "/subdir/%D1%84%D0%B0%D0%B9%D0%BB%20s%C3%B6nd%C3%A4rzeichen.txt",
    )
    .await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");
}

#[test(tokio::test)]
async fn no_file() {
    let mut app = make_app(default_conf());
    let text = response_text(StatusCode::NOT_FOUND);

    let session = make_session("GET", "/missing.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 404);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn no_file_with_page_404() {
    let mut app = make_app(extended_conf("page_404: /file.txt"));

    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let session = make_session("GET", "/missing.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 404);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");
}

#[test(tokio::test)]
async fn no_index() {
    let mut app = make_app(default_conf());

    let text = response_text(StatusCode::FORBIDDEN);
    let session = make_session("GET", "/subdir/").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 403);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn wrong_method() {
    let mut app = make_app(default_conf());

    let text = response_text(StatusCode::METHOD_NOT_ALLOWED);
    let session = make_session("POST", "/file.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 405);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn wrong_method_no_file() {
    let mut app = make_app(default_conf());
    let text = response_text(StatusCode::NOT_FOUND);

    let session = make_session("POST", "/missing.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 404);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn head_request() {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let session = make_session("HEAD", "/file.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    let text = response_text(StatusCode::NOT_FOUND);
    let session = make_session("HEAD", "/missing.txt").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 404);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, "");

    let text = response_text(StatusCode::PERMANENT_REDIRECT);
    let session = make_session("HEAD", "/subdir").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 308);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
            ("location", "/subdir/"),
        ],
    );
    assert_body(&result, "");
}

#[test(tokio::test)]
async fn bad_request() {
    let mut app = make_app(default_conf());
    let text = response_text(StatusCode::BAD_REQUEST);

    let session = make_session("GET", ".").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 400);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);

    let session = make_session("GET", "/../").await;
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 400);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html;charset=utf-8"),
        ],
    );
    assert_body(&result, &text);
}

#[test(tokio::test)]
async fn if_none_match() {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", &meta.etag)
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 304);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", "*")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 304);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", format!("\"xyz\", {}", &meta.etag))
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 304);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", &meta.etag)
        .unwrap();
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GMT")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 304);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", "\"xyz\"")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    // With compression enabled this should produce Vary header
    let mut app = make_app(extended_conf("compression_level_gzip: 3"));
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", &meta.etag)
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 304);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&result, "");
}

#[test(tokio::test)]
async fn if_match() {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", &meta.etag)
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", "*")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", format!("\"xyz\", {}", &meta.etag))
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", &meta.etag)
        .unwrap();
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", "Thu, 01 Jan 1970 00:00:00 GTM")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 412);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    // With compression enabled this should produce Vary header
    let mut app = make_app(extended_conf("compression_level_gzip: 3"));
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 412);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&result, "");
}

#[test(tokio::test)]
async fn if_modified_since() {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", meta.modified.as_ref().unwrap())
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 304);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GTM")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", meta.modified.as_ref().unwrap())
        .unwrap();
    session
        .req_header_mut()
        .insert_header("If-None-Match", "\"xyz\"")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    // With compression enabled this should produce Vary header
    let mut app = make_app(extended_conf("compression_level_gzip: 3"));
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", meta.modified.as_ref().unwrap())
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 304);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&result, "");
}

#[test(tokio::test)]
async fn if_unmodified_since() {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", meta.modified.as_ref().unwrap())
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", "Thu, 01 Jan 1970 00:00:00 GMT")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 412);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", meta.modified.as_ref().unwrap())
        .unwrap();
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 412);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    // With compression enabled this should produce Vary header
    let mut app = make_app(extended_conf("compression_level_gzip: 3"));
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", meta.modified.as_ref().unwrap())
        .unwrap();
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 412);
    assert_headers(
        &mut result,
        vec![
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&result, "");
}

#[test(tokio::test)]
async fn ranged_request() {
    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();

    let mut app = make_app(default_conf());
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=2-5")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 206);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", "4"),
            ("content-range", "bytes 2-5/100001"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "2345");

    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=99999-")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 206);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", "2"),
            ("content-range", "bytes 99999-100000/100001"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "9\n");

    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=-5")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 206);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", "5"),
            ("content-range", "bytes 99996-100000/100001"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "6789\n");

    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=200000-")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 416);
    assert_headers(
        &mut result,
        vec![
            ("Content-Type", "text/plain;charset=utf-8"),
            ("Content-Range", "bytes */100001"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&result, "");

    // With compression enabled this should produce Vary header
    let mut app = make_app(extended_conf("compression_level_gzip: 3"));
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=200000-")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 416);
    assert_headers(
        &mut result,
        vec![
            ("Content-Type", "text/plain;charset=utf-8"),
            ("Content-Range", "bytes */100001"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&result, "");
}

#[test(tokio::test)]
async fn dynamic_compression() {
    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();
    let mut app = make_app(extended_conf("compression_level_gzip: 3"));

    // Regular request should result in compressed response
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "gzip")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Encoding", "gzip"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("Transfer-Encoding", "chunked"),
            ("vary", "Accept-Encoding"),
        ],
    );

    // Request without matching encodings should result in uncompressed response
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "unsupported")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );

    // We shouldn’t get ranged requests in practice but Pingora will compress even these responses.
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "gzip")
        .unwrap();
    session
        .req_header_mut()
        .insert_header("Range", "bytes=0-10000")
        .unwrap();
    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());
    assert_status(&mut result, 206);
    assert_headers(
        &mut result,
        vec![
            ("Content-Encoding", "gzip"),
            ("content-range", "bytes 0-10000/100001"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("Transfer-Encoding", "chunked"),
            ("vary", "Accept-Encoding"),
        ],
    );
}

#[test(tokio::test)]
async fn static_compression() {
    let meta = Metadata::from_path(&root_path("large_precompressed.txt"), None).unwrap();
    let meta_compressed =
        Metadata::from_path(&root_path("large_precompressed.txt.gz"), None).unwrap();
    let mut app = make_app(extended_conf("precompressed: [gz, br]"));

    // Regular request should result in compressed response
    let mut session = make_session("GET", "/large_precompressed.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "br, gzip")
        .unwrap();

    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());

    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta_compressed.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta_compressed.modified.as_ref().unwrap()),
            ("etag", &meta_compressed.etag),
            ("Content-Encoding", "gzip"),
            ("vary", "Accept-Encoding"),
        ],
    );

    // Static compression should take precedence over dynamic
    let mut session = make_session("GET", "/large_precompressed.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "br, gzip")
        .unwrap();

    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());

    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta_compressed.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta_compressed.modified.as_ref().unwrap()),
            ("etag", &meta_compressed.etag),
            ("Content-Encoding", "gzip"),
            ("vary", "Accept-Encoding"),
        ],
    );

    // Request without matching encodings should result in uncompressed response
    let mut session = make_session("GET", "/large_precompressed.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "zstd")
        .unwrap();

    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());

    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );

    // Ranged response should be compressed
    let mut session = make_session("GET", "/large_precompressed.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "gzip")
        .unwrap();
    session
        .req_header_mut()
        .insert_header("Range", "bytes=0-10")
        .unwrap();

    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());

    assert_status(&mut result, 206);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", "11"),
            (
                "content-range",
                &format!("bytes 0-10/{}", meta_compressed.size),
            ),
            ("Content-Type", "text/plain;charset=utf-8"),
            ("last-modified", meta_compressed.modified.as_ref().unwrap()),
            ("etag", &meta_compressed.etag),
            ("Content-Encoding", "gzip"),
            ("vary", "Accept-Encoding"),
        ],
    );
}

#[test(tokio::test)]
async fn charset() {
    let meta = Metadata::from_path(&root_path("large_precompressed.txt.gz"), None).unwrap();

    // Binary files shouldn’t have a charset by default
    let mut app = make_app(default_conf());
    let session = make_session("GET", "/large_precompressed.txt.gz").await;

    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());

    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "application/gzip"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );

    // Enable charset for specific MIME type
    let mut app = make_app(extended_conf(
        "declare_charset: windows-1251\ndeclare_charset_types: application/gzip",
    ));
    let session = make_session("GET", "/large_precompressed.txt.gz").await;

    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());

    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "application/gzip;charset=windows-1251"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );

    // Enable charset for all MIME type
    let mut app = make_app(extended_conf("declare_charset_types: '*'"));
    let session = make_session("GET", "/large_precompressed.txt.gz").await;

    let mut result = app.handle_request(session).await;
    assert!(result.err().is_none());

    assert_status(&mut result, 200);
    assert_headers(
        &mut result,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "application/gzip;charset=utf-8"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
}
