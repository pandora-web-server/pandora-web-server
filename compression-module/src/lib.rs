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
use pingora_core::modules::http::compression::ResponseCompression;

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
            if let Some(rc) = session
                .downstream_modules_ctx
                .get_mut::<ResponseCompression>()
            {
                // TODO: Warn if there is no response compression module?
                rc.adjust_level(level);
            }
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

    use pandora_module_utils::pingora::{RequestHeader, TestSession};
    use pandora_module_utils::FromYaml;
    use test_log::test;

    #[derive(Debug, Clone, PartialEq, Eq, DeserializeMap, Default)]
    struct TestConf {}

    #[derive(Debug)]
    struct TestHandler {}

    impl TryFrom<TestConf> for TestHandler {
        type Error = Box<Error>;

        fn try_from(_conf: TestConf) -> Result<Self, Self::Error> {
            Ok(TestHandler {})
        }
    }

    #[async_trait]
    impl RequestFilter for TestHandler {
        type Conf = TestConf;
        type CTX = ();
        fn new_ctx() -> Self::CTX {}

        async fn request_filter(
            &self,
            session: &mut impl SessionWrapper,
            _ctx: &mut Self::CTX,
        ) -> Result<RequestFilterResult, Box<Error>> {
            let downstream_enabled = session
                .downstream_modules_ctx
                .get::<ResponseCompression>()
                .is_some_and(|rc| rc.is_enabled());

            if downstream_enabled && session.upstream_compression.is_enabled() {
                Ok(RequestFilterResult::ResponseSent)
            } else if downstream_enabled || session.upstream_compression.is_enabled() {
                Ok(RequestFilterResult::Handled)
            } else {
                Ok(RequestFilterResult::Unhandled)
            }
        }
    }

    #[derive(Debug, RequestFilter)]
    struct Handler {
        compression: CompressionHandler,
        test: TestHandler,
    }

    fn make_handler(configured: bool) -> Handler {
        let conf = if configured {
            <Handler as RequestFilter>::Conf::from_yaml(
                r#"
                    compression_level: 6
                    decompress_upstream: true
                "#,
            )
            .unwrap()
        } else {
            <Handler as RequestFilter>::Conf::default()
        };
        conf.try_into().unwrap()
    }

    async fn make_session() -> TestSession {
        let header = RequestHeader::build("GET", b"/", None).unwrap();
        TestSession::from(header).await
    }

    #[test(tokio::test)]
    async fn unconfigured() -> Result<(), Box<Error>> {
        let handler = make_handler(false);
        let mut session = make_session().await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut Handler::new_ctx())
                .await?,
            RequestFilterResult::Unhandled
        );
        Ok(())
    }

    #[test(tokio::test)]
    #[ignore]
    async fn configured() -> Result<(), Box<Error>> {
        let handler = make_handler(true);
        let mut session = make_session().await;
        assert_eq!(
            handler
                .request_filter(&mut session, &mut Handler::new_ctx())
                .await?,
            RequestFilterResult::ResponseSent
        );
        Ok(())
    }
}
