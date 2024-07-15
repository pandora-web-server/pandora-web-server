# Compression module for Pandora Web Server

The Compression module exposes two features provided by Pingora: dynamic compression of responses and decompression of upstream responses.

## Current limitation

When compressing, Pingora will currently only consider the first algorithm listed in the `Accept-Encoding` header. If compression is disabled for this algorithm, the response will not be compressed despite other supported algorithms present. For this reason it is currently recommendable to enable all compression algorithms.

## Configuration settings

| Configuration setting      | Command line                 | Type    | Default value | Description |
|----------------------------|------------------------------|---------|---------------|-------------|
| `compression_level_gzip`   | `--compression-level_gzip`   | integer |               | If present, enables dynamic gzip compression of server responses and sets the compression level |
| `compression_level_brotli` | `--compression-level_brotli` | integer |               | If present, enables dynamic Brotli compression of server responses and sets the compression level |
| `compression_level_zstd`   | `--compression-level_zstd`   | integer |               | If present, enables dynamic Zstandard compression of server responses and sets the compression level |
| `decompress_upstream`      | `--decompress-upstream`      | boolean | `false`       | If `true`, upstream responses using compression not supported by the client will be decompressed |
