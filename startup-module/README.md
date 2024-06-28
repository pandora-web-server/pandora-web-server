# Startup Module for Pandora Web Server

This crate helps configure and set up the Pingora server. It provides a `StartupOpt` data
structure with the relevant command line and `StartupConf` with the configuration file
options. Once these data structures are all set up, `StartupConf::into_server` method can be
used to get a Pingora server instance.

## General configuration

The Startup Module currently exposes all of the
[Pingora configuration options](https://docs.rs/pingora/0.2.0/pingora/server/configuration/struct.ServerConf.html). In addition, it
provides a `listen` configuration option, a list of IP address/port combinations that the
server should listen on:

```yaml
listen:
- 127.0.0.1:8080
- "[::1]:8080"
```

On many Unix and Linux systems, listening on `[::]` (all IPv6 addresses) has a special
behavior: it will also accept IPv4 connections on the same port. There is system-wide
configuration for this behavior, e.g. `/proc/sys/net/ipv6/bindv6only` file on Linux.

If you do not want the default system behavior, you can specify the `ipv6_only` flag
explicitly:

```yaml
listen:
- { addr: "[::]:8080", ipv6_only: true }
```

With the configuration above the server will still listen on all IPv6 addresses, yet IPv4
connections will only be accepted if configured explicitly.

The `listen` configuration option is also available as `--listen` command line option. Flags
cannot be specified via the command line, only the address to listen on. This command line
option can be specified multiple times to make the server listen on multiple addresses or ports.

Other command line options are: `--conf` (configuration file or configuration files to load),
`--daemon` (run process in background) and `--test` (test configuration and exit).

## TLS configuration

You can enable TLS for some or all addresses the server listens on by specifying the `tls`
flag:

```yaml
listen:
- {addr: 127.0.0.1:8080, tls: true}
- {addr: "[::1]:8080", tls: true}
```

If TLS is used, the configuration at the very least has to specify the default certificate and
key:

```yaml
tls:
    cert_path: cert.pem
    key_path: key.pem
```

If you use different certificates for different server names (SNI), you can additionally list
these under `server_names`:

```yaml
tls:
    cert_path: cert.pem
    key_path: key.pem
    server_names:
        example.com:
            cert_path: cert.example.com.pem
            key_path: key.example.com.pem
        example.net:
            cert_path: cert.example.net.pem
            key_path: key.example.net.pem
```

If a server name indicator is received and a matching server name exists in the configuration,
the corresponding certificate will be used. Otherwise the default certificate will be used as
fallback.

## TLS Redirector configuration

In order to simplify TLS setup, automatic redirection of non-HTTPS ports to TLS is supported.
The basic configuration for a localhost server looks like this:

```yaml
tls:
    redirector:
        listen:
        - 127.0.0.1:80
        - "[::1]:80"
        redirect_to: localhost
```

The `listen` setting works the same as the top-level setting of the same name, except that the
`tls` flag isn’t allowed. Here, any requests coming in on port 80 will be automatically
redirected to `https://localhost` while keeping the request path.

In scenarios where multiple host names are handled by the server, the `redirect_by_name`
setting can be used:

```yaml
tls:
    redirector:
        listen:
        - 127.0.0.1:80
        - "[::1]:80"
        redirect_to: localhost
        redirect_by_name:
            example.com: example.com
            example.net: example.net
```

Requests for `example.com` (note: exact host name match) will be redirected to
`https://example.com`, requests for `example.net` to `https://example.com` and all other
requests will be subject to the `redirect_to` setting and redirect to `https://localhost`.

If you use an HTTPS port other than 443, you can specify the port in the redirect settings:

```yaml
        redirect_to: localhost:8443
        redirect_by_name:
            example.com: example.com:8443
            example.net: example.net:8443
```

The incoming server name in the `redirect_by_name` setting does not depend on the port.

## Code example

```rust
use async_trait::async_trait;
use clap::Parser;
use pandora_module_utils::pingora::{Error, HttpPeer, ProxyHttp, Session};
use pandora_module_utils::FromYaml;
use startup_module::{StartupConf, StartupOpt};

pub struct MyServer;

#[async_trait]
impl ProxyHttp for MyServer {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>, Box<Error>> {
        Ok(Box::new(HttpPeer::new(("example.com", 443), true, "example.com".to_owned())))
    }
}

let opt = StartupOpt::parse();
let conf = StartupConf::load_from_files(opt.conf.as_deref().unwrap_or(&[])).unwrap();
let server = conf.into_server(MyServer {}, Some(opt)).unwrap();

// Do something with the server here, e.g. call server.run_forever()
```
