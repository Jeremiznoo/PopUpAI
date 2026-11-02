# Development

## Principle: nothing is installed on the host

Builds and tests run in a dedicated Exegol container named `dev` (Docker name
`exegol-dev`), created from the **already installed** `free` image. No image is
pulled or built. The host needs only Docker and the `exegol` CLI.

Other Exegol containers (`htb`, `oteria`, and so on) are never touched.

### Create the container (once)

```bash
exegol start dev free -w "$PWD" --no-gui --no-my-resources --no-exegol-resources \
  --network docker --offline -s bash --comment "PopUpAI dev: build and test only"
```

Run it from the project root. It opens a shell; `exit` leaves it. These options
are deliberately stricter than Exegol's defaults:

| Option | Why |
|---|---|
| `--offline` | The wrapper makes no internet request, so nothing is updated or pulled. |
| `--no-gui` | The host display (X11 or Wayland) is **not** shared. |
| `--no-my-resources`, `--no-exegol-resources` | `~/.exegol/...` is not mounted. |
| `--network docker` | Bridge network instead of the host's network stack. |
| `-w "$PWD"` | Only the project directory is shared, as `/workspace`. |

Exegol prints a generated root password for the container when it is created. It
protects only root inside the container; it is not needed by any script and is not
stored in this repository.

### Install the toolchain (once)

```bash
scripts/dev.sh setup
```

Downloads `rustup-init` from `static.rust-lang.org`, checks its SHA-256 against the
published file (this guards against corruption; authenticity comes from HTTPS),
then installs Rust **1.99.1** (minimal profile, `rustfmt`, `clippy`) to `/opt/rust`
inside the container. The version is pinned in
[rust-toolchain.toml](../rust-toolchain.toml). The script is idempotent.

### Daily commands

```bash
scripts/dev.sh cargo test --workspace
scripts/dev.sh cargo clippy --workspace --all-targets -- -D warnings
scripts/dev.sh cargo fmt --all --check
scripts/dev.sh shell        # interactive shell, as your uid
scripts/dev.sh clean        # empty the build cache in the container
```

Commands run as **your uid**, not root, so files written to the project stay
yours. `HOME`, `CARGO_HOME` and `CARGO_TARGET_DIR` live in `/var/tmp/popupai`
**inside the container**; build output never lands in the project tree. Deleting
the container deletes that cache.

## How the container is confined

Read from `docker inspect` after creation, not from Exegol's summary:

| Setting | Value |
|---|---|
| Privileged | no |
| Added or dropped capabilities | none |
| Network | `bridge` |
| Devices | none |
| Mounts | project at `/workspace` (rw), an Exegol helper script (ro), `/etc/localtime` (ro) |

The container has outbound network access, because the toolchain and `cargo`
download from `static.rust-lang.org` and crates.io.

## Verified on the development machine (2026-11-02)

- Container created from the installed image; configuration as in the table above.
- Rust 1.99.1, cargo, clippy and rustfmt run as uid 1000.
- Workspace: `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test` pass,
  131 tests in total, stable over repeated runs: `popupai-core` 54 unit and 1
  log-privacy test, `popupai-security` 20, `popupai-ai` 39 unit and 17 end-to-end.
- The end-to-end tests talk to a scripted HTTP server on the container's loopback
  interface. They check the headers actually sent (the key is only ever in a
  header, never in the URL or body), error mapping, refusal to follow redirects,
  timeouts, mid-stream failures, and that dropping a stream closes the connection.
- Two tests were checked to have teeth by breaking the code on purpose: with
  redirects enabled the redirect test fails, and with an answer deliberately
  written to the logs the log-privacy test fails.
- Logs are checked by capturing everything at TRACE level during a successful
  request, a failing one whose error text echoes the key, a stopped one and one
  that cannot start. The captured text must contain none of the prompts,
  answers, server error text or the key.
- No file in the project is owned by root.

### Not verified

- **No provider has been called for real.** Request and response formats were
  written from the providers' public documentation and tested with fixtures.
  Details that could differ on the live services include the OpenAI
  `max_completion_tokens` parameter, Gemini's `finishReason` handling and the
  model-list response shapes. Opt-in live tests (`#[ignore]`, keys from
  `.env.example`) are still to be written, and need real keys.
- **The OS secret store backend** is not written, and cannot be tested in this
  container, which has no Secret Service on a session bus.
- **Ollama** was installed on the host but not running when the machine was
  audited, so the Ollama adapter has only been tested against the scripted server.

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

## Known gap: the Qt shell cannot be built in this container yet

The `free` image is **Debian 12 (bookworm)**. It has `g++` 12.2, `cmake` 3.25,
`clang` 14 and only **Qt 5.15** (`qmake` is Qt 5). It has no Qt 6, no
`layer-shell-qt` and no Rust, and the target desktop runs Qt 6.11 with
LayerShellQt 6.8.

That is a problem only for the future `popupai-app` crate (QML window,
LayerShellQt). `popupai-core`, `popupai-ai` and `popupai-security` do not use Qt and
can be built and tested here. The apt package lists in the image are empty, so I
have **not** verified what the Debian repositories (including the `sid` source
that is configured) would offer. Options, none of which has been started:

1. **Fedora 44 container** matching the target desktop exactly (this is what I
   first built, about 2.3 GB, then deleted at your request). Needs one image
   build, which downloads a base image.
2. **Qt 6 and LayerShellQt inside `exegol-dev`** from Debian `sid`: possible but
   unverified, and mixing `sid` into a bookworm container is fragile.
3. **Defer the Qt shell** and continue with the Qt-free crates first.

## Running with a display: not done

The container was created with `--no-gui`, so it cannot draw on your screen.
Granting access later means recreating the container (Docker cannot add mounts to
an existing one), and it is a separate decision: first the Wayland socket only,
which is enough for spikes S1 to S3, never the session D-Bus without a filter.

### Known limits of container-based testing

- **Global shortcut portal (spike S4).** The portal identifies a caller by process
  and cgroup, and may map it to an installed `.desktop` file. A process inside a
  container will probably not be recognised the way an installed application is,
  so a bind result from a container is **not conclusive** about the packaged app.
  This is an assumption to confirm.
- **Tray, secret store, autostart** depend on the host session bus and have the
  same limit.
- **Multi-monitor** is limited by the single output on this machine.
- **Qt version mismatch** between any container and the host desktop.

## Testing strategy

**Automated (no desktop needed, runs in the container):** session state machine,
config parsing and validation, endpoint validation, log sanitizer, provider
adapters against a local mock server (streaming, cancellation, error mapping),
Markdown block model.

**Opt-in live tests:** real provider calls, `#[ignore]`d, keys from the
environment variables in `.env.example`.

**Manual desktop integration (cannot be automated reliably on Wayland):** spikes
S1 to S6 in [ADR 0002](ADR/0002-linux-desktop-integration.md), shortcut
conflicts, multi-monitor, X11 via XWayland. Results are recorded in
[PLATFORM_COMPATIBILITY.md](PLATFORM_COMPATIBILITY.md) and are never reported as
automated passes.

## Dev-machine side effects (to approve before they happen)

- Sharing the Wayland socket with a container (see above).
- A user-level launcher at `~/.local/share/applications/<app-id>.desktop`, which
  the shortcut portal may need to name the app.
- A global shortcut entry in KDE's shortcut settings, created through the portal
  (KDE asks for confirmation).

## Measuring performance

See the table in [ARCHITECTURE.md](ARCHITECTURE.md). Measurements will be taken
with the popup hidden and shown, repeated, and recorded here with the commit they
were taken at. A containerised run adds overhead that a packaged build does not,
so final numbers must come from a build running on the host.
