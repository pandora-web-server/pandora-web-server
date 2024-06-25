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

//! Handler for Pingora’s `request_filter` and `logging` phases

use async_trait::async_trait;
use http::header;
use log::error;
use module_utils::pingora::{Error, ErrorType, SessionWrapper};
use module_utils::{RequestFilter, RequestFilterResult};
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::mpsc::{channel, Sender};

use crate::configuration::{CommonLogConf, LogField};
use crate::writer::{log_writer, LogToken, WriterMessage};

fn normalize_path(path: PathBuf) -> Result<PathBuf, Box<Error>> {
    if path.as_os_str().is_empty() || path.as_os_str() == "-" {
        // Don't change special paths
        return Ok(path);
    }

    if let Some(parent) = path.parent() {
        let mut parent = if parent.as_os_str().is_empty() {
            PathBuf::from(".").canonicalize()
        } else {
            parent.canonicalize()
        }
        .map_err(|err| {
            Error::because(
                ErrorType::FileOpenError,
                "failed resolving log file's parent directory",
                err,
            )
        })?;
        if let Some(name) = path.file_name() {
            parent.push(name);
        }
        Ok(parent)
    } else {
        // Absolute path in the root, leave unchanged
        Ok(path)
    }
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonLogHandler {
    conf: CommonLogConf,
}

impl TryFrom<CommonLogConf> for CommonLogHandler {
    type Error = Box<Error>;

    fn try_from(mut conf: CommonLogConf) -> Result<Self, Self::Error> {
        // Normalize parent directory in case the same file is specified with different paths
        conf.log_file = normalize_path(conf.log_file)?;

        // If no log format specified, use default
        if conf.log_format.is_empty() {
            conf.log_format = vec![
                LogField::RemoteAddr,
                LogField::None,
                LogField::None,
                LogField::TimeLocal,
                LogField::Request,
                LogField::Status,
                LogField::BytesSent,
                LogField::RequestHeader(header::REFERER),
                LogField::RequestHeader(header::USER_AGENT),
            ]
            .into();
        }

        Ok(Self { conf })
    }
}

/// Context data for the log module
#[derive(Debug)]
pub struct RequestCtx {
    time: SystemTime,
    tokens: Vec<LogToken>,
}

#[async_trait]
impl RequestFilter for CommonLogHandler {
    type Conf = CommonLogConf;
    type CTX = RequestCtx;
    fn new_ctx() -> Self::CTX {
        RequestCtx {
            time: SystemTime::now(),
            tokens: Vec::new(),
        }
    }

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        if self.conf.log_file.as_os_str().is_empty() {
            // Logging disabled
            return Ok(RequestFilterResult::Unhandled);
        }

        for field in &self.conf.log_format {
            ctx.tokens.push(match field {
                LogField::None => LogToken::None,
                LogField::RemoteAddr => {
                    if let Some(client_addr) = session.client_addr() {
                        LogToken::RemoteAddr(client_addr.clone())
                    } else {
                        LogToken::None
                    }
                }
                LogField::RemotePort => {
                    if let Some(client_addr) = session.client_addr() {
                        LogToken::RemotePort(client_addr.clone())
                    } else {
                        LogToken::None
                    }
                }
                LogField::TimeLocal => LogToken::TimeLocal,
                LogField::TimeISO => LogToken::TimeISO,
                LogField::Request => {
                    let header = session.req_header();
                    let method = &header.method;

                    // Try getting URI from extensions first, virtual-hosts-module will store
                    // original URI there before stripping prefix.
                    let uri = session
                        .extensions()
                        .get()
                        .unwrap_or(&header.uri)
                        .path_and_query()
                        .map(|p| p.as_str())
                        .unwrap_or("");
                    let version = &header.version;
                    LogToken::Request(format!("{method} {uri} {version:?}"))
                }
                LogField::RequestHeader(name) => {
                    if let Some(value) = session.req_header().headers.get(name) {
                        LogToken::Header(value.clone())
                    } else {
                        LogToken::None
                    }
                }
                LogField::Status
                | LogField::BytesSent
                | LogField::ProcessingTime
                | LogField::ResponseHeader(_) => continue,
            });
        }

        Ok(RequestFilterResult::Unhandled)
    }

    async fn logging(
        &self,
        session: &mut impl SessionWrapper,
        _e: Option<&Error>,
        ctx: &mut RequestCtx,
    ) {
        if self.conf.log_file.as_os_str().is_empty() {
            // Logging disabled
            return;
        }

        let mut existing_tokens = ctx.tokens.split_off(0).into_iter();
        let mut tokens = Vec::new();

        for field in &self.conf.log_format {
            tokens.push(match field {
                LogField::None
                | LogField::RemoteAddr
                | LogField::RemotePort
                | LogField::TimeLocal
                | LogField::TimeISO
                | LogField::Request
                | LogField::RequestHeader(_) => {
                    // This is a token we’ve added previously. Panic if we don’t have one, it’s
                    // a bug that needs investigating.
                    existing_tokens.next().unwrap()
                }
                LogField::Status => {
                    if let Some(header) = session.response_written() {
                        LogToken::Status(header.status.as_u16())
                    } else {
                        LogToken::None
                    }
                }
                LogField::BytesSent => LogToken::BytesSent(session.body_bytes_sent()),
                LogField::ProcessingTime => {
                    if let Ok(time) = SystemTime::now().duration_since(ctx.time) {
                        LogToken::ProcessingTime(time)
                    } else {
                        LogToken::None
                    }
                }
                LogField::ResponseHeader(name) => {
                    if let Some(value) =
                        session.response_written().and_then(|h| h.headers.get(name))
                    {
                        LogToken::Header(value.clone())
                    } else {
                        LogToken::None
                    }
                }
            });
        }

        static LOG_SENDER: Lazy<Arc<Sender<WriterMessage>>> = Lazy::new(|| {
            let (sender, receiver) = channel(100);

            tokio::spawn(async move { log_writer(receiver).await });

            #[cfg(unix)]
            crate::signal::listen(&sender);

            Arc::new(sender)
        });

        let message = WriterMessage::log_data(ctx.time, &self.conf.log_file, tokens);
        if let Err(err) = Arc::make_mut(&mut (*LOG_SENDER).clone())
            .send(message)
            .await
        {
            error!("Failed logging request, thread crashed? {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::env::current_dir;

    #[test]
    fn path_normalization() {
        let cwd = current_dir().unwrap().canonicalize().unwrap();
        let mut root = cwd.clone();
        while let Some(parent) = root.parent() {
            root = parent.into();
        }

        assert_eq!(normalize_path("".into()).unwrap(), PathBuf::from(""));
        assert_eq!(normalize_path("-".into()).unwrap(), PathBuf::from("-"));
        assert_eq!(
            normalize_path("file.txt".into()).unwrap(),
            cwd.join("file.txt")
        );
        assert_eq!(
            normalize_path("./file.txt".into()).unwrap(),
            cwd.join("file.txt")
        );
        assert_eq!(
            normalize_path("../file.txt".into()).unwrap(),
            cwd.parent().unwrap().join("file.txt")
        );
        assert_eq!(
            normalize_path(cwd.join("file.txt")).unwrap(),
            cwd.join("file.txt")
        );
        assert_eq!(
            normalize_path(root.join("file.txt")).unwrap(),
            root.join("file.txt")
        );
    }
}
