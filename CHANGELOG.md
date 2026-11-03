# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added (since the first popup)

- Global shortcut through the XDG GlobalShortcuts portal (`popupai-platform`): registers
  the application id, binds one `toggle` shortcut with the preferred trigger converted to
  the portal format, turns `Activated` into a toggle, reports active / no key assigned /
  unavailable, and never falls back silently. Tested against a **mock portal** on a
  private bus; not yet run against KDE.
- Platform-neutral shortcut parsing and validation (`popupai-core::shortcut`).
- API keys in the system secret store through the Secret Service API (encrypted session),
  failing closed when no service runs. The round trip against the real service is an
  ignored test that must be run on purpose.
- Lightest model per provider by default (`claude-haiku-5-5`, `gpt-6-luna`,
  `gemini-3.5-flash-lite`); Ollama uses the smallest installed chat model. A missing
  configuration yields one entry per well-known provider.
- Provider list, switching (Ctrl+P, persisted unless the settings file was unreadable),
  automatic start on the first usable provider, and detection of a missing API key.
- Masked API key entry in the popup, handed straight to the secret store.
- Sanitised Markdown rendering of answers: a whitelist of tags with no attribute, no
  image, no link, raw HTML escaped.
- *Set the shortcut…* in the popup, which opens the desktop's shortcut settings through the
  portal when KDE leaves the requested key unassigned (observed on Plasma 6.8.2).
- Guided real-session run `tools/try-real.sh` and ignored live tests against a real local
  Ollama (`llama3.2:1b`), both run on 2026-11-02.
- Notification-area icon (StatusNotifierItem), autostart entry driven by
  `launch_at_startup`, desktop file and a per-user install script. Binary renamed `popupai`.

- Model picker (Ctrl+P) with search, recent and favourite models, remembered in `state.toml`;
  models come from each provider's default and from what a local Ollama has installed.
  The popup widens towards a square as the answer grows. Both are covered by unit tests of
  their logic and a headless load of the interface; **not yet looked at on screen**.

- Experimental *Sign in with ChatGPT* (device code flow, token renewal, Responses stream)
  for subscription use without an API key. Tested against a scripted server only; relies on an
  undocumented backend and may not be permitted by OpenAI's terms.

- Visual refresh: a rounder card with a soft shadow, a fade-and-settle opening, a focus ring,
  a send button that appears once there is text, and animated thinking dots. Checked by loading
  the interface headlessly; **not yet looked at on screen**.

- Settings page (Ctrl+,; Ctrl+A in the model picker): connection state of each provider, connect, disconnect
  (removes the stored credential) and the MCP servers. Ctrl+P goes round chat, models and settings; typing
  `/` lists the commands (`/help`, `/shortcuts`, `/models`, `/settings`). The rows and their wording are unit-tested; the page has not been looked at on
  screen.

- The model is told which system it runs on (distribution and version, kernel, architecture, desktop,
  shell, language, date) unless `advanced.share_system_info` is false. No identity or file data.

- The model can run commands: plain reads at once, everything else after the user's Enter; MCP servers
  already configured by other applications are detected (read-only), switched on one by one in
  Settings, and every tool call asks. Tested against scripted providers and a mock MCP server only.

- Visual identity (`branding/`): the logo redrawn as a clean SVG (the generated icon had stray pixels around
  the card), its PNG sizes, a monochrome and a symbol-only version, the README banner and the GitHub social
  preview, and the art direction guide `branding/BRAND.md`. The logo is the application icon (launcher,
  autostart entry, icon theme), the notification-area icon (embedded pixmaps), and is drawn in the popup,
  the model picker and the settings page, whose accents now use the brand colours.

- On-demand operation: PopUpAI no longer needs to stay running. `popupai --toggle` starts it and shows the
  popup, and it quits when the popup is closed and nothing is being answered (`stay_running = true` keeps the
  old resident mode with the tray and the portal shortcut). `popupai --setup-shortcut` binds the shortcut in KDE,
  GNOME or XFCE after asking, and prints a line for other desktops. The popup is placed for X11 and frameless
  on Wayland without layer-shell. Everything is keyboard only.

- MIT license, and the packaging for a release: Fedora spec, Debian packaging, an AUR `PKGBUILD` and a workflow that builds
  the `.rpm` and `.deb` in their own distribution when a version tag is pushed. **Nothing has been built or run yet**
  (see `docs/RELEASING.md`).

### Fixed (since the first popup)

- The model picker showed "undefined" everywhere: its model field was named `model`, which a QML delegate
  reads as its own model object. It is now `name`.
- The picker lists the models a connected hosted provider reports (chat models only) and what a local
  Ollama has installed, and any model name can be typed and used.
- The ChatGPT sign-in code and link can be selected and copied, with Copy and Open buttons.

- Google reports a wrong API key as HTTP 400, which was shown as a generic error; it is now an
  authentication error. Found by a live request with an invalid key to each hosted service.
- A refused API key could not be replaced from the popup (the message pointed at a settings
  window that does not exist). The key field now comes back after a refusal, and the messages
  describe what the user can actually do.

- Gemini reasoning parts (`thought: true`) are no longer mixed into the answer (from the
  vendor's documentation).

- Real KWallet round trip run and passed (2026-11-03).

### Changed (since the first popup)

- The popup is minimal: one field at the bottom, the answer growing above it, one dim
  line with the model and the actions that apply.
- The headless smoke test and the private test buses have no D-Bus service directories,
  so a test can never start the real desktop portal.
- Commit history rewritten once (local only, never pushed) to remove a tool attribution
  trailer from every message; the repository has a single author.

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
  is untrusted. It was shown on screen against a fake Ollama server: a question was
  sent from the keyboard, the answer streamed to completion and the popup closed on
  loss of focus, as recorded by its own logs. The dismissal log carries a reason.
- Spike S1 to S3 (`spikes/s1-layer-shell`) and `tools/fake_ollama.py` to try the
  interface without a model.
- `popupai-platform`: single instance and remote commands over D-Bus. The application
  is now resident: `popupai --toggle`, `--background` and `--quit`, and a second launch
  hands its command to the running instance. The interface has four argument-less
  methods only, checked by a test that introspects it. Tests run on a private bus.

### Fixed

- A second launch no longer steals the D-Bus name from the running instance (zbus
  requests names with AllowReplacement and ReplaceExisting by default).
- Keyless local providers such as Ollama now work when the system secret store is
  unavailable; providers that need a key still fail with a clear explanation.

### Changed

- Development moved from a container to the Fedora host, because the desktop
  integration (popup, shortcut portal, tray, Secret Service, performance) can only
  be validated in the real session. The build dependencies are installed with the
  command in `docs/DEVELOPMENT.md`; the container is kept as an optional legacy
  workflow. All 131 tests pass natively, and a Qt Quick plus LayerShellQt program
  and a minimal cxx-qt crate build against Qt 6.11.2.
