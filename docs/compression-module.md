# Compression module for Pandora Web Server

The Compression module exposes two features provided by Pingora: dynamic compression of responses and decompression of upstream responses.

## Current limitation

While Pingora supports gzip, brotli and zstd compression, currently only one setting for compression level exists which applies to all three algorithms. This issue [will be addressed in the next Pingora release](https://github.com/cloudflare/pingora/issues/228).

## Configuration settings

| Configuration setting   | Command line              | Type    | Default value | Description |
|-------------------------|---------------------------|---------|---------------|-------------|
| `compression_level`     | `--compression-level`     | integer |               | If present, enables dynamic compression of server responses and sets the compression level for all algorithms |
| `decompress_upstream`   | `--decompress-upstream`   | boolean | `false`       | If `true`, upstream responses using compression not supported by the client will be decompressed |
