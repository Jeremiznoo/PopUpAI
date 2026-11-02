# Security policy

## Reporting a vulnerability

A private reporting channel has **not been set up yet**. Before the first public
release, enable GitHub private vulnerability reporting on the repository (or
publish a security contact) and replace this paragraph. Until then, do not open
public issues for security problems.

## Security model

The full threat table is in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#security-model). In short:

- **Secrets:** API keys are stored only in the operating system secret store
  (Secret Service or KWallet on Linux, Credential Manager on Windows, Keychain on
  macOS). They are never written to configuration files, logs, URLs, crash
  reports or Git. If no store is available the app refuses to save a key instead
  of falling back to plaintext.
- **Logging:** structured and sanitised. Keys, authorisation headers, prompts and
  answers are not logged at default levels.
- **Network:** HTTPS by default; plain HTTP only for loopback or with an explicit,
  visible per-provider opt-in. Redirects are not followed for authenticated
  requests. Credentials are bound to their provider's origin.
- **Model output is untrusted:** rendered without HTML, without automatic remote
  image loading, and with links opened only on explicit click.
- **Privacy:** no telemetry, analytics, screenshots or clipboard reads. History is
  off by default.

## Repository hygiene

- `.env` and key material are git-ignored; `.env.example` holds placeholders only.
- Recommended before every push: a secret scanner such as `gitleaks` as a
  pre-commit hook and in CI. Test fixtures contain obviously fake keys (alphabet
  sequences) to exercise redaction; allow-list them in the scanner configuration
  rather than weakening the tests.
- Dependencies: commit `Cargo.lock`, run `cargo audit` and `cargo deny check` in
  CI, and review new dependencies before adding them.
- If a real credential is ever committed, treat it as compromised: revoke it at
  the provider first, then clean the history.
