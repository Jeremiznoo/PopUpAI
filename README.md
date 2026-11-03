# PopUpAI

An AI quick-ask launcher. Press a global shortcut, a small popup appears in the
corner of your screen with the input already focused, type a question, read the
streamed answer, click anywhere else and it disappears. Each opening is a fresh,
unsaved session.

> **Status: pre-alpha.** The application builds and its logic is covered by 230
> automated tests (plus one that is ignored on purpose, see below), but it has **not been run end to end on a real session with a real
> provider yet**. [What is verified, and what is not](docs/PLATFORM_COMPATIBILITY.md) is
> written down feature by feature; nothing below is claimed beyond that.

## Goals

Fast, lightweight, private by default, provider-agnostic, native on the desktop.
No telemetry, no forced account, no hidden conversation history.

## Target platforms

| Platform | State |
|---|---|
| Fedora Linux + KDE Plasma 6 (Wayland) | Primary target. The popup was shown and used on a real Plasma session during the spikes; the shortcut, tray and wallet code is tested against mocks and still has to be run on the real session. |
| KDE on X11 | Planned (via Qt `xcb`). |
| Windows 10/11, macOS | Architecture allows it. Not started. |

Details and the honest verification state: [docs/PLATFORM_COMPATIBILITY.md](docs/PLATFORM_COMPATIBILITY.md).

## Using it

Start `popupai` once (autostart is a setting, see below). Press **Ctrl+Shift+Space**
(the default), type, press **Enter**. **Esc**, a click elsewhere, or the shortcut again
closes it; the next opening is a fresh session. **Ctrl+P** (or a click on the dim line
under the field) opens the model picker: type to search, **Up/Down** and **Enter** to choose,
**Ctrl+F** to mark a favourite; recent and favourite models come first. The choice is
remembered in `state.toml` next to the configuration. **Stop** or **Ctrl+.** cancels an
answer, **Copy** copies it. The popup starts narrow and widens towards a square as the
answer gets longer.

The global shortcut needs the launcher file installed (see *Install for your user* below):
the desktop portal refuses an application it cannot identify. On KDE the key is then
assigned by you: if the popup says *no key assigned*, click *Set the shortcut…* and set it
in the window KDE opens. Without a working global shortcut (the popup says so), bind the command
`popupai --toggle` in *System Settings, Shortcuts*. Settings live in
`~/.config/popupai/config.toml` (created with mode 0600); there is no settings window yet.

The popup is deliberately bare: one field, the answer above it, one dim line.

## Providers and default models

OpenAI, Anthropic, Google Gemini, Ollama (local), and any OpenAI-compatible endpoint
(LM Studio, LocalAI, vLLM, OpenRouter, and so on). Model identifiers are free text.
Out of the box each provider uses the **lightest** model its vendor offers, since the
popup is for quick questions:

| Provider | Default model | Where the name comes from |
|---|---|---|
| Anthropic | `claude-haiku-5-5` | the vendor's model overview, 2026-11-02 |
| OpenAI | `gpt-6-luna` | the vendor's model page, 2026-11-02 |
| Google Gemini | `gemini-3.5-flash-lite` | the vendor's model page, 2026-11-02 |
| Ollama | the smallest chat model installed | asked from your Ollama server at request time |

Override any of them with `model = "…"` on the provider in the configuration file.

**First use.** With no key stored, PopUpAI starts on Ollama if you have it. To use a
hosted provider, pick one of its models (Ctrl+P): the field turns into a masked key field, and
the key you paste goes straight to the system secret store (KDE Wallet, GNOME Keyring or
KeePassXC) and is never written to a file. Local models are never downloaded for you:
install one yourself, for example `ollama pull llama3.2:1b`.

**What is verified, and what is not.** Against a scripted local server: all four adapters,
streaming, cancellation and error mapping. Against the **real** services on 2026-11-02:
Ollama answers end to end (local `llama3.2:1b`); and Anthropic, OpenAI and Google each
received one request with a deliberately **invalid** key and answered with an
authentication error, which proved the endpoints and headers and found (and fixed) that
Google reports a wrong key as HTTP 400. The streamed event formats of Anthropic and Google were
also compared with the vendors' own documentation, which led to ignoring Google's `thought`
parts. **Not verified:** a request with a *valid* key and the real streamed answers of the
hosted services, while the wallet round trip against KWallet passed on 2026-11-03 (they need your own key). The model names above come from vendor
documentation. The first real use with your key is also the first real test: please report
what you see.

If a stored key is refused, the popup shows the key field again so you can paste a new one.

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

## Install for your user

```bash
tools/install-local.sh            # binary in ~/.local/bin, launcher in ~/.local/share/applications
tools/install-local.sh --remove
```

Set `launch_at_startup = true` under `[general]` in the configuration file to start with
your session: that writes one entry in `~/.config/autostart`, and nothing else.

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
