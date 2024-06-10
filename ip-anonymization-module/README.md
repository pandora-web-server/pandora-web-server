# IP Anonymization Module

This crate allows removing part of the client’s IP address, making certain that a full IP
address is never logged or leaked. The remaining address still contains enough information
for geo-location but cannot be traced back to an individual user any more.

Currently, only one configuration setting is supported: setting `anonymization_enabled` to
`true` in the configuration or supplying `--anonymization-enabled` command line flag enables
this functionality.

*Note*: due to [Pingora limitations](https://github.com/cloudflare/pingora/issues/270), the
original IP address cannot be completely removed at the moment. Code that dereferences
[`SessionWrapper`] into the original Pingora `Session` data structure or code accessing
`session.digest()` directly will still get the unanonymized IP address. This will hopefully
be addressed with a future Pingora version.

## Anonymization approach

When given an IPv4 address, the last octet is removed: the address `192.0.2.3` for example
becomes `192.0.2.0`. With IPv6, all but the first two groups are removed: the address
`2001:db8:1234:5678::2` for example becomes `2001:db8::`.

## Using the module

This module’s handler should be called prior to any other handler for the `request_filter`
phase:

```rust
use ip_anonymization_module::{IPAnonymizationHandler, IPAnonymizationOpt};
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use pingora_core::server::configuration::{Opt as ServerOpt, ServerConf};
use pingora_core::server::Server;
use static_files_module::StaticFilesHandler;
use structopt::StructOpt;

#[derive(Debug, RequestFilter)]
struct Handler {
    anonymization: IPAnonymizationHandler,
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
    anonymization: IPAnonymizationOpt,
}

let opt = Opt::from_args();
let mut conf = opt
    .server
    .conf
    .as_ref()
    .and_then(|path| Some(Conf::load_from_yaml(path).unwrap()))
    .unwrap_or_default();
conf.handler.anonymization.merge_with_opt(opt.anonymization);

let mut server = Server::new_with_opt_and_conf(opt.server, conf.server);
server.bootstrap();

let handler = Handler::new(conf.handler);
```

For complete code see [single-static-root example](https://github.com/palant/pingora-utils/tree/main/examples/single-static-root) and [virtual-hosts](https://github.com/palant/pingora-utils/tree/main/examples/virtual-hosts) examples in the repository.
