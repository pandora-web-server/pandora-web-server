# Single static root example

This is a simple web server using `startup-module`, `anonymization-module`, `log-module`,
`compression-module`, `auth-module`, `rewrite-module`, `headers-module` and
`static-files-module` crates. It combines all their respective command line options and their
config file settings.

An example config file is provided in this directory. You can run this example with the
following command:

```sh
cargo run --package example-single-static-root -- -c config.yaml
```

To enable debugging output you can use the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug cargo run --package example-single-static-root -- -c config.yaml
```
