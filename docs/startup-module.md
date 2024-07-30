# Startup module for Pandora Web Server

The Startup module is responsible for initializing Pandora Web Server: setting up addresses to listen on, configuring TLS, redirecting plain HTTP connections to HTTPS. An example configuration file could look like this:

```yaml
listen:
- 127.0.0.1:8080
- addr: "[::]:443"
  tls: true
tls:
  cert_path: cert.pem
  key_path: key.pem
  redirector:
    listen:
    - "[::]:80"
    redirect_to: example.com
    redirect_by_name:
      [localhost, localhost.localdomain]: localhost
      example.net: example.com
```

## Server Name Indication (SNI) support

While some TLS setups will only use a single server certificate, often a matching certificate has to be selected based on the server name. This is done via a mechanism called Server Name Indication (SNI). The client provides the name of the server it wants to communicate with, which allows the server to choose the right certificate.

The Startup module supports SNI via the `server_name` setting in the [TLS configuration](#tls-configuration). It allows you to specify certificates per host name:

```yaml
tls:
  cert_path: cert.pem
  key_path: key.pem
  server_names:
    [example.com, www.example.com]:
      cert_path: cert.example.com.pem
      key_path: key.example.com.pem
    example.net:
      cert_path: cert.example.net.pem
      key_path: key.example.net.pem
```

Note that the default certificate is required even when the `server_names` setting is present. It will be used if the client requests an unknown server name or no server name at all.

Also, unlike with there Virtual Hosts module, server names are specified *without* a port number here. The selected certificate only depends on the requested server name, not on its port.

## TLS redirector

In order to simplify TLS setup, automatic redirection of non-HTTPS ports to TLS is supported. The basic configuration for a localhost server looks like this:

```yaml
listen:
- addr: 192.0.2.3:443
  tls: true

tls:
  redirector:
    listen: 192.0.2.3:80
    redirect_to: example.com
```

The main server listens on port 443 (HTTPS) here whereas the redirector listens on port 80 (HTTP). Incoming HTTP requests on port 80 are automatically redirected to corresponding addresses on `https://example.com`.

If there are multiple server names being handled by the server, the [redirector configuration](#tls-redirector-configuration) can be extended with the `redirect_by_name` map to specify additional redirect targets:

```yaml
tls:
  redirector:
    listen: 192.0.2.3:80
    redirect_to: example.com
    redirect_by_name:
      [example.net, www.example.net]: example.net
```

Note that the `redirect_to` setting is still required as fallback for the scenario that some unknown server name is requested.

## Configuration settings

| Configuration setting | Command line     | Type | Default value | Description |
|-----------------------|------------------|------|---------------|-------------|
|                       | `-c`, `--conf`   | list of file paths or globs |  | Configuration files to process |
| `listen`              | `-l`, `--listen` | list of [IP address/port configurations](#ip-addressport-configuration) | `[127.0.0.1:8080, "[::1]:8080"]` | The IP addresses and ports the server should bind on |
| `tls`                 |                  | [TLS configuration](#tls-configuration) | | TLS-related configuration settings |
| `daemon`              | `-d`, `--daemon` | boolean | `false` | If `true`, the server will start in background |
|                       | `-t`, `--test`   | boolean | `false` | If `true`, the server will exit after processing the configuration. |

In addition, this module exposes all [Pingora configuration settings](https://github.com/cloudflare/pingora/blob/0.2.0/docs/user_guide/conf.md).

### IP address/port configuration

An IP address/port combination can be provided as a string like `127.0.0.1:8080` or `[::1]:443`. In order to configure advanced settings however, it should be written out as a map. The following settings can be used:

| Configuration setting | Type    | Default value  | Description |
|-----------------------|---------|----------------|-------------|
| `addr`                | string  |                | IP address and port the server should bind on, e.g. `127.0.0.1:8080` |
| `tls`                 | boolean | `false`        | If `true`, expect TLS connections on this address/port combination   |
| `ipv6_only`           | boolean | system default | Determines whether listening on IPv6 `[::]` address should accept IPv4 connections as well |

The `tls` setting is ignored for TLS redirector addresses.

### TLS configuration

These settings are required in any of the addresses in the `listen` setting is listed with the `tls` flag.

| Configuration setting | Type      | Description |
|-----------------------|-----------|-------------|
| `cert_path`           | file path | Path to the default certificate file |
| `key_path`            | file path | Path to the default private key file |
| `server_names`        | map       | Lists of server names mapped to their respective `cert_path` and `key_path` settings |
| `redirector`          | [redirector configuration](#tls-redirector-configuration) | Configures plain HTTP to HTTP redirection |

Note that server names in the TLS configuration are different from virtual hosts, they do not contain the port number.

### TLS redirector configuration

The TLS redirector can automatically redirect incoming connections on plain HTTP ports to HTTPS.

| Configuration setting | Type      | Description |
|-----------------------|-----------|-------------|
| `listen`              | list of [IP address/port configurations](#ip-addressport-configuration) | The IP addresses and ports that the TLS redirector should bind to |
| `redirect_to`         | string    | Default server name to redirect to |
| `redirect_by_name`    | map       | Maps lists of server names to the names they should be redirected to |
