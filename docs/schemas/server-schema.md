# Server Schema

Server configuration defines SSH server connections stored in `servers/<id>.json`.

## Schema

```json
{
  "id": "string",
  "name": "string",
  "host": "string",
  "port": number,
  "user": "string",
  "identity_file": "string",
  "forward_agent": boolean
}
```

## Fields

### Required Fields

- **`id`** (string): Unique server identifier
- **`name`** (string): Human-readable server name
- **`host`** (string): Server hostname or IP address
- **`user`** (string): SSH username for authentication

### Optional Fields

- **`port`** (number): SSH port (default: 22)
- **`identity_file`** (string): Path to SSH private key file for authentication
- **`forward_agent`** (boolean): Enable SSH agent forwarding (default: false)

## Example

```json
{
  "id": "production",
  "name": "Production Server",
  "host": "example.com",
  "port": 22,
  "user": "deploy",
  "identity_file": "/Users/dev/.ssh/id_rsa",
  "forward_agent": true
}
```

## SSH Key Management

Homeboy manages SSH keys in two ways:

### Identity Files

SSH keys referenced in `identity_file` should exist on the local filesystem. Common locations:
- macOS/Linux: `~/.ssh/id_rsa`, `~/.ssh/id_ed25519`
- Windows: `%USERPROFILE%\.ssh\id_rsa`

### Keychain Integration

SSH key passphrases are stored in the OS keychain:
- macOS: Keychain Access
- Linux: libsecret / gnome-keyring
- Windows: Windows Credential Manager

Homeboy automatically retrieves passphrases from the keychain when establishing SSH connections.

## Storage Location

Servers are stored as individual JSON files under the OS config directory:
- **macOS/Linux**: `~/.config/homeboy/servers/<id>.json`
- **Windows**: `%APPDATA%\homeboy\servers\<id>.json`

## SSH Key Generation

Generate a new SSH key pair:

```bash
ssh-keygen -t ed25519 -C "your@email.com"
```

Copy the public key to the remote server:

```bash
ssh-copy-id user@hostname
```

Or manually:

```bash
cat ~/.ssh/id_ed25519.pub | ssh user@hostname "mkdir -p ~/.ssh && cat >> ~/.ssh/authorized_keys"
```

## SSH Agent Forwarding

When `forward_agent` is enabled, SSH keys from your local agent are forwarded to the server. This allows:

- Git operations on the remote server using your local credentials
- Access to other servers that trust your SSH keys
- Reduced need for keys on remote servers

Enable agent forwarding cautiously - it grants the remote server access to your forwarded SSH identities.

## Related

- [Server command](../commands/server.md) - Manage server configuration
- [SSH key management](../architecture/ssh-key-management.md) - Detailed SSH key handling
- [SSH command](../commands/ssh.md) - Remote shell access
