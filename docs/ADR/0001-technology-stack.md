# ADR 0001: Technology stack

- **Status:** Proposed, 2026-11-02. Becomes Accepted when spikes S1 to S3 in
  [ADR 0002](0002-linux-desktop-integration.md) pass on the target machine.
- **Deciders:** project owner, with the architect's recommendation below.

## Context

PopUpAI is a global-shortcut popup. Its hard problems are not AI calls, they are
desktop behaviours: appearing in a screen corner, taking keyboard focus,
dismissing on outside click, and registering a global shortcut, all on
**KDE Plasma 6 / Wayland**, where clients cannot position their own windows or
grab keys. The owner's priority order is:

1. Reliable KDE Wayland integration
2. Low memory use
3. Fast startup
4. Fast popup appearance
5. Cross-platform maintainability
6. Native look
7. Security

Facts established on the target machine (see
[ENVIRONMENT_REPORT.md](../ENVIRONMENT_REPORT.md)): KWin 6.8.2 advertises
`zwlr_layer_shell_v1` v5; the GlobalShortcuts portal v2 is served by the KDE
backend; Qt 6.11.2 and `layer-shell-qt` 6.8.2 runtimes are already installed
(Plasma depends on them).

## Options considered

| | Tauri 2 + Rust | **Qt 6 / QML + Rust (chosen)** | GTK4 + Rust | Slint / iced / egui | Electron |
|---|---|---|---|---|---|
| 1. KDE Wayland integration | Weak path: window is created by `tao` / GTK3; using `gtk-layer-shell` means initialising it on a window Tauri owns, which its public API does not expose **(assumption, not tested)** | Strong: `LayerShellQt` is KDE's own API, the one Plasma uses | Good (`gtk4-layer-shell`) | Layer-shell support is community-maintained or absent **(assumption)** | Poor on Wayland |
| 2. Memory | WebKitGTK adds separate web and network processes **(assumption, unmeasured)** | Qt libraries already resident for Plasma; Qt Quick only | Moderate | Low | High |
| 3/4. Startup, popup speed | Good once warm | Good once warm | Good | Good | Slow |
| 5. Cross-platform | Best out of the box | Good (Qt is first-class on Windows and macOS; needs Qt bundling) | Weak on Windows/macOS | Good | Good |
| 6. Native look on Plasma | Web UI imitating native | Follows the Plasma palette and fonts natively | Looks foreign on Plasma | Custom-drawn | Foreign |
| 7. Security surface | Large web engine plus IPC bridge | Small: no web engine, no JS | Small | Small | Large |
| Rich text (Markdown, code blocks, selection, IME) | Easiest | Needs a custom block renderer | Moderate | Weak **(assumption)** | Easiest |

The ratings are engineering judgement, not measurements. Two cells are the
reason this ADR is "Proposed" and not final: the Tauri layer-shell claim and the
memory claim were not tested, because the toolchain is not yet installed.

## Decision

- **UI and desktop shell:** Qt 6 with QML (Qt Quick). One frameless, transparent
  window is created at start-up and kept hidden, so showing it is a visibility
  change and not a window creation.
- **Application logic:** Rust, in a Cargo workspace. The Qt/Rust bridge is
  **cxx-qt 0.10.0** (KDAB, MIT or Apache-2.0).
- **Wayland placement:** `LayerShellQt` through a small C++ shim.
- **Async:** `tokio`. All network and D-Bus work runs off the UI thread.
- **Markdown:** parsed in Rust with `pulldown-cmark` into a block model rendered
  by QML delegates. This gives per-code-block copy buttons and lets us refuse
  remote images (see Security below).
- **Crate versions** checked against the crates.io API on 2026-11-02:
  cxx-qt 0.10.0, ashpd 0.13.13, zbus 6.2.1, ksni 0.3.7, keyring 4.2.0 (with
  keyring-core 1.0.1), reqwest 0.13.5, tokio 1.54.1, pulldown-cmark 0.13.6,
  rustls 0.23.46. They are pinned by `Cargo.lock` once the workspace exists.

## Consequences

**Good**

- The riskiest requirement (corner placement and focus on Wayland) uses the API
  KDE itself maintains.
- No browser engine to patch, and no JavaScript in the process that holds
  prompts.
- The application core (`popupai-core`, `popupai-ai`, `popupai-security`) has no
  Qt dependency. If Qt proves wrong for Windows or macOS, a different shell can
  reuse it. This is the deliberate escape hatch.

**Costs we accept**

- cxx-qt has a learning curve, and its API has changed between releases
  (0.6 to 0.10), so versions are pinned and upgrades are deliberate.
- A custom Markdown renderer must be written and tested, including during
  streaming.
- Windows and macOS packaging must bundle Qt (LGPL obligations apply).
- A small C++ shim is unavoidable for `LayerShellQt`.

## Rejected alternatives

- **Tauri 2:** best cross-platform and fastest UI development, but it ranks
  lowest on priority 1 on the primary target. Revisit if Qt fails on Windows or
  macOS: its web UI could sit on the same Rust core.
- **GTK4 + Rust:** solid Wayland story but visually foreign on Plasma, and
  integrating with the Plasma theme is extra work for no gain.
- **Slint / iced / egui:** weak fit for rich, selectable, accessible text.
- **Electron:** wrong on memory and startup.
- **Qt in C++ only (no Rust):** viable, but Rust gives memory safety in the
  code that handles API keys and untrusted network input.

## Revisit triggers

Reopen this ADR if spike S1, S2 or S3 fails on KWin, if cxx-qt cannot express the
bridge cleanly, or if measured idle memory is materially worse than the
provisional budget in [ARCHITECTURE.md](../ARCHITECTURE.md).
