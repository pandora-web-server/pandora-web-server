# Custom module example

This web server mostly replicates the `default-single-host` preset of the Pandora Web Server.
There are no virtual hosts, all modules are placed at the top level. Instead of using Upstream
or Static Files modules to display content, a custom Web App module produces the content.

The Web App module provides configuration of the handled routes via both configuration file and
command line options. Configured routes are matched via `matchit` crate and the configured
responses for the respective route are produced.

The file `config.yaml` in this directory provides an example configuration. You can run the
example with this configuration file using the following command:

```sh
cargo run -- -c config.yaml
```
