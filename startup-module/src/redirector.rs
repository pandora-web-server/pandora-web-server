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

use async_trait::async_trait;
use http::{header, Method, StatusCode};
use module_utils::pingora::{
    Error, ErrorType, HttpPeer, ProxyHttp, ResponseHeader, ServerConf, Session,
};
use module_utils::standard_response::response_text;
use pingora::{proxy::http_proxy_service, services::Service};
use std::collections::HashMap;
use std::sync::Arc;

use crate::configuration::{TlsRedirectorConf, TLS_CONF_ERR};

struct RedirectorApp {
    redirect_to: String,
    redirect_by_name: HashMap<String, String>,
}

#[async_trait]
impl ProxyHttp for RedirectorApp {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        let status = StatusCode::PERMANENT_REDIRECT;
        let text = response_text(status);

        let server_name = session
            .get_header(header::HOST)
            .and_then(|host| host.to_str().ok())
            .map(|host| host.split_once(':').map(|(host, _)| host).unwrap_or(host))
            .or_else(|| session.req_header().uri.host());
        let target = server_name
            .and_then(|host| self.redirect_by_name.get(host))
            .unwrap_or(&self.redirect_to);

        let mut header = ResponseHeader::build(status, Some(4))?;
        header.append_header(header::CONTENT_LENGTH, text.len().to_string())?;
        header.append_header(header::CONTENT_TYPE, "text/html")?;
        header.append_header(
            header::LOCATION,
            format!(
                "https://{}{}",
                target,
                session
                    .req_header()
                    .uri
                    .path_and_query()
                    .map(|p| p.as_str())
                    .unwrap_or_default()
            ),
        )?;
        session.write_response_header(Box::new(header)).await?;

        if session.req_header().method != Method::HEAD {
            session.write_response_body(text.into()).await?;
        }

        Ok(true)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        Err(Error::new(ErrorType::HTTPStatus(404)))
    }
}

pub(crate) fn create_redirector(
    conf: &TlsRedirectorConf,
    server_conf: &Arc<ServerConf>,
) -> Result<impl Service + 'static, Box<Error>> {
    if conf.redirect_to.is_empty() {
        return Err(Error::explain(
            TLS_CONF_ERR,
            "tls.redirector.redirect_to setting has to be specified for TLS redirector",
        ));
    }

    let app = RedirectorApp {
        redirect_to: conf.redirect_to.clone(),
        redirect_by_name: conf.redirect_by_name.to_owned(),
    };
    let mut service = http_proxy_service(server_conf, app);

    for addr in &conf.listen {
        if addr.tls {
            return Err(Error::explain(
                TLS_CONF_ERR,
                "tls.redirector.listen setting cannot contain any TLS addresses",
            ));
        }

        if let Some(socket_options) = addr.to_socket_options() {
            service.add_tcp_with_settings(&addr.addr, socket_options);
        } else {
            service.add_tcp(&addr.addr);
        }
    }

    Ok(service)
}
