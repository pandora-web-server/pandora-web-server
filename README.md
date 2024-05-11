# Pingora utils

This repository is meant to contain various crates which can be used to extend Pingora functionality. At the moment these are:

* [Module utils](../../tree/main/module-utils): Various useful helpers for the other crates
* [Compression Module](../../tree/main/compression-module): Helps configure Pingora’s built-in compression
* [Static Files Module](../../tree/main/static-files-module): Serve static files from a directory
* [Upstream Module](../../tree/main/upstream-module): Helps configure Pingora’s built-in upstream proxying
* [Virtual Hosts Module](../../tree/main/virtual-hosts-module): Handle separate configurations for virtual hosts

## Rust version

Currently, the minimal supported Rust version (MSRV) is 1.74. In future, the plan is to track [Pingora’s MSRV](https://github.com/cloudflare/pingora/?tab=readme-ov-file#rust-version).
