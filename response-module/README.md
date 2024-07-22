# Response module for Pandora Web Server

The Response module allows producing a response from configuration:

```yaml
response: "Look, the server is working!"
response_headers:
  Content-Type: text/html
```

Unless this is a maintenance message, you’ll usually want to limit its scope via the Virtual Hosts module:

```yaml
vhosts:
  localhost:
    default: true
    subpaths:
      /file.txt:
        response: "This is not an actual file."
        response_headers:
          Content-Type: text/plain
```

## Configuration settings

| Configuration setting   | Type        | Default value | Description |
|-------------------------|-------------|---------------|-------------|
| `response`              | string      |               | The response to be produced. This setting activates the module. |
| `response_status`       | integer     | 200           | The HTTP status code of the response |
| `response_headers`      | map         |               | The HTTP headers to be added to the response |
