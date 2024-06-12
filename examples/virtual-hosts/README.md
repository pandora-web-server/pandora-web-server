# Virtual hosts example

This web server uses `virtual-hosts-module` crate to handle virtual hosts. Various modules like
`compression-module` and `static-files-module` crates are used for each individual virtual
host. Other modules like `custom-headers-module` are used at the top level and apply to all
virtual hosts. The configuration file looks like this:

```yaml
# Startup module settings (https://docs.rs/startup-module/latest/startup_module/struct.StartupConf.html)
listen:
- "[::]:8080"
daemon: false

# Headers module settings (https://docs.rs/headers-module/latest/headers_module/struct.HeadersConf.html)
custom_headers:
- headers:
    Server: "My server is the best"

# Virtual hosts settings:
# * https://docs.rs/virtual-hosts-module/latest/virtual_hosts_module/struct.VirtualHostsConf.html
# * https://docs.rs/log-module/latest/log_module/struct.LogConf.html
# * https://docs.rs/compression-module/latest/compression_module/struct.CompressionConf.html
# * https://docs.rs/auth-module/latest/auth_module/struct.AuthConf.html
# * https://docs.rs/rewrite-module/latest/rewrite_module/struct.RewriteConf.html
# * https://docs.rs/upstream-module/latest/upstream_module/struct.UpstreamConf.html
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
