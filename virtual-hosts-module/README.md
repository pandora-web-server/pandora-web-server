# Virtual Hosts Module for Pandora Web Server

This module simplifies dealing with virtual hosts. It wraps any handler implementing
`pandora_module_utils::RequestFilter` and its configuration, allowing to supply a different
configuration for that handler for each virtual host and paths of that host. For example, if
Static Files Module is the wrapped handler, the configuration file might look like this:

```yaml
vhosts:
    localhost:8000:
        aliases:
            - 127.0.0.1:8000
            - "[::1]:8000"
        root: ./local-debug-root
    example.com:
        aliases:
            - www.example.com
        default: true
        root: ./production-root
        subpaths:
            /metrics/*
                root: ./metrics
            /test/*:
                strip_prefix: true
                root: ./local-debug-root
            /test/file.txt:
                root: ./production-root
```

A virtual host configuration adds three configuration settings to the configuration of the
wrapped handler:

* `aliases` lists additional host names that should share the same configuration.
* `default` can be set to `true` to indicate that this configuration should apply to all host
  names not listed explicitly.
* `subpaths` maps paths within the virtual host to their respective configuration. If the path
  ends with `/*`, it will match not only the exact path but any files within the subdirectory
  as well. The configuration is that of the wrapped handler with the added `strip_prefix`
  setting. If `true`, this setting will remove the matched path from the URI before the request
  is passed on to the handler.

If no default host entry is present and a request is made for an unknown host name, this
handler will leave the request unhandled. Otherwise the handling is delegated to the wrapped
handler.

When selecting a path configuration, longer matching paths are preferred. Matching always
happens against full file names, meaning that URI `/test/abc` matches the subdirectory
`/test` whereas the URI `/test_abc` doesn’t. If no matching path is found, the host
configuration will be used.

*Note*: When the `strip_prefix` option is used, the subsequent handlers will receive a URI
which doesn’t match the actual URI of the request. This might result in wrong links or
redirects. The Static Files and Auth modules know how to compensate. Upstream responses might
have to be corrected via Pingora’s `upstream_response_filter` phase.

## Code example

Usually, the virtual hosts configuration will be read from a configuration file and used to
instantiate the corresponding handler, pass it to the server app which in turn is used to
create a server. The `pandora-module-utils` and `startup-module` crates provide helpers to
simplify merging of configuration options as well as creating a server instance from the
configuration:

```rust
use clap::Parser;
use pandora_module_utils::{merge_conf, FromYaml};
use startup_module::{DefaultApp, StartupConf, StartupOpt};
use static_files_module::{StaticFilesConf, StaticFilesHandler};
use virtual_hosts_module::{VirtualHostsConf, VirtualHostsHandler};

// Combine statup configuration with virtual hosts wrapping static files configuration.
#[merge_conf]
struct Conf {
    startup: StartupConf,
    virtual_hosts: VirtualHostsConf<StaticFilesConf>,
}

// Read command line options and configuration file.
let opt = StartupOpt::parse();
let conf = Conf::load_from_files(opt.conf.as_deref().unwrap_or(&[])).unwrap();

// Create a server from the configuration
let app = DefaultApp::<VirtualHostsHandler<StaticFilesHandler>>::from_conf(conf.virtual_hosts)
    .unwrap();
let server = conf.startup.into_server(app, Some(opt)).unwrap();

// Do something with the server here, e.g. call server.run_forever()
```
