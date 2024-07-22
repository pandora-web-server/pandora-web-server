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
use log::trace;
use pandora_module_utils::pingora::{
    CompressionAlgorithm, Error, HttpModules, ResponseCompression, ResponseCompressionBuilder,
    SessionWrapper,
};
use pandora_module_utils::{DeserializeMap, RequestFilter};

/// Command line options of the compression module
#[derive(Debug, Default, Parser)]
pub struct CompressionOpt {
    /// Compression level to be used for dynamic gzip compression (omit to disable compression)
    #[clap(long)]
    pub compression_level_gzip: Option<u32>,

    /// Compression level to be used for dynamic Brotli compression (omit to disable compression)
    #[clap(long)]
    pub compression_level_brotli: Option<u32>,

    /// Compression level to be used for dynamic Zstandard compression (omit to disable
    /// compression)
    #[clap(long)]
    pub compression_level_zstd: Option<u32>,

    /// Decompress upstream responses before passing them on
    #[clap(long)]
    pub decompress_upstream: bool,
}

/// Configuration settings of the compression module
#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
pub struct CompressionConf {
    /// Compression level to be used for dynamic gzip compression (omit to disable compression).
    pub compression_level_gzip: Option<u32>,

    /// Compression level to be used for dynamic Brotli compression (omit to disable compression).
    pub compression_level_brotli: Option<u32>,

    /// Compression level to be used for dynamic Zstandard compression (omit to disable compression).
    pub compression_level_zstd: Option<u32>,

    /// If `true`, upstream responses will be decompressed
    pub decompress_upstream: bool,
}

impl CompressionConf {
    /// Merges the command line options into the current configuration. Any command line options
    /// present overwrite existing settings.
    pub fn merge_with_opt(&mut self, opt: CompressionOpt) {
        if opt.compression_level_gzip.is_some() {
            self.compression_level_gzip = opt.compression_level_gzip;
        }

        if opt.compression_level_brotli.is_some() {
            self.compression_level_brotli = opt.compression_level_brotli;
        }

        if opt.compression_level_zstd.is_some() {
            self.compression_level_zstd = opt.compression_level_zstd;
        }

        if opt.decompress_upstream {
            self.decompress_upstream = opt.decompress_upstream;
        }
    }
}

/// Compression module handler
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

    fn init_downstream_modules(modules: &mut HttpModules) {
        modules.add_module(ResponseCompressionBuilder::enable(0));
    }

    async fn early_request_filter(
        &self,
        session: &mut impl SessionWrapper,
        _ctx: &mut Self::CTX,
    ) -> Result<(), Box<Error>> {
        macro_rules! enable_compression {
            ($pref:ident => $algorithm:ident) => {
                if let Some(level) = self.conf.$pref {
                    trace!(
                        concat!(
                            "Enabled ",
                            stringify!($algorithm),
                            " compression with compression level {}"
                        ),
                        level
                    );
                    session
                        .downstream_modules_ctx
                        .get_mut::<ResponseCompression>()
                        .unwrap()
                        .adjust_algorithm_level(CompressionAlgorithm::$algorithm, level);
                }
            };
        }

        enable_compression!(compression_level_gzip => Gzip);
        enable_compression!(compression_level_brotli => Brotli);
        enable_compression!(compression_level_zstd => Zstd);

        if self.conf.decompress_upstream {
            session.upstream_compression.adjust_decompression(true);
        }

        Ok(())
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
                    compression_level_gzip: 6
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
        assert_eq!(
            session
                .downstream_modules_ctx
                .get::<ResponseCompression>()
                .is_some_and(|compression| compression.is_enabled()),
            downstream
        );
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
