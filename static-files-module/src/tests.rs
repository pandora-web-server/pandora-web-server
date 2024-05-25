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

use crate::configuration::StaticFilesConf;
use crate::handler::StaticFilesHandler;
use crate::metadata::Metadata;

use const_format::{concatcp, str_repeat};
use http::status::StatusCode;
use module_utils::pingora::{Error, RequestHeader, SessionWrapper, TestSession};
use module_utils::standard_response::response_text;
use module_utils::{FromYaml, RequestFilter, RequestFilterResult};
use std::path::PathBuf;
use test_log::test;

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

fn make_handler(conf_str: impl AsRef<str>) -> StaticFilesHandler {
    StaticFilesConf::from_yaml(conf_str)
        .unwrap()
        .try_into()
        .unwrap()
}

async fn make_session(method: &str, path: &str) -> TestSession {
    let header = RequestHeader::build(method, path.as_bytes(), None).unwrap();

    TestSession::from(header).await
}

fn assert_status(session: &TestSession, expected: u16) {
    assert_eq!(
        session.response_written().unwrap().status.as_u16(),
        expected
    );
}

fn assert_headers(session: &TestSession, expected: Vec<(&str, &str)>) {
    let mut headers: Vec<_> = session
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
        .collect();
    headers.sort();

    let mut expected: Vec<_> = expected
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
        .collect();
    expected.sort();

    assert_eq!(headers, expected);
}

fn assert_body(session: &TestSession, expected: &str) {
    assert_eq!(
        String::from_utf8_lossy(&session.response_body).as_ref(),
        expected
    );
}

#[test(tokio::test)]
async fn unconfigured() -> Result<(), Box<Error>> {
    let handler = make_handler("root:");

    let mut session = make_session("GET", "/file.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::Unhandled
    );
    assert!(session.response_written().is_none());
    assert_body(&session, "");

    Ok(())
}

#[test(tokio::test)]
async fn text_file() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();
    let mut session = make_session("GET", "/large.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, concatcp!(str_repeat!("0123456789", 10000), "\n"));

    Ok(())
}

#[test(tokio::test)]
async fn dir_index() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("index.html"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("GET", "/").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/html"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "<html>Hi!</html>\n");

    // Without matching directory index this should produce Forbidden response.
    let handler = make_handler(extended_conf("index_file: []"));

    let text = response_text(StatusCode::FORBIDDEN);
    let mut session = make_session("GET", "/").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 403);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn no_trailing_slash() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());
    let text = response_text(StatusCode::PERMANENT_REDIRECT);

    let mut session = make_session("GET", "/subdir?xyz").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 308);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
            ("location", "/subdir/?xyz"),
        ],
    );
    assert_body(&session, &text);

    // Add redirect prefix
    let handler = make_handler(extended_conf("redirect_prefix: /static"));

    let mut session = make_session("GET", "/subdir?xyz").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 308);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
            ("location", "/static/subdir/?xyz"),
        ],
    );
    assert_body(&session, &text);

    // Without canonicalize_uri this should just produce the response
    // (Forbidden because no index file).
    let handler = make_handler(extended_conf("canonicalize_uri: false"));

    let text = response_text(StatusCode::FORBIDDEN);
    let mut session = make_session("GET", "/subdir").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 403);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn unnecessary_percent_encoding() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());
    let text = response_text(StatusCode::PERMANENT_REDIRECT);

    let mut session = make_session("GET", "/file%2Etxt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 308);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
            ("location", "/file.txt"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn complex_path() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());
    let text = response_text(StatusCode::PERMANENT_REDIRECT);

    let mut session = make_session("GET", "/.//subdir/../file.txt?file%2Etxt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 308);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
            ("location", "/file.txt?file%2Etxt"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn utf8_path() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("subdir/файл söndärzeichen.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session(
        "GET",
        "/subdir/%D1%84%D0%B0%D0%B9%D0%BB%20s%C3%B6nd%C3%A4rzeichen.txt",
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    Ok(())
}

#[test(tokio::test)]
async fn no_file() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());
    let text = response_text(StatusCode::NOT_FOUND);

    let mut session = make_session("GET", "/missing.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 404);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn no_file_with_page_404() -> Result<(), Box<Error>> {
    let handler = make_handler(extended_conf("page_404: /file.txt"));

    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut session = make_session("GET", "/missing.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 404);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    Ok(())
}

#[test(tokio::test)]
async fn no_index() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());

    let text = response_text(StatusCode::FORBIDDEN);
    let mut session = make_session("GET", "/subdir/").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 403);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn wrong_method() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());

    let text = response_text(StatusCode::METHOD_NOT_ALLOWED);
    let mut session = make_session("POST", "/file.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 405);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn wrong_method_no_file() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());
    let text = response_text(StatusCode::NOT_FOUND);

    let mut session = make_session("POST", "/missing.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 404);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn head_request() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("HEAD", "/file.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", &meta.modified.unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    let text = response_text(StatusCode::NOT_FOUND);
    let mut session = make_session("HEAD", "/missing.txt").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 404);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, "");

    let text = response_text(StatusCode::PERMANENT_REDIRECT);
    let mut session = make_session("HEAD", "/subdir").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 308);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
            ("location", "/subdir/"),
        ],
    );
    assert_body(&session, "");

    Ok(())
}

#[test(tokio::test)]
async fn bad_request() -> Result<(), Box<Error>> {
    let handler = make_handler(default_conf());
    let text = response_text(StatusCode::BAD_REQUEST);

    let mut session = make_session("GET", ".").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 400);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    let mut session = make_session("GET", "/../").await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 400);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &text.len().to_string()),
            ("Content-Type", "text/html"),
        ],
    );
    assert_body(&session, &text);

    Ok(())
}

#[test(tokio::test)]
async fn if_none_match() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", &meta.etag)?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 304);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", "*")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 304);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", &format!("\"xyz\", {}", &meta.etag))?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 304);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", &meta.etag)?;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GMT")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 304);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", "\"xyz\"")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    // With compression enabled this should produce Vary header
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-None-Match", &meta.etag)?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 304);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&session, "");

    Ok(())
}

#[test(tokio::test)]
async fn if_match() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", &meta.etag)?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session.req_header_mut().insert_header("If-Match", "*")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", &format!("\"xyz\", {}", &meta.etag))?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", &meta.etag)?;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", "Thu, 01 Jan 1970 00:00:00 GTM")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 412);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    // With compression enabled this should produce Vary header
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 412);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&session, "");

    Ok(())
}

#[test(tokio::test)]
async fn if_modified_since() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", meta.modified.as_ref().unwrap())?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 304);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GTM")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", meta.modified.as_ref().unwrap())?;
    session
        .req_header_mut()
        .insert_header("If-None-Match", "\"xyz\"")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    // With compression enabled this should produce Vary header
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Modified-Since", meta.modified.as_ref().unwrap())?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 304);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&session, "");

    Ok(())
}

#[test(tokio::test)]
async fn if_unmodified_since() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", meta.modified.as_ref().unwrap())?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "Hi!\n");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", "Thu, 01 Jan 1970 00:00:00 GMT")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 412);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", meta.modified.as_ref().unwrap())?;
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 412);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    // With compression enabled this should produce Vary header
    let mut session = make_session("GET", "/file.txt").await;
    session
        .req_header_mut()
        .insert_header("If-Unmodified-Since", meta.modified.as_ref().unwrap())?;
    session
        .req_header_mut()
        .insert_header("If-Match", "\"xyz\"")?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 412);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&session, "");

    Ok(())
}

#[test(tokio::test)]
async fn ranged_request() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();

    let handler = make_handler(default_conf());
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=2-5")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 206);
    assert_headers(
        &session,
        vec![
            ("Content-Length", "4"),
            ("content-range", "bytes 2-5/100001"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "2345");

    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=99999-")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 206);
    assert_headers(
        &session,
        vec![
            ("Content-Length", "2"),
            ("content-range", "bytes 99999-100000/100001"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "9\n");

    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=-5")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 206);
    assert_headers(
        &session,
        vec![
            ("Content-Length", "5"),
            ("content-range", "bytes 99996-100000/100001"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "6789\n");

    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=200000-")?;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 416);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
        ],
    );
    assert_body(&session, "");

    // With compression enabled this should produce Vary header
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=200000-")?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 416);
    assert_headers(
        &session,
        vec![
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );
    assert_body(&session, "");

    Ok(())
}

#[test(tokio::test)]
async fn dynamic_compression() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();
    let handler = make_handler(default_conf());

    // Regular request should result in compressed response
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "gzip")?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Encoding", "gzip"),
            ("accept-ranges", "none"),
            ("Content-Type", "text/plain"),
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
        .insert_header("Accept-Encoding", "unsupported")?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );

    // Ranged response should be uncompressed
    let mut session = make_session("GET", "/large.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "gzip")?;
    session
        .req_header_mut()
        .insert_header("Range", "bytes=0-10000")?;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );
    assert_status(&session, 206);
    assert_headers(
        &session,
        vec![
            ("Content-Length", "10001"),
            ("content-range", "bytes 0-10000/100001"),
            ("Content-Type", "text/plain"),
            ("last-modified", meta.modified.as_ref().unwrap()),
            ("etag", &meta.etag),
            ("vary", "Accept-Encoding"),
        ],
    );

    Ok(())
}

#[test(tokio::test)]
async fn static_compression() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("large_precompressed.txt"), None).unwrap();
    let meta_compressed =
        Metadata::from_path(&root_path("large_precompressed.txt.gz"), None).unwrap();
    let handler = make_handler(extended_conf("precompressed: [gz, br]"));

    // Regular request should result in compressed response
    let mut session = make_session("GET", "/large_precompressed.txt").await;
    session
        .req_header_mut()
        .insert_header("Accept-Encoding", "br, gzip")
        .unwrap();

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta_compressed.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
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

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta_compressed.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
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

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    assert_status(&session, 200);
    assert_headers(
        &session,
        vec![
            ("Content-Length", &meta.size.to_string()),
            ("accept-ranges", "bytes"),
            ("Content-Type", "text/plain"),
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

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    assert_status(&session, 206);
    assert_headers(
        &session,
        vec![
            ("Content-Length", "11"),
            (
                "content-range",
                &format!("bytes 0-10/{}", meta_compressed.size),
            ),
            ("Content-Type", "text/plain"),
            ("last-modified", meta_compressed.modified.as_ref().unwrap()),
            ("etag", &meta_compressed.etag),
            ("Content-Encoding", "gzip"),
            ("vary", "Accept-Encoding"),
        ],
    );

    Ok(())
}
