# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Repository scaffolding: `.gitignore`, `.env.example` (placeholders only).
- Phase 0 environment report.
- Architecture document, platform compatibility matrix and development guide.
- ADR 0001 (technology stack), ADR 0002 (Linux desktop integration),
  ADR 0003 (session and cancellation policy), all in *Proposed* status.
- README, SECURITY and CONTRIBUTING documents.
- Optional Exegol container workflow (`scripts/dev.sh`) and a pinned
  `rust-toolchain.toml`. Development later moved to the host; see Changed.
- Cargo workspace and `popupai-core` with the session state machine from
  ADR 0003, covered by 15 unit tests.
- `popupai-security`: `Secret` (redacted, zeroized), `Endpoint` validation (https
  required, loopback-only http, no embedded credentials), log redaction, and a
  fail-closed `SecretStore` trait. 20 unit tests.
- `popupai-ai`: `Provider` trait and OpenAI, OpenAI-compatible, Anthropic, Gemini
  and Ollama adapters with streaming, cancellation by dropping the stream, model
  listing and a typed error taxonomy. Redirects are never followed and keys are
  only sent in headers. 39 unit tests and 17 end-to-end tests against a local
  scripted server. Not yet verified against the live services.
- `popupai-core` configuration: TOML settings with validation, atomic `0600`
  storage, refusal of keys in the file and of files writable by others.
- API keys bound to the endpoint they were saved for, so editing an endpoint in
  the configuration file cannot redirect a stored key to another host.
- Request orchestrator: one in-flight request, cancellation (Stop, dismissal
  policy, expiry), provider reuse for warm connections, readable error messages,
  and logs that carry only metadata. Verified by a test that captures all logs.
- `ProviderError::Unreachable` for refused connections, and `Debug` output of
  session messages that shows sizes instead of text.

- `popupai-app`: a first one-shot Qt/QML popup connected to the core through a
  cxx-qt controller. It is placed with the `org.kde.layershell` QML module
  (bottom-right, overlay layer, keyboard focus on demand), follows the Plasma
  colour scheme, and shows the answer as plain text on purpose, since model output
  is untrusted. It starts and loads headless; it has not been shown on screen yet.
- Spike S1 to S3 (`spikes/s1-layer-shell`) and `tools/fake_ollama.py` to try the
  interface without a model.

### Fixed

- Keyless local providers such as Ollama now work when the system secret store is
  unavailable; providers that need a key still fail with a clear explanation.

### Changed

- Development moved from a container to the Fedora host, because the desktop
  integration (popup, shortcut portal, tray, Secret Service, performance) can only
  be validated in the real session. The build dependencies are installed with the
  command in `docs/DEVELOPMENT.md`; the container is kept as an optional legacy
  workflow. All 131 tests pass natively, and a Qt Quick plus LayerShellQt program
  and a minimal cxx-qt crate build against Qt 6.11.2.
