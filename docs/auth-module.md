# Authentication module for Pandora Web Server

The Auth module restricts access to the web server contents to authorized users only. When used in conjunction with the Virtual Hosts module, this authorization requirement can be limited to a single virtual host or subpath.

This module supports two operation modes:

* In the `page` mode (default) logging in is handled by a web page. Successful logins are remembered using an HTTP cookie.
* In the `http` mode this module uses [Basic access authentication](https://en.wikipedia.org/wiki/Basic_access_authentication). Logging in is handled by the browser and isn’t configurable. Even after a successful login, the user’s credentials are sent with each request and have to be validated every time.

A very basic configuration could look like this:

```yaml
auth_mode: page
auth_credentials:
  me: $2y$12$iuKHb5UsRqktrX2X9.iSEOP1n1.tS7s/KB.Dq3HlE0E6CxlfsJyZK
  you: $2y$12$diY.HNTgfg0tIJKJxwmq.edEep5RcuAuQaAvXsP22oSPKY/dS1IVW
```

This sets up two users `me` and `you` with their respective password hashes, corresponding to the passwords `test` and `test2`. The module activates when credentials for at least one user are configured.

## Password hashing

The supported password hashes use the [bcrypt algorithm](https://en.wikipedia.org/wiki/Bcrypt)
and should start with either `$2b$` or `$2y$`. While `$2a$` and `$2x$` hashes can be handled as
well, using these is discouraged due to implementation bugs.

A hash can be generated using the `htpasswd` tool distributed along with the Apache web server:

```sh
htpasswd -nBC 12 user
```

Alternatively, you can use this module to generate a password hash for you:

1. To activate the module, make sure the `auth_credentials` setting isn’t empty. It doesn’t have to contain a valid set of credentials, any value will do.
2. Add the `auth_display_hash: true` setting to your configuration.
3. Run the server and navigate to the password protected area in your browser.
4. When prompted, enter the credentials you want to use.
5. When using `http` mode, close the prompt when the browser prompts you for credentials again. You should see the “401 Unauthorized” page then.

The page will contain a configuration suggestion with the generated password hash. You can remove the `auth_display_hash: true` setting now.

Note that password hashing should be slow in order to hinder [brute-force attacks](https://en.wikipedia.org/wiki/Brute-force_attack) on leaked password hashes. The side effect is that a large number of parallel login attempts can slow down the server. To prevent this, [login rate limits](#login-rate-limits) should be enforced.

## Session management

While in `http` mode session management is being performed by the browser, in `page` mode the module needs to set a cookie with a login token after a successful login. The cookie contains a signed [JSON Web Token](https://jwt.io/) proving a successful authentication.

In case of an incident where a JSON Web Token was issued mistakenly or leaked to an unauthorized party, changing credentials in the configuration will *not* have an effect. A JSON Web Token does not depend on these credentials. In order to invalidate this token you have the following options:

1. Make sure the token expires. If your `session_expiration` is set to a too long interval, you can change it to make the token expire sooner.
2. In urgent cases, e.g. when a login session is being actively abused, you can invalidate *all* active login sessions by changing the `token_secret` setting.

The `token_secret` setting doesn’t necessarily have to be configured: if omitted, it will be chosen randomly each time the server starts up. As a result, restarting the server will always invalidate all existing login sessions with such configurations.

## Implementing a custom login page

The `login_page` setting allows providing a URI that will be used as custom login page. This URI will be passed on to subsequent modules and should produce a page. It can be a static file produced by the Static Files module for example.

Note that only the request to the login page itself will be allowed to proceed. Requests to any subresources of the page like scripts will again result in a login prompt. This means that such subressources should either be placed outside the area protected by the Auth module, or the page should use inline resources and `data:` URIs to avoid dependencies.

When a login attempt is made, the page should send a dynamic request to `location.href` with the content type `application/x-www-form-urlencoded`. The following parameters should be specified:

* `username`: User to be logged in
* `password`: The user’s password
* `type`: should be set to `json`

A successful login will result in a response like:

```json
{"success":true}
```

For a failed login attempt `success` will be `false`. There might also be a `suggestion` field if `auth_display_hash` setting is enabled. It will contain a configuration suggestion for the supplied credentials.

The page should also be able to handle HTTP responses other than `200 OK`, in particular `429 Too Many Requests`.

## Configuration settings

| Configuration setting   | Command line          | Type               | Default value | Description |
|-------------------------|-----------------------|--------------------|---------------|-------------|
| `auth_mode`             | `--auth-mode`         | `page` or `http`   | `page`        | Login handling approach, either web page or HTTP Basic access authentication |
| `auth_credentials`      | `--auth-credentials`  | map                |               | Maps user names to the respective password hashes. On command line, values are specified as `user:hash`. |
| `auth_display_hash`     | `--auth-display-hash` | boolean            | `false`       | If `true`, unsuccessful login attempts will result in the login credentials being hashed and this hash displayed |
| `auth_rate_limits`      |                       | [rate limits](#login-rate-limits) |               | Limits for login attempts |
| `auth_page_strings`     |                       | [page strings](#page-strings)     |               | `page` mode only: texts used on the login page |
| `auth_page_session`     |                       | [session settings](#session-settings) |               | `page` mode only: session management settings |
| `auth_realm`            | `--auth-realm`        | string             | `"Server authentication"` | `http` mode only: “realm” parameter sent to the client. Determines which website areas share the same password. |

### Login rate limits

Note that in `http` mode each request (including subresources like scripts or images) is effectively a login attempt, even if the correct credentials have been entered already and the browser is no longer displaying a login prompt. As a results, higher rate limits might be required in this mode.

| Configuration setting   | Type               | Default value | Description |
|-------------------------|--------------------|---------------|-------------|
| `total`                 | integer            | 16            | Total allowed number of login attempts per second |
| `per_ip`                | integer            | 4             | Allowed number of login attempts per IP address per second |
| `per_user`              | integer            | 4             | Allowed number of login attempts per user name per second |

### Page strings

The login page displays a number of texts. All of these can be configured, e.g. when a language other than English should be used.

| Configuration setting   | Type               | Default value   | Description |
|-------------------------|--------------------|-----------------|-------------|
| `title`                 | string             | `Access denied` | Page title as displayed in tab title |
| `heading`               | string             | `Access is restricted, please log in.` | Visible page heading |
| `error`                 | string             | `Invalid credentials, please try again.` | Error message displayed on failed login attempt |
| `username_label`        | string             | `User name:`    | Label of the user name field |
| `password_label`        | string             | `Password:`     | Label of the password field  |
| `button_text`           | string             | `Log in`        | Label of the button to submit the form |

### Session settings

| Configuration setting   | Type               | Default value   | Description |
|-------------------------|--------------------|-----------------|-------------|
| `login_page`            | URI                |                 | If set, the specified page will be used instead of the default login page |
| `token_secret`          | string             | random          | Hex-encoded secret used to sign tokens issued on successful login |
| `cookie_name`           | string             | `token`         | Name of the cookie to store login token |
| `secure_cookie`         | boolean            | `true` for HTTPS | If set, determines explicitly whether the `Secure` flag should be set on the login cookie. |
| `session_expiration`    | time interval      | `7d`            | Time interval in days (e.g. `7d`) or hours (e.g. `2h`) after which a login session should expire |
