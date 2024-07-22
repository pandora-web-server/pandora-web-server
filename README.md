# Pandora Web Server

This repository contains various crates related to the Pandora Web Server. You probably want to
have a look at the [`pandora-web-server` crate](../../tree/main/pandora-web-server). If you are
interested in creating a custom web server build with your own modules, have a look at the
[`custom-module` example](../../tree/main/examples/custom-module).

Other than that, there are:

* [Pandora Module Utils](../../tree/main/pandora-module-utils): Various useful helpers used by the
  server and its modules
* [Authentication module](../../tree/main/auth-module): Authentication support
* [Common Log module](../../tree/main/common-log-module): Creation of access logs in the [Common
  Log Format](https://en.wikipedia.org/wiki/Common_Log_Format)
* [Compression module](../../tree/main/compression-module): Configured dynamic response compression
* [Headers module](../../tree/main/headers-module): Configure HTTP headers to be added to responses
* [IP Anonymization module](../../tree/main/ip-anonymization-module): Remove part of the IP address
  to anonymize requests
* [Response module](../../tree/main/response-module): Produce HTTP responses from configuration
* [Rewrite module](../../tree/main/rewrite-module): Rules to modify request URI or produce
  redirect responses
* [Startup module](../../tree/main/static-files-module): Configuring and starting the web server
* [Static Files module](../../tree/main/static-files-module): Serve static files from a directory
* [Upstream module](../../tree/main/upstream-module): Redirects response to an upstream HTTP server
* [Virtual Hosts module](../../tree/main/virtual-hosts-module): Handle separate configurations for
  virtual hosts

## Rust version

Currently, the minimal supported Rust version (MSRV) is 1.74. In future, the plan is to track
[Pingora’s MSRV](https://github.com/cloudflare/pingora/?tab=readme-ov-file#rust-version).
