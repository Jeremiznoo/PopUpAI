<p align="center"><img src="branding/banner.png" alt="PopUpAI: ask anything, anywhere on my desktop" width="100%"></p>

# PopUpAI

Ask an AI a quick question without leaving what you are doing. Press a global shortcut, a small card
pops up in the corner of your screen with the field already focused, type, read the streamed answer,
click elsewhere and it is gone. Each opening is a fresh, unsaved session.

- **Fast and light:** one field, the answer above it, the model on a dim line below.
- **Your account or your key:** sign in with ChatGPT, use an API key, or point it at a local model.
- **Keyboard first:** `Ctrl+P` goes round chat, models and settings; `/` lists the commands.
- **Private by default:** no telemetry, nothing logged but metadata, keys only in the system wallet.
- **Native:** Qt/QML with a Rust core, built for KDE Plasma on Wayland.

## Install

```bash
tools/install-local.sh            # binary in ~/.local/bin, launcher and icon for your user
tools/install-local.sh --remove
popupai --background              # start it; use --toggle to show or hide the popup
```

The global shortcut needs the installed launcher (the desktop portal refuses an application it cannot
identify). Default key: `Ctrl+Shift+Space`. On KDE the key is assigned by you: if the popup says it has
none, click *Set the shortcut…*. Without a global shortcut, bind `popupai --toggle` in your system
keyboard settings. To start with your session, set `launch_at_startup = true` under `[general]`.

## Use

| Keys | |
|---|---|
| `Enter` / `Shift+Enter` | Send / new line |
| `Ctrl+P` | Next view: chat → models → settings → chat |
| `/` | Commands: `/help`, `/shortcuts`, `/models`, `/settings` |
| `Ctrl+F` (in models) | Favourite |
| `Ctrl+.` / `Ctrl+R` | Stop / bring back an answer that finished in the background |
| `Esc` | Close |

**Models** (`Ctrl+P`): search, recent and favourite models first, and any model name can be typed.
**Settings** (`Ctrl+P` twice, or `Ctrl+,`): each provider's connection (connect with `Enter`,
disconnect with `Del`) and the MCP servers found on your computer.

## Providers

| Provider | Access | Default model |
|---|---|---|
| ChatGPT | Sign-in with your subscription (experimental, see below) | `gpt-6-luna` |
| Anthropic | API key | `claude-haiku-5-5` |
| OpenAI | API key | `gpt-6-luna` |
| Google Gemini | API key | `gemini-3.5-flash-lite` |
| Ollama and any OpenAI-compatible server | Your own local model, chosen by you | none |

Each default is the lightest model its vendor offers. Override any of them with `model = "…"` on the
provider in `~/.config/popupai/config.toml` (created with mode 0600).

Keys are asked for in the popup when needed and go straight to the system secret store (KDE Wallet,
GNOME Keyring, KeePassXC); they are never written to a file.

**ChatGPT sign-in** uses the device-code flow of OpenAI's Codex CLI and an undocumented backend that
OpenAI may change or restrict: check their terms before relying on it. Claude subscriptions cannot be
used this way.

## Commands and MCP

With a provider that supports tools (ChatGPT for now) the model can run commands and call tools of the
MCP servers you switch on.

- Plain read-only commands (`ls`, `cat`, `find` without `-exec`, `uname`…) run at once and are shown in
  the answer. Anything else is shown in full and waits for `Enter`; `Esc` refuses.
- MCP servers already configured by Claude Desktop, Claude Code, Cursor, VS Code or opencode are listed in
  Settings, all off. One is started only when you switch it on, and every call asks first.
- The model is told your OS, kernel, desktop, shell, language and date, never your name, host or files.

Turn these off with `allow_commands = false` and `share_system_info = false` under `[advanced]`.

## Privacy

Questions and answers are never logged, saved or copied on their own; the logs hold metadata only, and a
test fails if a question, answer or key ever shows up there. ChatGPT requests ask the service not to
store the conversation.

## Documentation

[Architecture](docs/ARCHITECTURE.md) · [Platform compatibility](docs/PLATFORM_COMPATIBILITY.md) ·
[Security](SECURITY.md) · [Development](docs/DEVELOPMENT.md) · [Brand](branding/BRAND.md) ·
[Contributing](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

## Development

```bash
cargo test --workspace
tools/smoke-resident.sh     # headless check of the application on a private bus
```

Native on Fedora 44 with KDE Plasma (Wayland); the dependencies are listed in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## License

Not chosen yet.
