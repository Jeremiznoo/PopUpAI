<p align="center"><img src="branding/banner.png" alt="PopUpAI: ask anything, anywhere on my desktop" width="100%"></p>

# PopUpAI

> 🔒 **Private repository for now.** No license has been chosen yet; nothing here is published.

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
`popupai --toggle` in *System Settings, Shortcuts*. **Settings** (Ctrl+, or the gear under the field; Ctrl+A from the model picker, as in opencode):
the list of providers with their state (signed in, API key stored, not connected, local), where
Enter connects (the field then asks for the key or starts the sign-in) and Delete removes the stored
credential, plus *Start with the session* and *Global shortcut*. Everything else lives in
`~/.config/popupai/config.toml` (created with mode 0600).

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
| Ollama (optional) | none: you choose your own local model | listed from your own Ollama server; nothing is installed or downloaded for you |

Override any of them with `model = "…"` on the provider in the configuration file.

**First use.** With no key stored, PopUpAI starts on Ollama if you have it. To use a
hosted provider, pick one of its models (Ctrl+P): the field turns into a masked key field, and
the key you paste goes straight to the system secret store (KDE Wallet, GNOME Keyring or
KeePassXC) and is never written to a file. Local models are optional and never
downloaded for you: if you use Ollama, install the model you want and pick it in the model picker
(Ctrl+P) or with `model = "..."` in the configuration.

**Sign in with ChatGPT (experimental).** Instead of an API key you can use a ChatGPT
subscription: pick *ChatGPT* in the model picker (Ctrl+P), press Enter, and PopUpAI shows a
code and opens `auth.openai.com/codex/device`; sign in there and confirm. You may first have
to enable device-code sign-in in your ChatGPT security settings. The password is never seen
by PopUpAI; the tokens go to the system wallet and are renewed automatically. **This uses
the flow of OpenAI's Codex CLI and an undocumented backend, which OpenAI can change or
withdraw, and its use by third-party applications may not be allowed by OpenAI's terms: check
them before relying on it. It has only been tested against a scripted local server, never
against OpenAI: your first sign-in is its first real test.** Anthropic restricts the use of
Claude subscriptions by third-party applications, so there is no equivalent for Claude.

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

## Commands and MCP servers

With a provider that supports tools (the ChatGPT account, for now) the model can run commands on
your computer, and call the tools of MCP servers you switched on.

- **Plain reads run at once:** one program from a short list (`ls`, `cat`, `find` without `-exec`,
  `uname`, `df`, ...), no pipe, redirection, substitution or chaining, and nothing that names keys,
  credentials, browser data or this application's own files. The command is shown in the answer.
- **Everything else asks:** the popup shows the full command and why it asks; **Enter** runs it, **Esc**
  refuses, and no answer within two minutes refuses. A refusal is told to the model.
- **Limits:** 30 seconds and 16 KiB of output per command, and at most 8 steps per answer.
- **MCP:** the servers already configured by Claude Desktop, Claude Code, Cursor, VS Code and opencode
  are found (read-only) and listed in Settings, all **off**. A server is a program: it is started only
  after you switch it on there, and every call of its tools asks for your approval. Environment values
  and credentials in their arguments are never shown, logged or sent to the model.
- Turn commands off with `allow_commands = false` under `[advanced]`.

**Not verified:** this was tested against scripted providers and a small mock MCP server only. The real
ChatGPT backend's tool calling and a real MCP server (such as the one found on your machine) have not
been run through it.

## The model knows your system

With each request PopUpAI tells the model which system it is on, so its commands fit: the
distribution and version (from `/etc/os-release`), the kernel, the architecture, the desktop and
session type, the shell, the language and the date. It never sends your user name, host name, home
folder, network details or anything from your files. Turn it off with `share_system_info = false`
under `[advanced]`. The model cannot run commands here: it is told to give you commands to run.

## Your questions are not logged

Questions and answers are never written to the logs, to a file or to the clipboard on their own.
The logs hold metadata only (which provider, how many bytes, when the popup opened and closed), and
a regression test fails if a question, an answer, a server message or a key ever appears in them.
The popup keeps nothing between openings. Requests to a ChatGPT account ask the service not to store
the conversation (`store: false`); what the service does beyond that is its own policy.

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
- [Brand and art direction](branding/BRAND.md)
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
