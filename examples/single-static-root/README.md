# Single static root example

This is a simple web server using `static-files-module` crate. It combines the usual
[Pingora command line options](https://docs.rs/pingora-core/0.1.1/pingora_core/server/configuration/struct.Opt.html) with the
[command line options of `static-files-module`](https://docs.rs/static-files-module/0.1.0/static_files_module/struct.StaticFilesOpt.html)
and the usual [Pingora config file settings](https://docs.rs/pingora-core/0.1.1/pingora_core/server/configuration/struct.ServerConf.html) with the
[config file settings of `static-files-module`](https://docs.rs/static-files-module/0.1.0/static_files_module/struct.StaticFilesConf.html).
In addition, it provides the following settings:

* `listen` (`--listen` as command line flag): A list of IP address/port combinations the server
  should listen on, e.g. `0.0.0.0:8080`.
* `compression_level` (`--compression-level` as command line flag): If present, dynamic
  compression will be enabled and compression level set to the value provided for all
  algorithms (see [Pingora issue #228](https://github.com/cloudflare/pingora/issues/228)).

An example config file is provided in this directory. You can run this example with the
following command:

```sh
cargo run --package example-single-static-root -- -c config.yaml
```

To enable debugging output you can use the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug cargo run --package example-single-static-root -- -c config.yaml
```
