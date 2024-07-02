# Common Log module for Pandora Web Server

The Common Log module provides access logging in the [Common Log Format](https://en.wikipedia.org/wiki/Common_Log_Format). Each line in the access log corresponds to one request. Such access log files can be processed further by a variety of tools. A configuration could look like this:

```yaml
log_file: access.log
log_format: [
    remote_addr, -, remote_name, time_local, request, status, bytes_sent, http_referer, http_user_agent
]
```

## Character escaping

Quoted values in the log can contain unprintable or non-ASCII characters. Such characters will be printed as a hex encoded sequence like `\x1f`. This is applied to all characters with character codes below 32 or above 127 as well as quotation marks `"` and backslashes `\`.

## Reopening log files

On Unix-based systems, the process can be sent a `HUP` or `USR1` signal to make it re-open all log files. This is useful after the logs have been rotated for example. The existing logs will be released then and the next request will result in new log files being created.

## Configuration settings

| Configuration setting   | Command line    | Type               | Default value | Description |
|-------------------------|-----------------|--------------------|---------------|-------------|
| `log_file`              | `--log-file`    | file path          | `-`           | File to write logs to or `-` to write to stdout |
| `log_format`            |                 | list of [log fields](#supported-log-fields) | `[remote_addr, -, remote_name, time_local, request, status, bytes_sent, http_referer, http_user_agent]` | Log fields to write to the file |

### Supported log fields

The following log fields are currently supported:

* `-`: Verbatim `-` character (for unsupported fields)
* `remote_addr`: client’s IP address
* `remote_port`: client’s TCP port
* `remote_name`: authorized user’s name if any
* `time_local`: date and time of the request, e.g. `[10/Oct/2000:13:55:36 -0700]`
* `time_iso8601`: date and time in the ISO 8601 format, e.g. `[2000-10-10T13:55:36-07:00]`
* `request`: quoted request line, e.g. `"GET / HTTP/1.1"`
* `status`: status code of the response, e.g. `200`
* `bytes_sent`: number of bytes sent as response
* `processing_time`: time from request being received to response in milliseconds
* `http_<header>`: quoted value of an HTTP request header. For example, `http_user_agent` adds
  the value of the `User-Agent` HTTP header to the log.
* `sent_http_<header>`: quoted value of an HTTP response header. For example,
  `sent_http_content_type` adds the value of the `Content-Type` HTTP header to the log.
