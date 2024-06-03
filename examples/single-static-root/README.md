# Single static root example
This is a simple web server using `log-module`, `compression-module`, `auth-module`,
`rewrite-module`, `headers-module` and `static-files-module` crates. It combines their
respective command line options with the usual [Pingora command line options](https://docs.rs/static-files-module/0.2.0/static_files_module/struct.StaticFilesOpt.html) and
their config file settings with [Pingora`s](https://docs.rs/static-files-module/0.2.0/static_files_module/struct.StaticFilesConf.html). In addition, it provides the following
setting:

* `listen` (`--listen` as command line flag): A list of IP address/port combinations the server
  should listen on, e.g. `0.0.0.0:8080`.

An example config file is provided in this directory. You can run this example with the
following command:

```sh
cargo run --package example-single-static-root -- -c config.yaml
```

To enable debugging output you can use the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug cargo run --package example-single-static-root -- -c config.yaml
```
