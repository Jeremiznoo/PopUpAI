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
- **Model output is untrusted:** the core renders Markdown to a fixed whitelist of tags
  with no attribute at all. Raw HTML is escaped, images show only their description, and a
  link shows its text followed by its address as plain text, so nothing is fetched and
  nothing can be clicked. Tests assert this on hostile inputs.
- **Typing an API key:** the field is masked and the key goes straight to the system secret
  store (an encrypted Secret Service session) and is cleared from the field and from the
  application's own copy. A copy inside Qt's text engine can outlive the field until it is
  overwritten; that residual exposure is not eliminated, only limited in time.
- **Where a key may be sent:** each key is bound to the origin it was saved for, and
  refused for any other, so editing an endpoint in the settings file cannot redirect it.
- **The D-Bus interface** of the running application has four methods (Toggle, Show,
  Hide, Quit) and none takes an argument, so another program of the same user cannot put
  text in the popup.
- **Privacy:** no telemetry, analytics, screenshots or clipboard reads. History is
  off by default.

## Known limits

- The shortcut, tray and wallet code is tested against mocks written from the
  specifications, not against KDE. See
  [docs/PLATFORM_COMPATIBILITY.md](docs/PLATFORM_COMPATIBILITY.md).
- The hosted providers have not been called live yet, so their wire formats are checked
  against documentation and fixtures only.
- Any process of the same user can read the user's session bus and, once the wallet is
  unlocked, ask the Secret Service for items. That is the trust model of the Linux desktop
  secret stores, not something PopUpAI can change.

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
