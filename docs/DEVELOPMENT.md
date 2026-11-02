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

## Running the app on the desktop

Running the app for real touches your session. Each of these is approved
beforehand, and none has happened yet:

- The first window shown on screen (spike S1: a small layer-shell window in the
  bottom-right corner).
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
