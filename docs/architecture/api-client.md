# API Client System

The API client provides HTTP request capabilities with template-based authentication per project.

## Overview

Homeboy projects can configure an API client for making HTTP requests to project APIs. This supports:

- RESTful API interactions
- Template-based URL and header construction
- Keychain-stored authentication tokens
- JSON request/response handling
- Bulk request operations

## Configuration

API client configuration lives in `projects/<project_id>.json`:

```json
{
  "api": {
    "base_url": "https://example.com/wp-json",
    "enabled": true
  }
}
```

### Configuration Fields

- **`base_url`** (string): Base URL for API requests
- **`enabled`** (boolean): Whether API client is active

## Authentication

Authentication credentials are stored securely in the OS keychain using `homeboy auth`.

### Store Credentials

```bash
homeboy auth <project_id>
```

This prompts for authentication token which is stored in keychain associated with the project ID.

### Keychain Storage

- **macOS**: Keychain Access (service: `homeboy`, account: `homeboy_api_<project_id>`)
- **Linux**: libsecret / gnome-keyring
- **Windows**: Windows Credential Manager

### Retrieved Credentials

Credentials are automatically retrieved from keychain when API requests are made. No manual token management required.

## Template Variables

API client templates support variables for constructing dynamic requests.

### Available Variables

- **`{{projectId}}`**: Project ID
- **`{{domain}}`**: Project domain
- **`{{apiUrl}}`**: Full API URL (base_url + endpoint)
- **`{{token}}`**: Authentication token from keychain

### Template Usage

Templates use standard Homeboy template syntax (both `{var}` and `{{var}}` supported).

## API Commands

### Make Request

```bash
homeboy api <project_id> <method> <endpoint> [options]
```

**Arguments:**
- `<project_id>`: Project identifier
- `<method>`: HTTP method (`GET`, `POST`, `PUT`, `DELETE`, `PATCH`)
- `<endpoint>`: API endpoint path (appended to `base_url`)

**Options:**
- `--data <json>`: Request body (JSON)
- `--params <key=value>`: Query parameters (repeatable)
- `--header <key=value>`: Request headers (repeatable)
- `--output <path>`: Save response to file
- `--json`: Return response as JSON (when applicable)

**Examples:**

```bash
# GET request
homeboy api myproject GET /posts

# POST with data
homeboy api myproject POST /posts --data '{"title": "Hello", "content": "World"}'

# With query parameters
homeboy api myproject GET /posts --params page=1 --params per_page=10

# Custom header
homeboy api myproject GET /posts --header "Authorization: Bearer {{token}}"

# Save response
homeboy api myproject GET /posts --output /tmp/posts.json
```

### Bulk Requests

```bash
homeboy api --json <spec>
```

**JSON Spec Format:**

```json
{
  "project_id": "myproject",
  "requests": [
    {
      "method": "GET",
      "endpoint": "/posts",
      "params": {"page": 1, "per_page": 10},
      "output": "/tmp/posts.json"
    },
    {
      "method": "POST",
      "endpoint": "/posts",
      "data": {"title": "New Post", "content": "Content"}
    }
  ]
}
```

## Module Integration

Modules can define API actions in their manifest for automated API interactions.

### Module Action API

```json
{
  "actions": {
    "sync_posts": {
      "type": "api",
      "description": "Sync posts from API",
      "config": {
        "method": "GET",
        "endpoint": "/posts",
        "params": {"per_page": 100}
      }
    }
  }
}
```

### Template Variables in Module Actions

Module API actions can use template variables:

```json
{
  "config": {
    "method": "POST",
    "endpoint": "/posts/{{postId}}/comments",
    "template": {
      "content": "{{payload.comment}}"
    }
  }
}
```

## Response Handling

### Success Responses

Successful API requests return the response body. Content type is respected:
- JSON responses are formatted and returned
- Text responses are returned as-is
- Binary responses can be saved to file via `--output`

### Error Responses

Failed requests return error information:
- HTTP status code
- Error message (if provided by API)
- Request details for debugging

### JSON Output

All API commands return responses wrapped in the global JSON envelope:

```json
{
  "success": true,
  "data": {
    "command": "api.request",
    "method": "GET",
    "endpoint": "/posts",
    "status_code": 200,
    "response": {
      "posts": []
    }
  }
}
```

## Use Cases

### WordPress REST API

```json
{
  "api": {
    "base_url": "https://example.com/wp-json",
    "enabled": true
  }
}
```

```bash
# List posts
homeboy api myproject GET /wp/v2/posts

# Create post
homeboy api myproject POST /wp/v2/posts --data '{"title": "New Post"}'
```

### Custom API

```json
{
  "api": {
    "base_url": "https://api.example.com/v1",
    "enabled": true
  }
}
```

```bash
# Make authenticated request
homeboy api myproject GET /users
```

## Security Considerations

1. **Never store tokens in project JSON**: Always use `homeboy auth` command
2. **Use HTTPS**: API base URLs should use HTTPS for secure communication
3. **Keychain storage**: Tokens are encrypted in OS keychain
4. **Token rotation**: Use `homeboy auth` to update tokens when they change

## Related

- [Auth command](../commands/auth.md) - Manage API authentication
- [API command](../commands/api.md) - Make API requests
- [Project schema](../schemas/project-schema.md) - API configuration structure
- [Keychain/secrets management](./keychain-secrets.md) - How secrets are stored
