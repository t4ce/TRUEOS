Vendored from the crates.io `sunset` 0.5.0 release (0BSD; see LICENSE).

TRUEOS extensions:

- `alloc`/`std` forward allocation support to `ssh-key` only when that optional
  dependency is already enabled; allocation alone does not enable the unused
  OpenSSH key-file parser.

- RFC 4256 server keyboard-interactive authentication with one non-echoed
  authenticator-code prompt and exactly one response. Ordinary password and
  public-key methods remain independently configurable.
- The existing PasswordAuth event exposes the code response. `defer()` retains
  that event until `finish_deferred_auth()` resolves a durable verification.
  The application must not call `progress()` while authentication is deferred.
- Public server authentication-method configuration and graceful session exit.
- PTY dimensions and window-change notifications exposed to the server application.
- Authentication responses redact Debug output; decoding errors omit raw bytes.

The client implementation does not support keyboard-interactive authentication.
TRUEOS tests exercise this server extension with the system OpenSSH client in
`tools/testpy/test_shell3_ssh.py`.
