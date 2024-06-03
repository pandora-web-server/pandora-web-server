# Common Log Module for Pingora

This crate implements the creation of access log files in the
[Common Log Format](https://en.wikipedia.org/wiki/Common_Log_Format) that can be processed
further by a variety of tools. A configuration could look like this:

```yaml
log_file: access.log
log_format: [
    remote_addr, -, -, time_local, request, status, bytes_sent, http_referer, http_user_agent
]
```

The `log_file` field is also available as `--log-file` command line option.

The supported fields for the `log_format` setting are:

* `-`: Verbatim `-` character (for unsupported fields)
* `remote_addr`: client’s IP address
* `remote_port`: client’s TCP port
* `time_local`: date and time of the request, e.g. `[10/Oct/2000:13:55:36 -0700]`
* `time_iso8601`: date and time in the ISO 8601 format, e.g. `[2000-10-10T13:55:36-07:00]`
* `request`: quoted request line, e.g. `"GET / HTTP/1.1"`
* `status`: status code of the response, e.g. `200`
* `bytes_sent`: number of bytes sent as response
* `processing_time`: time from request being received to response in milliseconds
* `http_<header>`: quoted value of an HTTP request header. For example, `http_user_agent` adds
  the value of the `User-Agent` HTTP header to the log.
* `sent_http_<header>`: quoted value of an HTTP response header. For example,
  `sent_http_content_type` adds the value of the `Content-Type` HTTP header to the log.

This module will add one line per request to the log file. A log file will be created if
necessary, data in already existing files will be kept.

Multiple log files are possible via `virtual-hosts-module` for example. Adding Common Log
Module to its host handler will make sure that each virtual host has its own logging
configuration.

On Unix-based systems, the process can be sent a `HUP` or `USR1` signal to make it re-open log
files. This is useful after the logs have been rotated for example.

## Code example

This handler needs to run first during the `request_filter` phase, so that it can capture
relevant data before it has been altered. Later the actual logging can be performed during the
`logging` phase. Handler configuration would then look as follows:

```rust
use common_log_module::{CommonLogHandler, CommonLogOpt};
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use static_files_module::StaticFilesHandler;
use structopt::StructOpt;

#[derive(Debug, RequestFilter)]
struct Handler {
    log: CommonLogHandler,
    static_files: StaticFilesHandler,
}

#[merge_conf]
struct Conf {
    server: ServerConf,
    handler: <Handler as RequestFilter>::Conf,
}

#[merge_opt]
struct Opt {
    server: ServerOpt,
    log: CommonLogOpt,
}

let opt = Opt::from_args();
let mut conf = opt
    .server
    .conf
    .as_ref()
    .and_then(|path| Some(Conf::load_from_yaml(path).unwrap()))
    .unwrap_or_default();
conf.handler.log.merge_with_opt(opt.log);

let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
server.bootstrap();

let handler = Handler::new(conf.handler);
```

You can then use that handler in your server implementation:
```rust
use async_trait::async_trait;
use module_utils::RequestFilter;
use pingora_core::Error;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_proxy::{ProxyHttp, Session};

struct MyServer {
    handler: Handler,
}

#[async_trait]
impl ProxyHttp for MyServer {
    type CTX = <Handler as RequestFilter>::CTX;
    fn new_ctx(&self) -> Self::CTX {
        Handler::new_ctx()
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool, Box<Error>> {
        self.handler.handle(session, ctx).await
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        panic!("Upstream phase should not be reached");
    }

    async fn logging(
        &self,
        session: &mut Session,
        _e: Option<&Error>,
        ctx: &mut Self::CTX,
    ) {
        self.handler.log.logging(session, &mut ctx.log);
    }
}
```

For complete code see [single-static-root example](https://github.com/palant/pingora-utils/tree/main/examples/single-static-root) and [virtual-hosts](https://github.com/palant/pingora-utils/tree/main/examples/virtual-hosts) examples in the repository.
