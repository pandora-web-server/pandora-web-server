# Upstream module for Pandora Web Server

The Upstream module allows forwarding incoming requests to another HTTP or HTTPS server. Its configuration supports only one upstream server that will handle all requests, multiple servers are possible by combining this module with the Virtual Hosts module.

## Request forwarding

The configuration defines only the scheme (HTTP or HTTPS), host name and port of the upstream server. Other URL parts such as path or query string are ignored.

If the request needs to be mapped to a different path prior to forwarding, the Rewrite module can be used.

## Configuration settings

| Configuration setting   | Command line    | Type    | Description |
|-------------------------|-----------------|---------|-------------|
| `upstream`              | `--upstream`    | string  | An upstream server like `http://127.0.0.1:8081` or `https://example.com` |

### Additional settings

Pingora settings such as `ca_file` and `client_bind_to_ipv4` apply to upstream requests. These are exposed by the Startup module configuration.
