# IP Anonymization Module for Pandora Web Server

This crate allows removing part of the client’s IP address, making certain that a full IP
address is never logged or leaked. The remaining address still contains enough information
for geo-location but cannot be traced back to an individual user any more.

Currently, only one configuration setting is supported: setting `anonymization_enabled` to
`true` in the configuration or supplying `--anonymization-enabled` command line flag enables
this functionality.

*Note*: due to [Pingora limitations](https://github.com/cloudflare/pingora/issues/270), the
original IP address cannot be completely removed at the moment. Code that dereferences
`SessionWrapper` into the original Pingora `Session` data structure or code accessing
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
use clap::Parser;
use ip_anonymization_module::{IPAnonymizationHandler, IPAnonymizationOpt};
use pandora_module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use startup_module::{DefaultApp, StartupConf, StartupOpt};
use static_files_module::{StaticFilesHandler, StaticFilesOpt};

#[derive(Debug, RequestFilter)]
struct Handler {
    anonymization: IPAnonymizationHandler,
    static_files: StaticFilesHandler,
}

#[merge_conf]
struct Conf {
    startup: StartupConf,
    handler: <Handler as RequestFilter>::Conf,
}

#[merge_opt]
struct Opt {
    startup: StartupOpt,
    anonymization: IPAnonymizationOpt,
    static_files: StaticFilesOpt,
}

let opt = Opt::parse();
let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
conf.handler.anonymization.merge_with_opt(opt.anonymization);
conf.handler.static_files.merge_with_opt(opt.static_files);

let app = DefaultApp::<Handler>::from_conf(conf.handler).unwrap();
let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();

// Do something with the server here, e.g. call server.run_forever()
```
