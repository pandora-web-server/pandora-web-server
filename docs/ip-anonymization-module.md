# IP Anonymization module for Pandora Web Server

The IP Anonymization module improves privacy of website visitors by removing a part of their IP addresses. As a result, only a censored IP address is written to various logs, never the full address.

## Anonymization approach

When given an IPv4 address, this module removes the last octet: the address `192.0.2.3` for example becomes `192.0.2.0`.

With IPv6, all but the first two groups are removed: the address `2001:db8:1234:5678::2` for example becomes `2001:db8::`.

## Current limitation

Due to a [Pingora limitation](https://github.com/cloudflare/pingora/issues/270), the original IP address cannot be completely removed at the moment. Code that dereferences `SessionWrapper` into the original Pingora `Session` data structure or code accessing `session.digest()` directly will still be able to access the original IP address. This will be addressed when the next Pingora version is out and Pandora Web Server starts using it.

## Configuration settings

| Configuration setting   | Command line              | Type    | Default value | Description |
|-------------------------|---------------------------|---------|---------------|-------------|
| `anonymization_enabled` | `--anonymization-enabled` | boolean | `false`       | If `true`, IP address anonymization  will be enabled |
