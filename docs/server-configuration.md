# Pandora Web Server configuration

Pandora Web Server is configured via YAML files. A configuration could look like this:

```yaml
anonymization_enabled: true
vhosts:
  localhost:8080:
    aliases:
    - 127.0.0.1:8080
    - "[::1]:8080"
    root: ./local-debug-root
  example.com:
    default: true
    compression_level: 3
    root: ./production-root
```

You specify one or multiple files to be used via the `--conf` command line option, for example:

```sh
pandora-web-server --conf "config/*.yaml"
```

Or listing the files explicitly:

```sh
pandora-web-server --conf config/config1.yaml --conf config/config2.yaml --conf config/config3.yaml
```

## Available configuration options

The available configuration options depend on the modules compiled into the web server and their placement. For the default build the structure looks as follows:

* [Startup settings](startup-module.md#configuration-settings)
* [IP Anonymization settings](ip-anonymization-module.md#configuration-settings)
* [Headers settings](headers-module.md#configuration-settings)
* `vhosts:`
  * `example.com:`
    * [Virtual Hosts host settings](virtual-hosts-module.md#host-configuration)
    * [Common Log settings](common-log-module.md#configuration-settings)
    * [Compression settings](compression-module.md#configuration-settings)
    * [Authentication settings](auth-module.md#configuration-settings)
    * [Rewrite settings](rewrite-module.md#configuration-settings)
    * [Upstream settings](upstream-module.md#configuration-settings)
    * [Static Files settings](static-files-module.md#configuration-settings)
    * `subpaths:`
      * `/dir:`
        * [Virtual Hosts subpath settings](virtual-hosts-module.md#subpath-configuration)
        * [Common Log settings](common-log-module.md#configuration-settings)
        * [Compression settings](compression-module.md#configuration-settings)
        * [Authentication settings](auth-module.md#configuration-settings)
        * [Rewrite settings](rewrite-module.md#configuration-settings)
        * [Upstream settings](upstream-module.md#configuration-settings)
        * [Static Files settings](static-files-module.md#configuration-settings)

The `default-single-host` preset has all modules configured at the top level, so the configuration file structure looks as follows:

* [Startup settings](startup-module.md#configuration-settings)
* [IP Anonymization settings](ip-anonymization-module.md#configuration-settings)
* [Common Log settings](common-log-module.md#configuration-settings)
* [Compression settings](compression-module.md#configuration-settings)
* [Headers settings](headers-module.md#configuration-settings)
* [Authentication settings](auth-module.md#configuration-settings)
* [Rewrite settings](rewrite-module.md#configuration-settings)
* [Upstream settings](upstream-module.md#configuration-settings)
* [Static Files settings](static-files-module.md#configuration-settings)

## Command line options

Some modules can also be configured via command line options. Typically, these have the same name as configuration file settings but with underscores `_` replaced by dashes `-`. For example, the configuration file setting `anonymization_enabled` corresponds to the command line flag `--anonymization-enabled`.

The module’s command line options are only available if the module in question is configured at the top level, not within a virtual host configuration. To see the command line options available for your build, run:

```sh
pandora-web-server --help
```

## Configuration merging

When multiple configuration files are provided, their settings are merged on the fly. For example, if `config1.yaml` is the following:

```yaml
vhosts:
  localhost:8080:
    aliases:
    - 127.0.0.1:8080
    - "[::1]:8080"
    root: ./local-debug-root
    index_file: index.html
```

And `config2.yaml` is:

```yaml
anonymization_enabled: true
vhosts:
  localhost:8080:
    root: ./other-local-debug-root
    index_file: index.txt
  example.com:
    default: true
    compression_level: 3
    root: ./production-root
    index_file:
    - index.html
    - index.txt
```

Then running `pandora-web-server --conf config1.yaml --conf config2.yaml` is effectively the same as giving it the following configuration file:

```yaml
anonymization_enabled: true
vhosts:
  localhost:8080:
    aliases:
    - 127.0.0.1:8080
    - "[::1]:8080"
    root: ./other-local-debug-root
    index_file:
    - index.html
    - index.txt
  example.com:
    default: true
    compression_level: 3
    root: ./production-root
    index_file:
    - index.html
    - index.txt
```

The merging of individual configuration entries depends on their type. Lists are merged by joining their entries from all configuration files. Maps are merged similarly, except when map entries exist in multiple configuration files: the entry values are then themselves merged. For other types, each configuration file applied overwrites existing values so that the last configuration file applied wins.

## Configuration file ordering

The order in which configuration files are applied does *not* depend on their ordering in the command line. Instead, the list of configuration files is sorted alphabetically, and the files are applied in that order.

If applying the configuration files in a particular order is important to you, you can add a numeric prefix to your configuration file names. For example:

```
10-main.yaml
20-local.yaml
30-upstream.yaml
40-tls.yaml
```

The files will always be applied in this order then. The values configured in the files with a higher numeric prefix will take precedence over those in the files with a lower numeric prefix.

Command line options are always applied last, after processing all configuration files. Typically, no merging is performed for command line options, the existing configuration is overwritten even in case of lists.

## Specifying lists

You can always specify list settings as YAML lists, using both inline and multi-line syntax:

```yaml
listen: [127.0.0.1:8080, "[::1]:8080"]
response_headers:
  cache_control:
  - max-age: 604800
    include: localhost:8080
  - max-age: 3600
    include: localhost:8080/uncompressed/*
```

For lists with only one entry, you can reduce the syntactical overhead by omitting the list syntax:

```yaml
listen: "[::]:8080"
response_headers:
  cache_control:
    max-age: 604800
    include: localhost:8080
```
