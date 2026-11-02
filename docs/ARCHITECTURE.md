# Architecture

Status: 2026-11-02. The Qt-free crates are implemented (table below); the desktop
and UI layers are not started. Decisions are
recorded in [docs/ADR/](ADR/): [0001 stack](ADR/0001-technology-stack.md),
[0002 Linux integration](ADR/0002-linux-desktop-integration.md),
[0003 session policy](ADR/0003-session-and-cancellation-policy.md).

## Implementation status

| Component | State | Verification |
|---|---|---|
| `popupai-core`: session lifecycle (ADR 0003) | Implemented | 15 unit tests |
| `popupai-security`: `Secret`, `Endpoint` validation, log redaction, `SecretStore` trait | Implemented | 20 unit tests |
| `popupai-security`: OS secret store backend (Secret Service / KWallet) | **Not implemented**; only the fail-closed `UnavailableStore` exists | Cannot be exercised from the build container, which has no session bus |
| `popupai-ai`: OpenAI, OpenAI-compatible, Anthropic, Gemini, Ollama adapters, streaming, cancellation, error taxonomy | Implemented | 39 unit tests and 17 end-to-end tests against a scripted local server |
| `popupai-ai` against the live services | **Not verified** | Wire formats come from public documentation and offline fixtures; no real request has been made |
| `popupai-core`: configuration, request orchestration | Not started | |
| `popupai-platform`, `popupai-app`, QML UI | Not started | Needs a Qt 6 build environment and a display decision, see [DEVELOPMENT.md](DEVELOPMENT.md) |

## Component diagram

```mermaid
flowchart TB
  user([User]) -- "CTRL+SHIFT+SPACE" --> portal["XDG portal GlobalShortcuts<br/>(KWin / kglobalaccel)"]
  cli["popupai --toggle"] -- "D-Bus Toggle" --> app

  subgraph app["popupai-app (binary)"]
    qml["UI layer: QML popup and settings"]
    bridge["cxx-qt bridge (QObjects)"]
    place["Window placement<br/>LayerShellQt shim / X11"]
  end

  subgraph platform["popupai-platform (no Qt)"]
    sc["GlobalShortcut"]
    tray["Tray (StatusNotifierItem)"]
    auto["Autostart"]
  end

  subgraph core["popupai-core (no UI, no OS)"]
    state["App state and config"]
    sess["Session lifecycle"]
    orch["Request orchestration"]
  end

  subgraph ai["popupai-ai"]
    iface["Provider trait"]
    oa["OpenAI and compatible"]
    an["Anthropic"]
    ge["Gemini"]
    ol["Ollama"]
  end

  subgraph sec["popupai-security"]
    store["SecretStore"]
    san["Log sanitizer"]
    net["Endpoint validation"]
  end

  portal --> sc --> bridge
  qml <--> bridge --> place
  bridge --> orch --> sess
  orch --> state
  orch --> iface
  iface --> oa & an & ge & ol
  oa & an & ge & ol -- "HTTPS (rustls)" --> ext(["Provider APIs / local Ollama"])
  oa & an & ge & ol --> net
  state --> store --> keyring[("Secret Service / KWallet")]
  tray --> bridge
```

## Layers

| Layer | Crate or location | Responsibility | Depends on Qt? |
|---|---|---|---|
| UI | `popupai-app/qml/` | Input, streaming answer, loading state, provider selector, settings | Yes |
| Desktop (Qt-bound) | `popupai-app/src/window/` | Popup lifecycle, layer-shell or X11 placement, focus and dismissal | Yes |
| Desktop (Qt-free) | `popupai-platform` | Global shortcut, tray, autostart behind traits | No |
| Application core | `popupai-core` | State, session lifecycle, config schema, orchestration | No |
| AI | `popupai-ai` | `Provider` trait, adapters, streaming, cancellation, error taxonomy | No |
| Security | `popupai-security` | `SecretStore`, log sanitizer, endpoint and redirect policy | No |

Platform traits (Linux implements first; Windows and macOS later):
`GlobalShortcut`, `Tray`, `Autostart`, `PopupPlacement`, `SecretStore`. Outside-click
dismissal and focus handling live behind `PopupPlacement`. **No Linux-specific
logic is allowed in core, ai or security.**

## Project structure

```
PopUpAI/
├── Cargo.toml                  # workspace
├── crates/
│   ├── popupai-core/           # state, sessions, config, orchestration
│   ├── popupai-ai/             # Provider trait + openai, anthropic, gemini, ollama
│   ├── popupai-security/       # SecretStore, log sanitizer, endpoint validation
│   ├── popupai-platform/       # traits + linux/ (ashpd, ksni, autostart)
│   └── popupai-app/            # binary: cxx-qt bridge, window/, qml/, --toggle
├── packaging/                  # desktop file, metainfo, icons, rpm spec, flatpak
├── docs/                       # this folder
├── .env.example                # placeholders for opt-in live tests only
└── README.md  SECURITY.md  CONTRIBUTING.md  CHANGELOG.md
```

Five crates, each with a reason to exist: the first three are UI-agnostic and
unit-testable without a desktop; `platform` isolates D-Bus and portals; `app` is
the only crate that links Qt.

## Quick Ask data flow

1. The portal emits `Activated` (or `--toggle` arrives over D-Bus).
2. `app` asks core for a fresh session, then shows the pre-created window with
   the input focused (ADR 0002, ADR 0003).
3. On Enter, core builds a request from the session and the selected provider
   configuration, and fetches the key from `SecretStore` just in time.
4. The adapter streams chunks. Each chunk becomes an event to the UI thread.
   Network and D-Bus work never run on the UI thread.
5. The Markdown block model updates incrementally; the view auto-scrolls.
6. Stop aborts the HTTP request. Dismissal follows ADR 0003.

## Provider contract

Common interface: send messages, stream tokens, cancel, list models (where
supported), and a typed error taxonomy (`Auth`, `RateLimited`, `Network`,
`Timeout`, `InvalidEndpoint`, `Provider`). Adapters are isolated; one failing
provider cannot affect another. OpenAI-compatible endpoints reuse the OpenAI
adapter with a custom base URL. Model identifiers are free text, with discovery
as a convenience (OpenAI-compatible `/models`, Ollama `/api/tags`).

## Security model

**Assets:** API keys, prompts, responses, provider configuration.

| Threat | Control |
|---|---|
| Key stolen from disk, git or backups | Keys only in the OS secret store (Secret Service / KWallet; Credential Manager; Keychain). No plaintext fallback: if the store is unavailable the app says so and refuses to save a key. |
| Key leaked to the wrong host | One HTTP client per provider bound to its origin; redirects not followed for authenticated requests; no credentials in URLs (Gemini uses a header); no cross-provider reuse. |
| Insecure transport | HTTPS required. Plain HTTP only for loopback, or with an explicit per-provider opt-in and a visible warning. URLs containing user-info are rejected. rustls, no "accept invalid certs" option. |
| Secrets or content in logs | Structured `tracing` logs; secrets wrapped in a redacting type; prompts and answers never logged at default levels. |
| Exfiltration through untrusted model output | Markdown is rendered by our own block renderer: no HTML, no automatic remote image loading, links shown with their real target and only opened on explicit click for `http(s)` and `mailto` schemes. |
| Hidden data collection | No telemetry, analytics, clipboard reads or screen capture. Copy buttons only write. |
| Abuse of the local IPC | D-Bus interface limited to `Toggle`, `Show`, `Hide`. |
| Supply chain | Minimal dependencies, committed `Cargo.lock`, `cargo audit` and `cargo deny` in CI, secret scanning (see [SECURITY.md](../SECURITY.md)). |

Configuration lives in `$XDG_CONFIG_HOME/popupai/config.toml`, written atomically
with mode `0600`, and **contains no secrets**.

## Performance budget

The 150 ms activation target applies to local latency only; provider inference
time is reported separately.

| Metric | Target | Status | How it will be measured |
|---|---|---|---|
| Popup activation, warm | < 150 ms from shortcut event to first frame with focused input | Unmeasured | Monotonic timestamps at shortcut received, `setVisible(true)`, first frame, active, input focus |
| Idle CPU | Close to 0 % | Unmeasured | `/proc/<pid>/stat` over 60 s with the popup hidden |
| Idle memory | Provisional budget < 100 MB PSS, to be revised after the first baseline | Unmeasured | `/proc/<pid>/smaps_rollup` |
| Cold start to ready | Provisional < 1 s | Unmeasured | Process start to shortcut registered |
| Repeated discovery requests | None while idle | By design | Model discovery only on explicit user action or settings open, cached |

Targets marked provisional are not claims; they are budgets to check.

## Platform abstraction summary

See [PLATFORM_COMPATIBILITY.md](PLATFORM_COMPATIBILITY.md) for what is verified
and what is only planned.
