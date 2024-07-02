# Static Files module for Pandora Web Server

The Static Files module allows serving static files from a directory.

## Supported functionality

* `GET` and `HEAD` requests
* Configurable directory index files
* A page can be configured to display on `404 Not Found` errors instead of the standard error page.
* Conditional requests via `If-Modified-Since`, `If-Unmodified-Since`, `If-Match`, `If-None` match HTTP headers
* Byte range requests via `Range` and `If-Range` HTTP headers
* Serving pre-compressed versions of files (gzip, zlib deflate, compress, Brotli, Zstandard algorithms supported)

## Known limitations

* Requests with multiple byte ranges are not supported and will result in the full file being returned. The complexity required for implementing this feature isn’t worth this rare use case.
* Zero-copy data transfer (a.k.a. sendfile) cannot currently be supported within the Pingora framework.

## Compression support

You can activate support for selected compression algorithms via the `precompressed` configuration setting, e.g. with this configuration:

```yaml
root: /var/www/html
precompressed:
- gz
- br
```

With this configuration, a request for `/file.txt` might result in the file `/file.txt.gz` or `/file.txt.br` being returned if present in the directory and supported by the client. If multiple supported pre-compressed files exist, one is chosen according to the client’s preferences communicated in the [`Accept-Encoding` HTTP header](https://datatracker.ietf.org/doc/html/rfc7231#section-5.3.4).

If pre-compressed files are disabled or no supported variant is found, the response might still get dynamically compressed. The Compression module can be used to activate dynamic compression.

## Configuration settings

| Configuration setting   | Command line         | Type            | Default value | Description |
|-------------------------|----------------------|-----------------|---------------|-------------|
| `root`                  | `--root`             | directory path  |               | The directory to serve static files from |
| `canonicalize_uri`      | `--canonicalize-uri` | boolean         | `true`        | If `true`, requests to `/file%2etxt` will be redirected to `/file.txt` and requests to `/dir` redirected to `/dir/` |
| `index_file`            | `--index-file`       | list of strings | `[]`          | When a directory is requested, look for these files within to directory and show the first one if found instead of the usual `403 Forbidden` error |
| `page_404`              | `--page-404`         | URI             |               | If set, this page will be displayed instead of the standard `404 Not Found` error |
| `precompressed`         | `--precompressed`    | list of file extensions | `[]`  | File extensions of pre-compressed files to look for. Supported extensions are `gz` (gzip), `zz` (zlib deflate), `z` (compress), `br` (Brotli), `zst` (Zstandard). |
