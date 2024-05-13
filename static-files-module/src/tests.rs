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

use crate::compression_algorithm::CompressionAlgorithm;
use crate::configuration::StaticFilesConf;
use crate::handler::StaticFilesHandler;
use crate::metadata::Metadata;
use crate::standard_response::response_text;

use const_format::{concatcp, str_repeat};
use http::status::StatusCode;
use httpdate::fmt_http_date;
use module_utils::pingora::{Error, Session};
use module_utils::{RequestFilter, RequestFilterResult};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::SystemTime;
use test_log::test;
use tokio_test::io::Builder;

struct Request<'a> {
    method: &'a str,
    uri: &'a str,
    headers: Vec<(&'a str, String)>,
}

struct Response<'a> {
    expected_status: &'a str,
    expected_headers: Vec<(&'a str, String)>,
    expected_body: &'a str,
}

fn request<'a>(method: &'a str, uri: &'a str) -> Request<'a> {
    Request {
        method,
        uri,
        headers: Vec::new(),
    }
}

fn request_with_headers<'a>(
    method: &'a str,
    uri: &'a str,
    headers: Vec<(&'a str, String)>,
) -> Request<'a> {
    Request {
        method,
        uri,
        headers,
    }
}

fn response<'a>(
    expected_status: &'a str,
    expected_headers: Vec<(&'a str, String)>,
    expected_body: &'a str,
) -> Response<'a> {
    Response {
        expected_status,
        expected_headers,
        expected_body,
    }
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

fn handler() -> StaticFilesHandler {
    StaticFilesConf {
        root: Some(root_path("")),
        ..Default::default()
    }
    .try_into()
    .unwrap()
}

fn mock(request: Request<'_>) -> Builder {
    let mut mock = Builder::new();

    let method = request.method;
    let uri = request.uri;
    mock.read(format!("{method} {uri} HTTP/1.1\r\n").as_bytes());
    mock.read(b"Connection: close\r\n");
    for (name, value) in request.headers {
        mock.read(format!("{name}: {value}\r\n").as_bytes());
    }
    mock.read(b"\r\n");
    mock
}

async fn make_session_no_response(request: Request<'_>) -> Session {
    let mut mock = mock(request);

    let mut session = Session::new_h1(Box::new(mock.build()));
    assert!(session.read_request().await.unwrap());
    session
}

async fn make_session(request: Request<'_>, response: Response<'_>) -> Session {
    let mut mock = mock(request);

    let expected_status = response.expected_status;
    mock.write(format!("HTTP/1.1 {expected_status}\r\n").as_bytes());
    for (name, value) in response.expected_headers {
        mock.write(format!("{name}: {value}\r\n").as_bytes());
    }
    mock.write(format!("Date: {}\r\n", fmt_http_date(SystemTime::now())).as_bytes());
    mock.write(b"Connection: close\r\n");

    mock.write(b"\r\n");

    if response.expected_body == "<ignore>" {
        mock.write_error(ErrorKind::Other.into());
    } else {
        mock.write(response.expected_body.as_bytes());
    }

    let mut session = Session::new_h1(Box::new(mock.build()));
    assert!(session.read_request().await.unwrap());
    session
}

#[test(tokio::test)]
async fn unconfigured() -> Result<(), Box<Error>> {
    let mut handler = handler();
    handler.conf_mut().root = None;

    let mut session = make_session_no_response(request("GET", "/file.txt")).await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::Unhandled
    );

    Ok(())
}

#[test(tokio::test)]
async fn text_file() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request("GET", "/file.txt"),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.unwrap()),
                ("etag", meta.etag),
            ],
            "Hi!\n",
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();
    let mut session = make_session(
        request("GET", "/large.txt"),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.unwrap()),
                ("etag", meta.etag),
            ],
            concatcp!(str_repeat!("0123456789", 10000), "\n"),
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn dir_index() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("index.html"), None).unwrap();

    let mut handler = handler();
    let mut session = make_session(
        request("GET", "/"),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/html".into()),
                ("last-modified", meta.modified.unwrap()),
                ("etag", meta.etag),
            ],
            "<html>Hi!</html>\n",
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // Without matching directory index this should produce Forbidden response.
    handler.conf_mut().index_file = vec![];

    let text = response_text(StatusCode::FORBIDDEN);
    let mut session = make_session(
        request("GET", "/"),
        response(
            "403 Forbidden",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn no_trailing_slash() -> Result<(), Box<Error>> {
    let mut handler = handler();
    let text = response_text(StatusCode::PERMANENT_REDIRECT);
    let mut session = make_session(
        request("GET", "/subdir?xyz"),
        response(
            "308 Permanent Redirect",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
                ("location", "/subdir/?xyz".into()),
            ],
            &text,
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // Add redirect prefix
    handler.conf_mut().redirect_prefix = Some("/static".to_owned());

    let mut session = make_session(
        request("GET", "/subdir?xyz"),
        response(
            "308 Permanent Redirect",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
                ("location", "/static/subdir/?xyz".into()),
            ],
            &text,
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // Without canonicalize_uri this should just produce the response
    // (Forbidden because no index file).
    handler.conf_mut().canonicalize_uri = false;

    let text = response_text(StatusCode::FORBIDDEN);
    let mut session = make_session(
        request("GET", "/subdir"),
        response(
            "403 Forbidden",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn unnecessary_percent_encoding() -> Result<(), Box<Error>> {
    let handler = handler();
    let text = response_text(StatusCode::PERMANENT_REDIRECT);
    let mut session = make_session(
        request("GET", "/file%2Etxt"),
        response(
            "308 Permanent Redirect",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
                ("location", "/file.txt".into()),
            ],
            &text,
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn complex_path() -> Result<(), Box<Error>> {
    let handler = handler();
    let text = response_text(StatusCode::PERMANENT_REDIRECT);
    let mut session = make_session(
        request("GET", "/.//subdir/../file.txt?file%2Etxt"),
        response(
            "308 Permanent Redirect",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
                ("location", "/file.txt?file%2Etxt".into()),
            ],
            &text,
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn utf8_path() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("subdir/файл söndärzeichen.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request(
            "GET",
            "/subdir/%D1%84%D0%B0%D0%B9%D0%BB%20s%C3%B6nd%C3%A4rzeichen.txt",
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.unwrap()),
                ("etag", meta.etag),
            ],
            "Hi!\n",
        ),
    )
    .await;

    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn no_file() -> Result<(), Box<Error>> {
    let handler = handler();
    let text = response_text(StatusCode::NOT_FOUND);

    let mut session = make_session(
        request("GET", "/missing.txt"),
        response(
            "404 Not Found",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn no_file_with_page_404() -> Result<(), Box<Error>> {
    let mut handler = handler();
    handler.conf_mut().page_404 = Some("/file.txt".to_owned());

    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let mut session = make_session(
        request("GET", "/missing.txt"),
        response(
            "404 Not Found",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn no_index() -> Result<(), Box<Error>> {
    let handler = handler();

    let text = response_text(StatusCode::FORBIDDEN);
    let mut session = make_session(
        request("GET", "/subdir/"),
        response(
            "403 Forbidden",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn wrong_method() -> Result<(), Box<Error>> {
    let handler = handler();

    let text = response_text(StatusCode::METHOD_NOT_ALLOWED);
    let mut session = make_session(
        request("POST", "/file.txt"),
        response(
            "405 Method Not Allowed",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await.unwrap(),
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn wrong_method_no_file() -> Result<(), Box<Error>> {
    let handler = handler();
    let text = response_text(StatusCode::NOT_FOUND);

    let mut session = make_session(
        request("POST", "/missing.txt"),
        response(
            "404 Not Found",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn head_request() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request("HEAD", "/file.txt"),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.unwrap()),
                ("etag", meta.etag),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let text = response_text(StatusCode::NOT_FOUND);
    let mut session = make_session(
        request("HEAD", "/missing.txt"),
        response(
            "404 Not Found",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let text = response_text(StatusCode::PERMANENT_REDIRECT);
    let mut session = make_session(
        request("HEAD", "/subdir"),
        response(
            "308 Permanent Redirect",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
                ("location", "/subdir/".into()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn bad_request() -> Result<(), Box<Error>> {
    let handler = handler();
    let text = response_text(StatusCode::BAD_REQUEST);

    let mut session = make_session(
        request("GET", "."),
        response(
            "400 Bad Request",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request("GET", "/../"),
        response(
            "400 Bad Request",
            vec![
                ("Content-Length", text.len().to_string()),
                ("Content-Type", "text/html".into()),
            ],
            &text,
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn if_none_match() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-None-Match", meta.etag.clone())],
        ),
        response(
            "304 Not Modified",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers("GET", "/file.txt", vec![("If-None-Match", "*".into())]),
        response(
            "304 Not Modified",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-None-Match", format!("\"xyz\", {}", meta.etag.clone()))],
        ),
        response(
            "304 Not Modified",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![
                ("If-None-Match", meta.etag.clone()),
                ("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GMT".into()),
            ],
        ),
        response(
            "304 Not Modified",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-None-Match", "\"xyz\"".into())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // With compression enabled this should produce Vary header
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-None-Match", meta.etag.clone())],
        ),
        response(
            "304 Not Modified",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn if_match() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request_with_headers("GET", "/file.txt", vec![("If-Match", meta.etag.clone())]),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers("GET", "/file.txt", vec![("If-Match", "*".into())]),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-Match", format!("\"xyz\", {}", meta.etag.clone()))],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![
                ("If-Match", meta.etag.clone()),
                (
                    "If-Unmodified-Since",
                    "Thu, 01 Jan 1970 00:00:00 GTM".into(),
                ),
            ],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers("GET", "/file.txt", vec![("If-Match", "\"xyz\"".into())]),
        response(
            "412 Precondition Failed",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // With compression enabled this should produce Vary header
    let mut session = make_session(
        request_with_headers("GET", "/file.txt", vec![("If-Match", "\"xyz\"".into())]),
        response(
            "412 Precondition Failed",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn if_modified_since() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-Modified-Since", meta.modified.clone().unwrap())],
        ),
        response(
            "304 Not Modified",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GTM".into())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![
                ("If-Modified-Since", meta.modified.clone().unwrap()),
                ("If-None-Match", "\"xyz\"".into()),
            ],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // With compression enabled this should produce Vary header
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-Modified-Since", meta.modified.clone().unwrap())],
        ),
        response(
            "304 Not Modified",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn if_unmodified_since() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("file.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![("If-Unmodified-Since", meta.modified.clone().unwrap())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "Hi!\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![(
                "If-Unmodified-Since",
                "Thu, 01 Jan 1970 00:00:00 GMT".into(),
            )],
        ),
        response(
            "412 Precondition Failed",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![
                ("If-Unmodified-Since", meta.modified.clone().unwrap()),
                ("If-Match", "\"xyz\"".into()),
            ],
        ),
        response(
            "412 Precondition Failed",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // With compression enabled this should produce Vary header
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/file.txt",
            vec![
                ("If-Unmodified-Since", meta.modified.clone().unwrap()),
                ("If-Match", "\"xyz\"".into()),
            ],
        ),
        response(
            "412 Precondition Failed",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn ranged_request() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();

    let handler = handler();
    let mut session = make_session(
        request_with_headers("GET", "/large.txt", vec![("Range", "bytes=2-5".into())]),
        response(
            "206 Partial Content",
            vec![
                ("Content-Length", "4".into()),
                ("content-range", "bytes 2-5/100001".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "2345",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers("GET", "/large.txt", vec![("Range", "bytes=99999-".into())]),
        response(
            "206 Partial Content",
            vec![
                ("Content-Length", "2".into()),
                ("content-range", "bytes 99999-100000/100001".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "9\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers("GET", "/large.txt", vec![("Range", "bytes=-5".into())]),
        response(
            "206 Partial Content",
            vec![
                ("Content-Length", "5".into()),
                ("content-range", "bytes 99996-100000/100001".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "6789\n",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    let mut session = make_session(
        request_with_headers("GET", "/large.txt", vec![("Range", "bytes=200000-".into())]),
        response(
            "416 Range Not Satisfiable",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
            ],
            "",
        ),
    )
    .await;
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    // With compression enabled this should produce Vary header
    let mut session = make_session(
        request_with_headers("GET", "/large.txt", vec![("Range", "bytes=200000-".into())]),
        response(
            "416 Range Not Satisfiable",
            vec![
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    assert_eq!(
        handler.request_filter(&mut session, &mut ()).await?,
        RequestFilterResult::ResponseSent
    );

    Ok(())
}

#[test(tokio::test)]
async fn dynamic_compression() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("large.txt"), None).unwrap();
    let handler = handler();

    // Regular request should result in compressed response
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/large.txt",
            vec![("Accept-Encoding", "gzip".into())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Encoding", "gzip".into()),
                ("accept-ranges", "none".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("Transfer-Encoding", "chunked".into()),
                ("vary", "Accept-Encoding".into()),
            ],
            "<ignore>",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    handler
        .request_filter(&mut session, &mut ())
        .await
        .expect_err("Writing response body should error out");

    // Request without matching encodings should result in uncompressed response
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/large.txt",
            vec![("Accept-Encoding", "unsupported".into())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "<ignore>",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    handler
        .request_filter(&mut session, &mut ())
        .await
        .expect_err("Writing response body should error out");

    // Ranged response should be uncompressed
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/large.txt",
            vec![
                ("Accept-Encoding", "gzip".into()),
                ("Range", "bytes=0-10000".into()),
            ],
        ),
        response(
            "206 Partial Content",
            vec![
                ("Content-Length", "10001".into()),
                ("content-range", "bytes 0-10000/100001".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "<ignore>",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    handler
        .request_filter(&mut session, &mut ())
        .await
        .expect_err("Writing response body should error out");

    Ok(())
}

#[test(tokio::test)]
async fn static_compression() -> Result<(), Box<Error>> {
    let meta = Metadata::from_path(&root_path("large_precompressed.txt"), None).unwrap();
    let meta_compressed =
        Metadata::from_path(&root_path("large_precompressed.txt.gz"), None).unwrap();
    let mut handler = handler();
    handler.conf_mut().precompressed =
        vec![CompressionAlgorithm::Gzip, CompressionAlgorithm::Brotli];

    // Regular request should result in compressed response
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/large_precompressed.txt",
            vec![("Accept-Encoding", "br, gzip".into())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta_compressed.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta_compressed.modified.clone().unwrap()),
                ("etag", meta_compressed.etag.clone()),
                ("Content-Encoding", "gzip".into()),
                ("vary", "Accept-Encoding".into()),
            ],
            "<ignore>",
        ),
    )
    .await;
    handler
        .request_filter(&mut session, &mut ())
        .await
        .expect_err("Writing response body should error out");

    // Static compression should take precedence over dynamic
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/large_precompressed.txt",
            vec![("Accept-Encoding", "br, gzip".into())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta_compressed.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta_compressed.modified.clone().unwrap()),
                ("etag", meta_compressed.etag.clone()),
                ("Content-Encoding", "gzip".into()),
                ("vary", "Accept-Encoding".into()),
            ],
            "<ignore>",
        ),
    )
    .await;
    session.downstream_compression.adjust_level(3);
    handler
        .request_filter(&mut session, &mut ())
        .await
        .expect_err("Writing response body should error out");

    // Request without matching encodings should result in uncompressed response
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/large_precompressed.txt",
            vec![("Accept-Encoding", "zstd".into())],
        ),
        response(
            "200 OK",
            vec![
                ("Content-Length", meta.size.to_string()),
                ("accept-ranges", "bytes".into()),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta.modified.clone().unwrap()),
                ("etag", meta.etag.clone()),
                ("vary", "Accept-Encoding".into()),
            ],
            "<ignore>",
        ),
    )
    .await;
    handler
        .request_filter(&mut session, &mut ())
        .await
        .expect_err("Writing response body should error out");

    // Ranged response should be compressed
    let mut session = make_session(
        request_with_headers(
            "GET",
            "/large_precompressed.txt",
            vec![
                ("Accept-Encoding", "gzip".into()),
                ("Range", "bytes=0-10".into()),
            ],
        ),
        response(
            "206 Partial Content",
            vec![
                ("Content-Length", "11".into()),
                (
                    "content-range",
                    format!("bytes 0-10/{}", meta_compressed.size),
                ),
                ("Content-Type", "text/plain".into()),
                ("last-modified", meta_compressed.modified.clone().unwrap()),
                ("etag", meta_compressed.etag.clone()),
                ("Content-Encoding", "gzip".into()),
                ("vary", "Accept-Encoding".into()),
            ],
            "<ignore>",
        ),
    )
    .await;
    handler
        .request_filter(&mut session, &mut ())
        .await
        .expect_err("Writing response body should error out");

    Ok(())
}
