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

//! Handles writing logs on a separate thread

use chrono::{DateTime, Local};
use http::HeaderValue;
use log::error;
use pandora_module_utils::pingora::SocketAddr;
use std::collections::HashMap;
use std::fs::File;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc::Receiver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LogToken {
    None,
    RemoteAddr(SocketAddr),
    RemotePort(SocketAddr),
    RemoteName(String),
    TimeLocal,
    TimeISO,
    Request(String),
    Status(u16),
    BytesSent(usize),
    ProcessingTime(Duration),
    Header(HeaderValue),
}

#[derive(Debug)]
pub(crate) struct LogData {
    time: SystemTime,
    log_file: PathBuf,
    tokens: Vec<LogToken>,
}

#[derive(Debug)]
pub(crate) enum WriterMessage {
    Reopen,
    LogData(LogData),
}

impl WriterMessage {
    pub(crate) fn log_data(time: SystemTime, log_file: &Path, tokens: Vec<LogToken>) -> Self {
        Self::LogData(LogData {
            time,
            log_file: log_file.to_owned(),
            tokens,
        })
    }
}

fn open_file(path: &PathBuf) -> Box<dyn Write + Send> {
    if path.as_os_str() != "-" {
        match File::options().append(true).create(true).open(path) {
            Ok(file) => return Box::new(file),
            Err(err) => {
                error!(
                    "Failed opening log file {} (cause: {err}), falling back to stdout",
                    path.as_os_str().to_string_lossy()
                );
            }
        }
    }
    Box::new(stdout())
}

fn write_escaped(buf: &mut Vec<u8>, data: impl AsRef<[u8]>) -> Result<(), std::io::Error> {
    fn is_allowed(byte: u8) -> bool {
        (b' '..=b'~').contains(&byte) && byte != b'"' && byte != b'\\'
    }

    buf.push(b'"');
    for byte in data.as_ref() {
        if is_allowed(*byte) {
            buf.push(*byte);
        } else {
            let _ = write!(buf, "\\x{byte:02x}");
        }
    }
    buf.push(b'"');

    Ok(())
}

fn stringify_data(buf: &mut Vec<u8>, time: SystemTime, tokens: Vec<LogToken>) {
    buf.truncate(0);

    for token in tokens {
        if !buf.is_empty() {
            let _ = write!(buf, " ");
        }
        let _ = match token {
            LogToken::None => write!(buf, "-"),
            LogToken::RemoteAddr(SocketAddr::Inet(addr)) => {
                write!(buf, "{}", addr.ip())
            }
            LogToken::RemoteAddr(SocketAddr::Unix(addr)) => {
                if let Some(path) = addr.as_pathname().and_then(|p| p.as_os_str().to_str()) {
                    write!(buf, "{path}")
                } else {
                    write!(buf, "-")
                }
            }
            LogToken::RemotePort(SocketAddr::Inet(addr)) => {
                write!(buf, "{}", addr.port())
            }
            LogToken::RemotePort(SocketAddr::Unix(_)) => write!(buf, "-"),
            LogToken::RemoteName(remote_name) => write_escaped(buf, remote_name),
            LogToken::TimeLocal => {
                let time = DateTime::<Local>::from(time).format("%d/%b/%Y:%H:%M:%S %z");
                write!(buf, "[{time}]")
            }
            LogToken::TimeISO => {
                let time = DateTime::<Local>::from(time).to_rfc3339();
                write!(buf, "[{time}]")
            }
            LogToken::Request(request) => write_escaped(buf, request),
            LogToken::Status(status) => write!(buf, "{status}"),
            LogToken::BytesSent(bytes) => write!(buf, "{bytes}"),
            LogToken::ProcessingTime(time) => {
                write!(buf, "{:.3}", time.as_secs_f32() * 1000.0)
            }
            LogToken::Header(value) => write_escaped(buf, value),
        };
    }
    let _ = writeln!(buf);
}

pub(crate) async fn log_writer(mut receiver: Receiver<WriterMessage>) {
    let mut files = HashMap::new();

    let mut buf = Vec::<u8>::with_capacity(4096);

    while let Some(data) = receiver.recv().await {
        match data {
            WriterMessage::Reopen => {
                files = HashMap::new();
            }
            WriterMessage::LogData(data) => {
                stringify_data(&mut buf, data.time, data.tokens);
                let writer = files.entry(data.log_file).or_insert_with_key(open_file);
                let _ = writer.write_all(&buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        let mut buf = Vec::<u8>::new();
        let _ = write_escaped(&mut buf, b"abcd");
        assert_eq!(&buf, b"\"abcd\"");

        buf.truncate(0);
        let _ = write_escaped(&mut buf, b"\0ab\"\\+-=! cd");
        assert_eq!(&buf, b"\"\\x00ab\\x22\\x5c+-=! cd\"");

        buf.truncate(0);
        let _ = write_escaped(&mut buf, b"ab~\x7f\x80\xfe\xffcd");
        assert_eq!(&buf, b"\"ab~\\x7f\\x80\\xfe\\xffcd\"");
    }

    #[test]
    fn tokens_to_string() {
        std::env::set_var("TZ", "UTC+1");

        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1716979999); // 2024-05-29 10:53:19 UTC
        let tokens = vec![
            LogToken::RemoteAddr(SocketAddr::Inet("127.0.0.1:8080".parse().unwrap())),
            LogToken::None,
            LogToken::RemoteName("me".to_owned()),
            LogToken::TimeLocal,
            LogToken::Request("GET /test\n/\" HTTP/1.1".into()),
            LogToken::Status(200),
            LogToken::BytesSent(876),
            LogToken::Header("https://example.com/".try_into().unwrap()),
            LogToken::Header(
                b"Mozilla/1.0 \\\"invalid data\x80"
                    .as_ref()
                    .try_into()
                    .unwrap(),
            ),
            LogToken::ProcessingTime(Duration::from_nanos(1234567)),
            LogToken::RemotePort(SocketAddr::Inet("127.0.0.1:8080".parse().unwrap())),
            LogToken::TimeISO,
        ];

        let mut buf = Vec::new();
        stringify_data(&mut buf, time, tokens);
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "127.0.0.1 - \"me\" [29/May/2024:09:53:19 -0100] \"GET /test\\x0a/\\x22 HTTP/1.1\" 200 876 \"https://example.com/\" \"Mozilla/1.0 \\x5c\\x22invalid data\\x80\" 1.235 8080 [2024-05-29T09:53:19-01:00]\n"
        );
    }
}
