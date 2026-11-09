# Security policy

## Supported version

Security fixes are provided for the latest published release.

## Reporting a vulnerability

Do not include subscription URLs, passwords, private keys, cookies, diagnostic
archives or other secrets in a public issue. Open a GitHub security advisory for
the repository and include the affected version, reproduction steps and impact.

If the repository does not yet expose private vulnerability reporting, open a
public issue containing no exploit or secret and ask the maintainer to establish
a private channel.

## Security boundaries

- Secrets belong in Windows Credential Manager or macOS Keychain.
- Update artifacts and publisher catalogs must pass signature verification.
- Gateway and remote-control changes use preview, hash gates and rollback.
- AI strict routing fails closed rather than silently using a direct connection.
