# Virtual hosts example

This web server uses `virtual-hosts-module` crate to handle virtual hosts and
`static-files-module` crate for each individual virtual host. The configuration file looks like
this:

```yaml
# Application-specific settings
listen:
- "[::]:8080"
compression_level: 3

# General server settings (https://docs.rs/pingora-core/0.1.1/pingora_core/server/configuration/struct.ServerConf.html)
daemon: false

# Virtual hosts settings (https://docs.rs/static-files-module/latest/static_files_module/struct.StaticFilesConf.html)
vhosts:
    localhost:8080:
        aliases:
        - 127.0.0.1:8080
        - "[::1]:8080"
        root: ./local-debug-root
    example.com:
        default: true
        root: ./production-root
```

An example config file is provided in this directory. You can run this example with the
following command:

```sh
cargo run --package example-virtual-hosts -- -c config.yaml
```

To enable debugging output you can use the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug cargo run --package example-virtual-hosts -- -c config.yaml
```
