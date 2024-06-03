# Virtual hosts example

This web server uses `virtual-hosts-module` crate to handle virtual hosts. The
`compression-module` and `static-files-module` crates are used for each individual virtual
host. The configuration file looks like this:

```yaml
# Application-specific settings
listen:
- "[::]:8080"

# General server settings (https://docs.rs/pingora-core/0.2.0/pingora_core/server/configuration/struct.ServerConf.html)
daemon: false

# Virtual hosts settings:
# * https://docs.rs/virtual-hosts-module/latest/virtual_hosts_module/struct.VirtualHostsConf.html
# * https://docs.rs/compression-module/latest/compression_module/struct.CompressionConf.html
# * https://docs.rs/static-files-module/latest/static_files_module/struct.StaticFilesConf.html
vhosts:
    localhost:8080:
        aliases:
        - 127.0.0.1:8080
        - "[::1]:8080"
        root: ./local-debug-root
    example.com:
        default: true
        compression_level: 3
        root: ./production-root
```

Example config files are provided in this directory. You can run this example with the
following command:

```sh
cargo run --package example-virtual-hosts -- -c config/*.yaml
```

To enable debugging output you can use the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug cargo run --package example-virtual-hosts -- -c config/*.yaml
```
