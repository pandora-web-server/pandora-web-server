# Auth Module for Pingora

This crate allows putting up an authentication check before further processing of the request
happens. Only authorized users can proceed, others get a “401 Unauthorized” response. This
barrier can apply to the entire server or, with the help of the Virtual Hosts Module, only a
single virtual host/subdirectory.

A configuration could look like this:

```yaml
auth_realm: Protected area
auth_credentials:
    me: $2y$12$iuKHb5UsRqktrX2X9.iSEOP1n1.tS7s/KB.Dq3HlE0E6CxlfsJyZK
    you: $2y$12$diY.HNTgfg0tIJKJxwmq.edEep5RcuAuQaAvXsP22oSPKY/dS1IVW
```

This sets up two users `me` and `you` with their respective password hashes, corresponding with
the passwords `test` and `test2`.

## Password hashes

The supported password hashes use the [bcrypt algorithm](https://en.wikipedia.org/wiki/Bcrypt)
and should start with either `$2b$` or `$2y$`. While `$2a$` and `$2x$` hashes can be handled as
well, these should be considered insecure due to implementation bugs.

A hash can be generated using the `htpasswd` tool distributed along with the Apache web server:

```sh
htpasswd -nBC 12 user
```

Alternatively, you can use this module to generate a password hash for you:

1. To activate the module, make sure the `auth_credentials` setting isn’t empty. It doesn’t
have to contain a valid set of credentials, any value will do.
2. Add the `auth_display_hash: true` setting to your configuration.
3. Run the server and navigate to the password protected area with your browser.
4. When prompted by the browser, enter the credentials you want to use.
5. When prompted for credentials again, close the prompt to see the “401 Unauthorized” page.

The page will contain the credentials you should add to your configuration. You can remove the
`auth_display_hash: true` setting now.

## Code example

You would normally put this handler in front of other handlers, such as the Static Files
Module. You would use macros to merge the configuration and the command-line options of the
handlers and Pingora:

```rust
use auth_module::{AuthHandler, AuthOpt};
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use static_files_module::{StaticFilesHandler, StaticFilesOpt};
use structopt::StructOpt;

#[derive(Debug, RequestFilter)]
struct Handler {
    auth: AuthHandler,
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
    auth: AuthOpt,
}

let opt = Opt::from_args();
let conf = opt
    .server
    .conf
    .as_ref()
    .and_then(|path| Some(Conf::load_from_yaml(path).unwrap()))
    .unwrap_or_default();

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
use pingora_http::ResponseHeader;
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
        panic!("Unexpected, upstream_peer stage reached");
    }
}
```

For complete code see [single-static-root example](https://github.com/palant/pingora-utils/tree/main/examples/single-static-root) and [virtual-hosts](https://github.com/palant/pingora-utils/tree/main/examples/virtual-hosts) examples in the repository.
