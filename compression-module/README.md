# Compression Module for Pingora

This crate helps configure Pingora’s built-in compression mechanism. It provides two
configuration options:

* `compression_level` (`--compression-level` as command-line option): If present, will enable
  dynamic downstream compression and use the specified compression level (same level for all
  compression algorithms, see
  [Pingora issue #228](https://github.com/cloudflare/pingora/issues/228)).
* `decompress_upstream` (`--decompress-upstream` as command-line flag): If `true`,
  decompression of upstream responses will be enabled.

## Code example

You would normally put this handler in front of other handlers, such as the Static Files
Module. The `module-utils` and `startup-modules` provide helpers to simplify merging of
configuration and the command-line options of various handlers as well as creating a server
instance from the configuration:

```rust
use compression_module::{CompressionConf, CompressionHandler, CompressionOpt};
use module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use startup_module::{DefaultApp, StartupConf, StartupOpt};
use static_files_module::{StaticFilesHandler, StaticFilesOpt};
use structopt::StructOpt;

#[derive(Debug, RequestFilter)]
struct Handler {
    compression: CompressionHandler,
    static_files: StaticFilesHandler,
}

#[merge_opt]
struct Opt {
    startup: StartupOpt,
    compression: CompressionOpt,
    static_files: StaticFilesOpt,
}

#[merge_conf]
struct Conf {
    startup: StartupConf,
    handler: <Handler as RequestFilter>::Conf,
}

let opt = Opt::from_args();
let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
conf.handler.compression.merge_with_opt(opt.compression);
conf.handler.static_files.merge_with_opt(opt.static_files);

let app = DefaultApp::<Handler>::from_conf(conf.handler).unwrap();
let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();

// Do something with the server here, e.g. call server.run_forever()
```

For more comprehensive examples see the [`examples` directory in the repository](https://github.com/palant/pingora-utils/tree/main/examples).
