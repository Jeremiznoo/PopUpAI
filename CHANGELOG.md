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
- Containerised development environment on a dedicated Exegol container
  (`scripts/dev.sh`, `rust-toolchain.toml`): nothing is installed on the host.
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
