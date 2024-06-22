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

//! Structures handling command line options and YAML deserialization for the Common Log Module

use http::HeaderName;
use module_utils::DeserializeMap;
use serde::Deserialize;
use std::path::PathBuf;
use structopt::StructOpt;

/// Command line options of the common log module
#[derive(Debug, Default, StructOpt)]
pub struct CommonLogOpt {
    /// Access log file path
    ///
    /// Special values are an empty string (disable logging) and - (write to standard output).
    #[structopt(long, parse(from_os_str))]
    pub log_file: Option<PathBuf>,
}

/// An individual log field
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum LogField {
    /// Skipped field, `-` in config file
    None,
    /// Client address, `remote_addr` in config file
    RemoteAddr,
    /// Client port, `remote_port` in config file
    RemotePort,
    /// Local time in the Common Log Format, `time_local` in config file
    TimeLocal,
    /// Local time in the ISO 8601 format, `time_iso8601` in config file
    TimeISO,
    /// Request line like `"GET / HTTP/1.1"`, `request` in config file
    Request,
    /// Numeric response status code, `status` in config file
    Status,
    /// Number of bytes sent as response, `bytes_sent` in config file
    BytesSent,
    /// Time it took to process the request, `processing_time` in config file
    ProcessingTime,
    /// A request header, `http_<header>` in config file
    RequestHeader(HeaderName),
    /// A response header, `sent_http_<header>` in config file
    ResponseHeader(HeaderName),
}

impl TryFrom<&str> for LogField {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "-" => Ok(Self::None),
            "remote_addr" => Ok(Self::RemoteAddr),
            "remote_port" => Ok(Self::RemotePort),
            "time_local" => Ok(Self::TimeLocal),
            "time_iso8601" => Ok(Self::TimeISO),
            "request" => Ok(Self::Request),
            "status" => Ok(Self::Status),
            "bytes_sent" => Ok(Self::BytesSent),
            "processing_time" => Ok(Self::ProcessingTime),
            name => {
                if let Some(header) = name.strip_prefix("http_") {
                    let header = header.replace('_', "-");
                    Ok(Self::RequestHeader(
                        HeaderName::try_from(header).map_err(|err| err.to_string())?,
                    ))
                } else if let Some(header) = name.strip_prefix("sent_http_") {
                    let header = header.replace('_', "-");
                    Ok(Self::ResponseHeader(
                        HeaderName::try_from(header).map_err(|err| err.to_string())?,
                    ))
                } else {
                    Err(format!("Unsupported log field {name}"))
                }
            }
        }
    }
}

impl TryFrom<String> for LogField {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.as_str().try_into()
    }
}

/// Configuration settings of the common log module
#[derive(Debug, Clone, PartialEq, Eq, DeserializeMap)]
pub struct CommonLogConf {
    /// Access log file path
    ///
    /// Special values are an empty string (disable logging) and - (write to standard output).
    pub log_file: PathBuf,
    /// List of fields to be logged
    ///
    /// See [`LogField`] for a list of supported values. The default log format is:
    ///
    /// ```yaml
    /// [remote_addr, -, -, time_local, request, status, bytes_sent, http_referer, http_user_agent]
    /// ```
    pub log_format: Vec<LogField>,
}

impl Default for CommonLogConf {
    fn default() -> Self {
        Self {
            log_file: PathBuf::from("-"),
            log_format: Vec::new(),
        }
    }
}

impl CommonLogConf {
    /// Merges the command line options into the current configuration. Any command line options
    /// present overwrite existing settings.
    pub fn merge_with_opt(&mut self, opt: CommonLogOpt) {
        if let Some(log_file) = opt.log_file {
            self.log_file = log_file;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use http::header;

    #[test]
    fn log_field_parsing() {
        let log_fields: Vec<_> = "remote_addr - - time_local request status bytes_sent http_referer http_user_agent processing_time sent_http_content_type remote_port time_iso8601".split_ascii_whitespace().map(|s| {
            LogField::try_from(s).unwrap()
        }).collect();
        assert_eq!(
            log_fields,
            vec![
                LogField::RemoteAddr,
                LogField::None,
                LogField::None,
                LogField::TimeLocal,
                LogField::Request,
                LogField::Status,
                LogField::BytesSent,
                LogField::RequestHeader(header::REFERER),
                LogField::RequestHeader(header::USER_AGENT),
                LogField::ProcessingTime,
                LogField::ResponseHeader(header::CONTENT_TYPE),
                LogField::RemotePort,
                LogField::TimeISO,
            ]
        );
        assert!(LogField::try_from("unsupported_field").is_err());
    }
}
