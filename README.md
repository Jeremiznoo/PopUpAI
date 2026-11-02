# PopUpAI

An AI quick-ask launcher. Press a global shortcut, a small popup appears in the
corner of your screen with the input already focused, type a question, read the
streamed answer, click anywhere else and it disappears. Each opening is a fresh,
unsaved session.

> **Status: pre-alpha, design phase.** There is no runnable application yet.
> Everything below describes the target, and is marked with what is verified.

## Goals

Fast, lightweight, private by default, provider-agnostic, native on the desktop.
No telemetry, no forced account, no hidden conversation history.

## Target platforms

| Platform | State |
|---|---|
| Fedora Linux + KDE Plasma 6 (Wayland) | Primary target. Environment audited, design written, nothing exercised yet. |
| KDE on X11 | Planned (via Qt `xcb`). |
| Windows 10/11, macOS | Architecture allows it. Not started. |

Details and the honest verification state: [docs/PLATFORM_COMPATIBILITY.md](docs/PLATFORM_COMPATIBILITY.md).

## Planned providers

OpenAI, Anthropic, Google Gemini, Ollama (local), and any OpenAI-compatible
endpoint (LM Studio, LocalAI, vLLM, OpenRouter, and so on). Model identifiers are
free text; no closed model list. Local models are never downloaded automatically.

State: the adapters are implemented and tested against a local scripted server,
but have **not** yet been run against the live services.

## Security and privacy model

API keys go in the operating system's secret store, never in files, logs or Git.
If no secret store is available the app says so and does not fall back to
plaintext. Prompts and answers are not logged by default and not persisted by
default. See [SECURITY.md](SECURITY.md) and the threat table in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#security-model).

## Documentation

- [Architecture](docs/ARCHITECTURE.md) and [decision records](docs/ADR/)
- [Environment report](docs/ENVIRONMENT_REPORT.md)
- [Development setup](docs/DEVELOPMENT.md)
- [Contributing](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

## Development

Development is native on Fedora 44 (KDE Plasma, Wayland). Install the build
dependencies once (the command and what it installs are in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)), then:

```bash
cargo test --workspace
```

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Known limitations

- Only the core logic exists (sessions, configuration, security, providers and
  request orchestration). There is no popup, global shortcut, tray or UI yet.
- The operating system secret store backend is not written yet.
- On Wayland, corner placement, initial focus and outside-click dismissal depend
  on compositor support that has been confirmed to be *advertised* on KDE Plasma
  6.8 but not yet tested.
- Multi-monitor behaviour is untested.

## License

Not yet chosen. This is an open decision for the project owner and must be set
before the first public release.
