# Development

## Environment: the Fedora host

On 2026-11-02 the owner decided to develop natively on the host (Fedora 44, KDE
Plasma 6, Wayland). The reason is the work ahead: the popup, the global-shortcut
portal, the tray, the Secret Service, autostart and performance can only be
validated in the real desktop session. A container cannot reach the session bus,
and its measurements would not be representative. Development started in a
container; see [Legacy: the Exegol container](#legacy-the-exegol-container).

### Prerequisites

```bash
sudo dnf install --setopt=install_weak_deps=False rust cargo rustfmt clippy gcc-c++ cmake qt6-qtbase-devel qt6-qtdeclarative-devel layer-shell-qt-devel
```

The transaction is 35 packages, 142 MiB to download and 550 MiB on disk, with
nothing removed or replaced. All come from the official `fedora` and `updates`
repositories. It was installed on 2026-11-02.

| Packages | Reason |
|---|---|
| `rust`, `cargo`, `rustfmt`, `clippy` | Toolchain (1.99.1), formatting, lints. |
| `gcc-c++`, `cmake` | cxx-qt and the LayerShellQt shim compile C++; the TLS stack builds C code. |
| `qt6-qtbase-devel`, `qt6-qtdeclarative-devel` | Qt 6.11.2 headers, `qmake6`, QML tooling. |
| `layer-shell-qt-devel` | Headers for `LayerShellQt::Window`. |

Not installed on purpose: `openssl-devel` (TLS uses rustls), `libsecret-devel` and
`dbus-devel` (the Secret Service and D-Bus clients are pure Rust), GTK and WebKit.

Fedora's Rust is not managed by `rustup`, so [rust-toolchain.toml](../rust-toolchain.toml)
is informational here; the installed version (1.99.1) matches the pin. If `rustup`
is used later, it will install the pinned version.

### Commands

```bash
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Disk use after the first build: about 1.4 GB in `target/` (git-ignored) and about
190 MB in `~/.cargo`.

## Verified on the development machine (2026-11-02)

- Native toolchain: rustc and cargo 1.99.1, Qt 6.11.2, LayerShellQt 6.8.2, g++ 16.2.1,
  cmake 4.3.0. `rpm -V` reports no anomaly.
- Workspace, natively: `cargo fmt --check`, `cargo clippy -D warnings` and
  `cargo test` pass, 131 tests in total: `popupai-core` 54 unit and 1 log-privacy
  test, `popupai-security` 20, `popupai-ai` 39 unit and 17 end-to-end.
- A Qt Quick program using `LayerShellQt::Window` (overlay layer, bottom-right
  anchor, on-demand keyboard focus) compiles and links against the host libraries.
  It was **not run**.
- cxx-qt 0.10 builds against Qt 6.11.2 in about 48 seconds, and a minimal binary
  using `QString` runs (Qt Core only, no display).
- The end-to-end tests talk to a scripted HTTP server on the loopback interface.
  They check the headers actually sent (the key is only ever in a header, never in
  the URL or body), error mapping, refusal to follow redirects, timeouts,
  mid-stream failures, and that dropping a stream closes the connection.
- Two tests were checked to have teeth by breaking the code on purpose: with
  redirects enabled the redirect test fails, and with an answer deliberately
  written to the logs the log-privacy test fails.
- Logs are checked by capturing everything at TRACE level during a successful
  request, a failing one whose error text echoes the key, a stopped one and one
  that cannot start. The captured text must contain none of the prompts, answers,
  server error text or the key.

### Open items

- **Linker warning.** `cxx-qt-build` warns that none of `mold`, `lld` or `gold` was
  found and that GNU `ld.bfd` "will likely fail". The minimal build succeeded with
  `ld.bfd`, but a build with a real QML module may need `lld` or `mold`. Install
  `lld` if the first QML build fails to link.

- **Loss of focus: the popup always gets focus, but keeping it is not established.**
  The popup closes when it loses keyboard focus. Across the on-screen runs it became
  active after every show (12 of 12, in 4 to 9 ms, apart from the first show after
  start). Keeping the focus is less clear. In the second silent run of
  `tools/try-resident.sh` (nothing printed to the terminal), 3 of 6 shows ended by
  `escape`, a key the script never sends, so someone did press keys while it ran and
  the run was not hands-off. That makes the focus losses at 1.4 s and 3.3 s ambiguous
  (they may be the user's clicks). One case cannot be human: **show 2 became active and
  was inactive again 38 ms later.** A spike run had also lost focus 250 ms after showing.
  The cause of those two early losses is unknown; the terminal-output hypothesis was
  not confirmed (the loss happened with the terminal silent). An unattended run, with
  nobody touching the machine, is the decisive test. Until then no claim is made that
  the popup reliably keeps focus.
- **Cold first show.** The first show after start took 158 ms, then 114 ms, to become active, above
  the 150 ms target on one of the two; later shows took under 10 ms. Pre-creating the window
  surface at start-up is the obvious remedy, not done yet.

### Not verified

- **No provider has been called for real.** Request and response formats were
  written from the providers' public documentation and tested with fixtures.
  Details that could differ on the live services include the OpenAI
  `max_completion_tokens` parameter, Gemini's `finishReason` handling and the
  model-list response shapes. Opt-in live tests (`#[ignore]`, keys from
  `.env.example`) are still to be written, and need real keys.
- **The OS secret store backend** is not written yet. The host has the Secret
  Service (`ksecretd`), so it can now be tested natively.
- **Ollama** is installed on the host but was not running when the machine was
  audited, so the Ollama adapter has only been tested against the scripted server.
- **Nothing has been run on the desktop yet**: no window, shortcut or tray.

## Testing strategy

**Automated (no desktop needed):** session state machine, configuration,
credentials, request orchestration, endpoint validation, log sanitizer, provider
adapters against a local mock server (streaming, cancellation, error mapping),
Markdown block model.

**Opt-in live tests:** real provider calls, `#[ignore]`d, keys from the
environment variables in `.env.example`.

**Manual desktop integration (cannot be automated reliably on Wayland):** spikes
S1 to S6 in [ADR 0002](ADR/0002-linux-desktop-integration.md), shortcut
conflicts, multi-monitor, X11 via XWayland. Results are recorded in
[PLATFORM_COMPATIBILITY.md](PLATFORM_COMPATIBILITY.md) and are never reported as
automated passes. The dev machine has a single monitor.

### A logging-test pitfall

`tracing` caches process-wide which log points are enabled. A test that installs a
subscriber can therefore miss events when other tests run in the same process
without one: it passed alone and failed in parallel. The log-privacy test lives in
its own file (`crates/popupai-core/tests/logging.rs`), which Cargo runs as a
separate process, with a single test function. Keep it that way.

Tests that cancel a request must first wait for it to have started (for example
by waiting for its first event). Cancelling a task that has not been polled yet
means no request was ever created, so there is nothing to observe closing.

### A build-cache pitfall

Cargo decides whether to rebuild from file modification times. If a source file is
restored from a backup with an older timestamp (for example after a manual
experiment), cargo may reuse a stale binary. `touch` the file to force a rebuild.
This happened once during development and was caught by the full test run.

## Running the application

The application is resident. The first process keeps running; dismissing the popup
hides it, and a later `popupai --toggle` shows it again with a fresh session. A second
launch never starts a second PopUpAI: it hands its command to the running one over
D-Bus and exits. While it runs it holds the name `io.github.popupai.PopUpAI` on the
session bus (a provisional id), and releases it on exit.

```text
popupai              show the popup (starts PopUpAI if needed)
popupai --toggle     show it if hidden, hide it if shown
popupai --background start hidden (for autostart)
popupai --quit       stop the running instance
```

The global shortcut and tray icon come next; until then `--toggle` is how you bring
the popup back, and `--quit` (or Ctrl+C in its terminal) is how you stop it.

1. **A configuration with a provider.** There is no settings screen yet, so write
   one by hand at `~/.config/popupai/config.toml` (or point `POPUPAI_CONFIG` at a
   file, which is what the tests and examples do). The file never holds API keys:
   they are rejected. For example, with a local Ollama:

   ```toml
   default_provider = "ollama"

   [[provider]]
   id = "ollama"
   kind = "ollama"
   model = "llama3.2:latest"   # any model you have pulled
   ```

2. **Try the interface without a model.** `tools/fake_ollama.py` serves a canned,
   word-by-word answer on `127.0.0.1:11555` and never logs what you type. Use
   `endpoint = "http://127.0.0.1:11555"` and `model = "fake-model"`.

   Or run `tools/try-popup.sh`: it builds, starts the fake server on a free port
   with a temporary configuration, prints what to try, counts down five seconds so
   you are ready, runs the popup (quitting when it is dismissed, through
   `POPUPAI_EXIT_ON_DISMISS=1`), and stops everything it started when it exits.

   `tools/smoke-resident.sh` checks the resident behaviour headlessly, on a private
   D-Bus bus and the Qt `offscreen` platform, so it touches neither your session bus
   nor your screen.

   `tools/try-resident.sh` is the on-screen counterpart: it starts PopUpAI hidden, shows
   and hides the popup by itself several times over D-Bus, prints the latency of each
   appearance from the application's own timing logs (fixed labels only), then leaves
   the popup up for you to ask a question. It holds the D-Bus name on your session bus
   while it runs.

3. **Run it.**

   ```bash
   QT_FORCE_STDERR_LOGGING=1 cargo run -p popupai-app
   ```

   `QT_FORCE_STDERR_LOGGING=1` matters: inside a systemd user session Qt sends its
   own logs to the journal, so QML warnings are otherwise invisible. The
   application's own logs (provider, model, counts, error kind; never prompts,
   answers or keys) go to stderr at the level set in the configuration.

Things to know while developing:

- **Keys.** The system secret store is not implemented yet, so providers that need
  a key (OpenAI, Anthropic, Gemini) report that clearly. Keyless local providers
  such as Ollama work.
- **cxx-qt and `unsafe`.** The bridge expands to unsafe code and its module accepts
  no attributes, so `popupai-app` uses `deny(unsafe_code)` with the exception scoped
  to the single `controller` module. The other crates `forbid` it.
- **Build noise.** GCC 16 prints `-Wsfinae-incomplete` warnings from Qt's own
  headers during the C++ build; they are not from this project. `cxx-qt-build` also
  warns that no `lld`, `mold` or `gold` linker was found. The build links fine with
  `ld.bfd` so far.
- **Layer-shell in QML.** The window is configured with the `org.kde.layershell`
  QML module shipped by the `layer-shell-qt` runtime package, so no C++ shim is
  needed. `popupai-app` therefore needs that package at run time, and is Plasma
  specific until the X11 and other-compositor fallbacks of ADR 0002 are written.

## Running the app on the desktop

Running the app for real touches your session. Each of these is approved
beforehand. The spike windows and the real popup (against a fake Ollama server)
have been shown with approval. Still to come:

- A user-level launcher at `~/.local/share/applications/<app-id>.desktop`, which
  the shortcut portal may need to name the app.
- A global shortcut entry in KDE's shortcut settings, created through the portal
  (KDE asks for confirmation).
- A tray icon, and secret-store access (KWallet may ask to be unlocked).
- Autostart configuration.

Screenshots are never taken of the whole screen. If one is needed it is asked
first and limited to the popup's own window. Behaviour is otherwise observed
through the application's own timestamped event logs.

## Measuring performance

See the table in [ARCHITECTURE.md](ARCHITECTURE.md). Measurements will be taken
natively with the popup hidden and shown, repeated, and recorded here with the
commit they were taken at.

## Legacy: the Exegol container

Development started in a dedicated Exegol container (`exegol-dev`, created from the
installed `free` image) driven by [scripts/dev.sh](../scripts/dev.sh). It was kept
off the host while the toolchain question was open and is no longer needed.

- It is Debian 12. To get Qt 6 for the Qt shell, Qt 6.8.2 and LayerShellQt 6.3.4
  were installed from Debian 13 (trixie) **in place**, which upgraded `libc6` and
  Python and replaced 29 library packages. This is not a supported Debian
  configuration, so treat that container as disposable. A chroot was ruled out
  because the container has no `SYS_ADMIN` capability, so it cannot mount `/proc`
  or the project directory.
- It can still run the Qt-free crates in isolation:
  `scripts/dev.sh setup`, then `scripts/dev.sh cargo test --workspace`.
- To remove it: `docker rm -f exegol-dev`. The Docker volume `popupai-cache`
  from the first container attempt can go too: `docker volume rm popupai-cache`.
