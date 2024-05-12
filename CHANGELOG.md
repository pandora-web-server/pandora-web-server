# Changelog
All notable changes to this project will be documented in this file.

## [v0.2.0] - 2024-05-12
### :boom: BREAKING CHANGES
- due to [`f0b82e1`](https://github.com/palant/pingora-utils/commit/f0b82e1cca468b5fd9dd685ef3dc8c6e9cd6be93) - Introduced Virtual Hosts Module implementing per-host handler configuration *(commit by [@palant](https://github.com/palant))*:

  Calling `StaticFilesHandler::new()` and `StaticFilesHandler::handle()` now requires the `module_utils::RequestFilter` trait to be in scope.

  `StaticFilesHandler::handle()` now takes the context as an additional (unused) parameter.

- due to [`a66fdc6`](https://github.com/palant/pingora-utils/commit/a66fdc6464032a5cb1c5c2425592f3dd74639fc0) - If no root is configured, disable the module *(commit by [@palant](https://github.com/palant))*:

  `StaticFilesConf.root` is an `Option<PathBuf>`, no longer `PathBuf`

- due to [`062126e`](https://github.com/palant/pingora-utils/commit/062126e3e9ff68f48069fb8eca87605d0f244611) - Do not respond with 404 when not configured, ignore request instead *(commit by [@palant](https://github.com/palant))*:

  With default configuration, Static Files Module will leave all requests unhandled


### :sparkles: New Features
- [`8e98035`](https://github.com/palant/pingora-utils/commit/8e98035b65fab191458bfd46265c7c24706fff3e) - **module-utils**: Added `FromYaml` trait to simplify extending Pingora configuration files *(commit by [@palant](https://github.com/palant))*
- [`dfa1933`](https://github.com/palant/pingora-utils/commit/dfa1933fd332d3511d8ab2b4e1525c7befcb13b7) - **module-utils**: Added `merge_opt` and `merge_conf` macros, simplifying extending configuration structures *(commit by [@palant](https://github.com/palant))*
- [`f0b82e1`](https://github.com/palant/pingora-utils/commit/f0b82e1cca468b5fd9dd685ef3dc8c6e9cd6be93) - **virtual-hosts-module**: Introduced Virtual Hosts Module implementing per-host handler configuration *(commit by [@palant](https://github.com/palant))*
- [`fda9c11`](https://github.com/palant/pingora-utils/commit/fda9c11f7420f438e4a8ee60234b6be6fb237a74) - **compression-module**: Added Compression Module to simplify configuration of Pingora's compression *(commit by [@palant](https://github.com/palant))*
- [`bc57d8e`](https://github.com/palant/pingora-utils/commit/bc57d8eb0c276088ec9fe6b156e48996d5063f0e) - **upstream-module**: Added Upstream Module to help configure upstream connections *(commit by [@palant](https://github.com/palant))*
- [`a66fdc6`](https://github.com/palant/pingora-utils/commit/a66fdc6464032a5cb1c5c2425592f3dd74639fc0) - **static-files-module**: If no root is configured, disable the module *(commit by [@palant](https://github.com/palant))*
- [`d591bfe`](https://github.com/palant/pingora-utils/commit/d591bfed04962b9a1feba5fc77e52d6372c2dcea) - **virtual-hosts-module**: Added per-directory configurations *(commit by [@palant](https://github.com/palant))*

### :bug: Bug Fixes
- [`227af31`](https://github.com/palant/pingora-utils/commit/227af314dc0e6eccfabd75ef208291303393d7ed) - **module-utils**: Fixed compile error due to missing symbol *(commit by [@palant](https://github.com/palant))*

### :recycle: Refactors
- [`cb093fd`](https://github.com/palant/pingora-utils/commit/cb093fd85f8f029643daf397db415f60af37b0bc) - **module-utils**: Removed unnecessary type qualifier *(commit by [@palant](https://github.com/palant))*
- [`761229d`](https://github.com/palant/pingora-utils/commit/761229dd8e4b5ed804bb1fcacbf3a75831e0ed9a) - **module-utils**: Renamed `pingora-utils-core` crate into `module-utils` *(commit by [@palant](https://github.com/palant))*
- [`a16fc2f`](https://github.com/palant/pingora-utils/commit/a16fc2f3a017a9627d31949a7da37270b2c56a45) - **module-utils**: Somewhat hardened macros against namespace collisions *(commit by [@palant](https://github.com/palant))*
- [`0d302da`](https://github.com/palant/pingora-utils/commit/0d302daafee13ff34b829937d049b8ac157647a8) - **module-utils**: Turned macros into procedural macros *(commit by [@palant](https://github.com/palant))*
- [`062126e`](https://github.com/palant/pingora-utils/commit/062126e3e9ff68f48069fb8eca87605d0f244611) - **static-files-module**: Do not respond with 404 when not configured, ignore request instead *(commit by [@palant](https://github.com/palant))*

### :wrench: Chores
- [`252d6b6`](https://github.com/palant/pingora-utils/commit/252d6b6369255defc92b73b2278de32a9cf03bc9) - Upgraded to Pingora 0.2.0 *(commit by [@palant](https://github.com/palant))*
- [`d3ad536`](https://github.com/palant/pingora-utils/commit/d3ad536f90de9290183a21413b4d11bcd89fe9d3) - Releasing version 0.2.0 *(commit by [@palant](https://github.com/palant))*

[v0.2.0]: https://github.com/palant/pingora-utils/compare/v0.1.0...v0.2.0

## [v0.1.0] - 2024-05-08

This is the initial release. It includes Static Files Module with the following functionality:

* `GET` and `HEAD` requests
* Configurable directory index files (`index.html` by default)
* Page configurable to display on 404 Not Found errors instead of the standard error page
* Conditional requests via `If-Modified-Since`, `If-Unmodified-Since`, `If-Match`, `If-None` match HTTP headers
* Byte range requests via `Range` and `If-Range` HTTP headers
* Compression support: serving pre-compressed versions of the files (gzip, zlib deflate, compress, Brotli, Zstandard algorithms supported)
* Compression support: dynamic compression via Pingora (currently gzip, Brotli and Zstandard algorithms supported)

[v0.1.0]: https://github.com/palant/pingora-utils/compare/6821ef9...v0.1.0
