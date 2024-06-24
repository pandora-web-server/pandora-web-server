# Upstream Module for Pandora Web Server

This crate helps configure Pingora’s upstream functionality. It is most useful in combination
with the `virtual-hosts-module` crate that allows applying multiple upstream configurations
conditionally.

Currently only one configuration option is provided: `upstream` (`--upstream` as command line
option). The value should be a URL like `http://127.0.0.1:8081` or `https://example.com`.

Supported URL schemes are `http://` and `https://`. Other than the scheme, only host name and
port are considered. Other parts of the URL are ignored if present.

## Code example

`UpstreamHandler` handles both `request_filter` and `upstream_peer` phases. The former selects
an upstream peer and modifies the request by adding the appropriate `Host` header. The latter
retrieves the previously selected upstream peer.

```rust
use module_utils::{merge_conf, merge_opt, FromYaml};
use startup_module::{DefaultApp, StartupConf, StartupOpt};
use structopt::StructOpt;
use upstream_module::{UpstreamConf, UpstreamHandler, UpstreamOpt};

#[merge_conf]
struct Conf {
    startup: StartupConf,
    upstream: UpstreamConf,
}

#[merge_opt]
struct Opt {
    startup: StartupOpt,
    upstream: UpstreamOpt,
}

let opt = Opt::from_args();
let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
conf.upstream.merge_with_opt(opt.upstream);

let app = DefaultApp::<UpstreamHandler>::from_conf(conf.upstream).unwrap();
let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();

// Do something with the server here, e.g. call server.run_forever()
```
