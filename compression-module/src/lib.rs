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

#![doc = include_str!("../README.md")]

use async_trait::async_trait;
use clap::Parser;
use pandora_module_utils::pingora::{Error, SessionWrapper};
use pandora_module_utils::{DeserializeMap, RequestFilter, RequestFilterResult};

/// Command line options of the compression module
#[derive(Debug, Default, Parser)]
pub struct CompressionOpt {
    /// Compression level to be used for dynamic compression (omit to disable compression)
    #[clap(long)]
    pub compression_level: Option<u32>,

    /// Decompress upstream responses before passing them on
    #[clap(long)]
    pub decompress_upstream: bool,
}

/// Configuration settings of the compression module
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct CompressionConf {
    /// Compression level to be used for dynamic compression (omit to disable compression).
    pub compression_level: Option<u32>,

    /// If `true`, upstream responses will be decompressed
    pub decompress_upstream: bool,
}

impl CompressionConf {
    /// Merges the command line options into the current configuration. Any command line options
    /// present overwrite existing settings.
    pub fn merge_with_opt(&mut self, opt: CompressionOpt) {
        if opt.compression_level.is_some() {
            self.compression_level = opt.compression_level;
        }

        if opt.decompress_upstream {
            self.decompress_upstream = opt.decompress_upstream;
        }
    }
}

/// Handler for Pingora’s `request_filter` phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionHandler {
    conf: CompressionConf,
}

impl TryFrom<CompressionConf> for CompressionHandler {
    type Error = Box<Error>;

    fn try_from(conf: CompressionConf) -> Result<Self, Self::Error> {
        Ok(Self { conf })
    }
}

#[async_trait]
impl RequestFilter for CompressionHandler {
    type Conf = CompressionConf;
    type CTX = ();
    fn new_ctx() -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        if let Some(level) = self.conf.compression_level {
            session.downstream_compression.adjust_level(level);
        }

        if self.conf.decompress_upstream {
            session.upstream_compression.adjust_decompression(true);
        }

        Ok(RequestFilterResult::Unhandled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pandora_module_utils::pingora::{create_test_session, RequestHeader, Session};
    use pandora_module_utils::FromYaml;
    use startup_module::{AppResult, DefaultApp};
    use test_log::test;

    fn make_app(configured: bool) -> DefaultApp<CompressionHandler> {
        let conf = if configured {
            <CompressionHandler as RequestFilter>::Conf::from_yaml(
                r#"
                    compression_level: 6
                    decompress_upstream: true
                "#,
            )
            .unwrap()
        } else {
            <CompressionHandler as RequestFilter>::Conf::default()
        };
        DefaultApp::new(conf.try_into().unwrap())
    }

    async fn make_session() -> Session {
        let header = RequestHeader::build("GET", b"/", None).unwrap();
        create_test_session(header).await
    }

    fn assert_compression(result: &mut AppResult, downstream: bool, upstream: bool) {
        let session = result.session();
        assert_eq!(session.downstream_compression.is_enabled(), downstream);
        assert_eq!(session.upstream_compression.is_enabled(), upstream);
    }

    #[test(tokio::test)]
    async fn unconfigured() {
        let mut app = make_app(false);
        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert_compression(&mut result, false, false);
    }

    #[test(tokio::test)]
    async fn configured() {
        let mut app = make_app(true);
        let session = make_session().await;
        let mut result = app.handle_request(session).await;
        assert_compression(&mut result, true, true);
    }
}
