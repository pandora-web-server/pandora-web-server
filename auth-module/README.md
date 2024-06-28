# Auth Module for Pandora Web Server

This crate allows putting up an authentication check before further processing of the request
happens. Only authorized users will be able to access the content. This barrier can apply to
the entire web server or, with the help of the Virtual Hosts Module, only a single virtual
host/subdirectory.

A configuration could look like this:

```yaml
auth_mode: page
auth_credentials:
    me: $2y$12$iuKHb5UsRqktrX2X9.iSEOP1n1.tS7s/KB.Dq3HlE0E6CxlfsJyZK
    you: $2y$12$diY.HNTgfg0tIJKJxwmq.edEep5RcuAuQaAvXsP22oSPKY/dS1IVW
```

This sets up two users `me` and `you` with their respective password hashes, corresponding with
the passwords `test` and `test2`.

## Common settings

The following settings always apply:

* `auth_credentials`: A map containing user names and respective password hashes (see “Password
  hashes” section below). The module activates only if this setting has some entries.
* `auth_mode`: Either `http` or `page`. The former uses
  [Basic access authentication](https://en.wikipedia.org/wiki/Basic_access_authentication), the
  latter (default) displays a web page to handle logging in.
* `auth_display_hash`: If `true`, unsuccessful login attempts will display the password hash
  for the entered password (see “Password hashes” section below).
* `auth_rate_limits`: Login rate limits to prevent denial-of-service attacks against the server
  by triggering the (necessarily slow) login validation frequently. This can contain three
  entries `total`, `per_ip` and `per_user`, the default values are 16, 4 and 4 respectively.
  The value 0 disables the rate limiting category. Note that the default values might be too
  low for `http` mode where each server request is considered a login attempt.

## `http` mode settings

In `http` mode the `auth_realm` setting also applies. It determines the “realm” parameter sent
to the browser in the authentication challenge. Modern browsers no longer display this
parameter to the user, but will automatically use the same credentials when encountering
website areas with identical “realm.”

## `page` mode settings

In `page` mode several additional settings apply:

* `auth_page_strings`: This map allows adjusting the text of the default login page. The
  strings `title` (page title), `heading` (heading above the login form), `error` (error
  message on invalid login), `username_label` (label of the user name field), `password_label`
  (label of the password field), `button_text` (text of the submit button) can be specified.
* `auth_page_session`: Various session-related parameters:
  * `login_page`: An optional path of the login page to use instead of the default page.
    *Note*: while the request to the login page will be allowed, requests to resources used by
    that page won’t be. These resources either have to be placed outside the area protected by
    the Authentication Module, or the page can use inline resource and `data:` URIs to avoid
    dependencies.
  * `token_secret`: Hex-encoded secret used to sign tokens issued on successful login. Normally
    you should generate 16 bytes (32 hex digits) randomly. If this setting is omitted, a secret
    will be randomly generated during server startup. While this is a viable option, a server
    restart will always invalidate all previously issued tokens, requiring users to log in
    again.
  * `cookie_name`: The cookie used to store the token issued upon successful login.
  * `secure_cookie`: If set, determines explicitly whether the `Secure` attribute should be
    used for the login cookie. Default behavior is to set this attribute for HTTPS sessions.
  * `session_expiration`: The time interval after which a login session will expire, requiring
    the user to log in again. This interval can be specified in hours (e.g. `2h`) or days (e.g.
    `7d`). *Note*: Changing this setting will have no effect on already issued tokens.
* `auth_redirect_prefix`: This setting should be specified when using Virtual Hosts Module with
  `strip_prefix` set to `true`. It should be set to the subdirectory that Authentication Module
  applies to, to ensure correct redirects after logging in.

## Password hashes

The supported password hashes use the [bcrypt algorithm](https://en.wikipedia.org/wiki/Bcrypt)
and should start with either `$2b$` or `$2y$`. While `$2a$` and `$2x$` hashes can be handled as
well, using these is discouraged due to implementation bugs.

A hash can be generated using the `htpasswd` tool distributed along with the Apache web server:

```sh
htpasswd -nBC 12 user
```

Alternatively, you can use this module to generate a password hash for you:

1. To activate the module, make sure the `auth_credentials` setting isn’t empty. It doesn’t
   have to contain a valid set of credentials, any value will do.
2. Add the `auth_display_hash: true` setting to your configuration.
3. Run the server and navigate to the password protected area with your browser.
4. When prompted by the browser, enter the credentials you want to use.
5. When using `http` mode, close the prompt when the browser prompts you for credentials again.
   You should see the “401 Unauthorized” page then.

The page will contain a configuration suggestion with the generated credentials. You can remove
the `auth_display_hash: true` setting now.

## Code example

You would normally put this handler in front of other handlers, such as the Static Files
Module. The `pandora-module-utils` and `startup-module` crates provide helpers to simplify
merging of configuration and the command-line options of various handlers as well as creating
a server instance from the configuration:

```rust
use auth_module::{AuthHandler, AuthOpt};
use clap::Parser;
use pandora_module_utils::{merge_conf, merge_opt, FromYaml, RequestFilter};
use startup_module::{DefaultApp, StartupConf, StartupOpt};
use static_files_module::{StaticFilesHandler, StaticFilesOpt};

#[derive(Debug, RequestFilter)]
struct Handler {
    auth: AuthHandler,
    static_files: StaticFilesHandler,
}

#[merge_conf]
struct Conf {
    startup: StartupConf,
    handler: <Handler as RequestFilter>::Conf,
}

#[merge_opt]
struct Opt {
    startup: StartupOpt,
    auth: AuthOpt,
    static_files: StaticFilesOpt,
}

let opt = Opt::parse();
let mut conf = Conf::load_from_files(opt.startup.conf.as_deref().unwrap_or(&[])).unwrap();
conf.handler.auth.merge_with_opt(opt.auth);
conf.handler.static_files.merge_with_opt(opt.static_files);

let app = DefaultApp::<Handler>::from_conf(conf.handler).unwrap();
let server = conf.startup.into_server(app, Some(opt.startup)).unwrap();

// Do something with the server here, e.g. call server.run_forever()
```
