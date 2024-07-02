# Virtual Hosts module for Pandora Web Server

The Virtual Hosts module allows applying configuration of other modules not to the entire web presence but only to a specific virtual host or a subpath of a host. For example, the configuration of a server displaying different static files directories for different hosts might look like this:

```yaml
vhosts:
  localhost:8000:
    root: ./local-debug-root
  [example.com, www.example.com]:
    default: true
    root: ./production-root
    subpaths:
      /metrics/*:
        root: ./metrics
      /test/*:
        strip_prefix: true
        root: ./local-debug-root
      /test/file.txt:
        root: ./production-root
```

## Matching configuration to the request

Matching a host configuration always requires an exact match. If the server runs on a non-default port (i.e. not 80 for HTTP or 443 for HTTPS), the port number will also be part of the host name and needs to be specified. All requests where no specific host configuration applies will be handled with the default host configuration if one exists.

Subpath matching on the other hand supports both exact matches (e.g. `/test`) and prefix matches (e.g. `/test/*`). The former will match both `/test` and `/test/` requests whereas the latter will also match `/test/file.txt`. As matching always happens at the file name boundary, the request `/test_abc` will not be matched by either rule.

It can happen that multiple subpath configuration potentially apply to a request. In these scenarios a “closer” match (the configurations with a longer path) is preferred. Should both an exact and a prefix match exist, the former will be preferred.

## Prefix stripping caveats

The `strip_prefix` setting is useful for example when serving static files in a subdirectory of the webspace without actually reflecting the subdirectory name in the file structure. If the configuration is for `/subdir/*` then the Static Files module will see a request for `/file.txt` rather than one for `/subdir/file.txt`, and you don’t need to put the files into a `subdir` directory on disk.

Things get complicated when the handler does something with the provided URI such as displaying links or performing a redirect. The Static Files and the Auth modules know to perform redirects using the original request URI, making certain to still redirect to the correct location. In other cases such as responses from upstream servers, the response might have to be modified before it is passed on.

## Configuration settings

| Configuration setting   | Type    | Default value | Description |
|-------------------------|---------|---------------|-------------|
| `vhosts`                | map     |               | Maps host names or lists of host names to their respective [host configuration](#host-configuration) |

## Host configuration

The host configuration contains the settings of all the modules that were combined into the host handler. The following table contains only the additional settings provided by the Virtual Hosts module.

| Configuration setting   | Type    | Default value | Description |
|-------------------------|---------|---------------|-------------|
| `default`               | boolean | `false`       | If `true`, requests for hosts not matching any specific host configuration will be handled by this host configuration |
| `subpaths`              | map     |               | Maps paths (e.g. `/test`) or path prefixes (e.g. `/path/*`) to their respective [subpath configuration](#subpath-configuration) |

## Subpath configuration

The subpath configuration contains the settings of all the modules that were combined into the host handler. The following table contains only the additional settings provided by the Virtual Hosts module.

| Configuration setting   | Type    | Default value | Description |
|-------------------------|---------|---------------|-------------|
| `strip_prefix`          | boolean | `false`       | If `true`, the host handler will receive the request URI with the path part used to match the configuration removed |
