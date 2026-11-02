# ADR 0002: Linux desktop integration (Wayland first, KDE Plasma)

- **Status:** Proposed, 2026-11-02. Every mechanism below is **unproven** until
  its spike passes; results go into
  [PLATFORM_COMPATIBILITY.md](../PLATFORM_COMPATIBILITY.md). Progress on
  2026-11-02: S2 passed (an on-demand layer surface gets keyboard focus when shown,
  with no click); S3 passed (an outside click deactivates the window and still
  reaches the window underneath, on the user's confirmation); S1 passed (the user
  confirmed by eye that the popup sits in the bottom-right corner and is readable);
  S4 to S6 are not started.
- **Depends on:** [ADR 0001](0001-technology-stack.md)

## Constraints (Wayland)

A Wayland client cannot choose its window position, cannot reliably stay on top
as a normal toplevel, cannot grab global keys, and cannot observe clicks that
land on other clients. Each requirement therefore needs a supported mechanism,
and none may be replaced silently by a weaker one.

## Decisions

### 1. Placement: `wlr-layer-shell` through LayerShellQt

The popup is a layer surface: layer `Overlay`, anchored `Bottom | Right`, with a
small margin, exclusive zone `0` (so it sits above the Plasma panel and not
under it), keyboard interactivity `OnDemand`, scope `popupai`. KWin 6.8.2
advertises layer-shell v5 (verified), and v4 or later is required for
`OnDemand`.

The output is chosen by the compositor first. Multi-monitor and scaling
behaviour is a manual test because the dev machine has one output.

### 2. Dismissal: focus loss, never input monitoring

Outside-click dismissal is implemented as "the popup lost keyboard focus", via
`QWindow::activeChanged(false)`. When another window is clicked, the compositor
moves focus and the layer surface is deactivated. This needs no global input
capture, no polling and no extra privileges.

Guard rules: ignore deactivation until the popup has been active once after
being shown, and treat a focus loss caused by our own file or menu dialogs as
not-a-dismissal.

*Known unknown:* whether the click that causes the focus loss still reaches the
target window (it should, because the compositor delivers it normally). This is
spike S3.

### 3. Initial keyboard focus

The requirement is "type immediately, no click". With `OnDemand`, the protocol
does not promise focus on map and compositors differ **(from upstream protocol
discussion; KWin's behaviour is not documented in what I could find, so it must
be tested)**. Escalation order if S2 fails, stopping at the first that works:

1. Request activation with an `xdg_activation_v1` token if the portal supplies
   one with the shortcut event (the protocol is advertised by KWin).
2. Re-assert focus after map from the compositor-delivered activation.
3. `Exclusive` keyboard interactivity **only if** a click-through-preserving
   dismissal can be kept. Exclusive plus a full-screen click-catcher swallows the
   click and is a UX regression, so it is documented and not the default.

Anything that deviates from the above is written up here before it ships.

### 4. Global shortcut: XDG Desktop Portal GlobalShortcuts (via `ashpd`)

`CreateSession`, `BindShortcuts` with id `toggle` and preferred trigger
`CTRL+SHIFT+space`, then listen for `Activated`. The portal is the supported,
cross-desktop mechanism and v2 is present here (verified). KWin and `kglobalaccel`
sit underneath it.

Things the spike must establish:

- Whether an unsandboxed binary needs an installed `.desktop` file whose name
  matches the portal app id, and how that id is determined. I found two KDE bugs
  on duplicate or misnamed shortcut entries
  ([492992](https://bugs.kde.org/show_bug.cgi?id=492992),
  [474294](https://bugs.kde.org/show_bug.cgi?id=474294)); both are fixed in
  Plasma 6.3.0 and this machine runs 6.8.2. What an unsandboxed app must do is
  **not documented in those reports**.
- That the binding persists across restarts without creating duplicates.
- What the portal reports on a conflict, and whether the user's final trigger
  differs from the preferred one.

*No silent fallback.* If registration fails the app says so (tray notice and
popup banner) and points to the supported manual route below.

### 5. Manual route: `popupai --toggle`

A second invocation sends a D-Bus call to the running instance (single instance
via the well-known name `io.github.popupai.PopUpAI`, provisional id). Any user
can bind that command in *System Settings, Keyboard, Shortcuts, Add Command*.
This works on every desktop and is the documented fallback, not an automatic
one. The D-Bus interface exposes `Toggle`, `Show`, `Hide` and `Quit` only. It never
accepts prompt text: a test introspects the exported interface and asserts that no
method takes an argument and that there are no properties or signals.

### 6. System tray: StatusNotifierItem via `ksni`

Pure-Rust over D-Bus, so there is no GTK or Qt Widgets dependency. A StatusNotifier
host is present here (verified). Behind a `Tray` trait for other platforms.

### 7. X11 path

Qt `xcb`: frameless tool window with a stay-on-top hint, position computed from
`QScreen::availableGeometry`, same `activeChanged` dismissal. On this machine it
can only be exercised through XWayland and is labelled so.

## Rejected mechanisms

| Mechanism | Why not |
|---|---|
| KWin scripting (`loadScript` over D-Bus) to place a normal window | Injects code into the compositor, KDE-only, persists until unloaded, fragile across KWin versions. Considered only as a user-approved last resort. |
| Global input monitoring (evdev, libinput, InputCapture portal) for outside clicks | Needs privileges or a user-visible grant, and is exactly the intrusive polling the requirements forbid. |
| `org_kde_plasma_shell` positioning | Intended for the shell itself, not stable for third-party clients. |
| Plain `xdg_toplevel` | Compositor chooses the position; no always-on-top. |
| `org_kde_kwin_fake_input` / `xdotool` for the app's own behaviour | Test-only at best, and not available to normal clients. |

## Spikes (gates before Phase 3)

| # | Question | Pass criterion |
|---|---|---|
| S1 | Does a layer surface land bottom-right, above the panel, with the right margin and fractional scale? | Visible at the expected corner; no overlap with the panel. |
| S2 | Does typing work immediately after the shortcut, with no click? | Key events reach the input without a click, logged with timestamps. |
| S3 | Does clicking another window dismiss the popup and still deliver the click? | `activeChanged(false)` fires; the target window receives the click. |
| S4 | Does the portal bind, persist across restarts and report conflicts? | One entry in KDE's shortcut settings; works after restart; trigger readable. |
| S5 | Does the `ksni` tray icon show, toggle on click and menu correctly in Plasma? | Observed in the real tray. |
| S6 | Multi-monitor and mixed scaling | Manual: needs a second output or a nested KWin. Not testable on the dev machine today. |

**How I will observe results without capturing your screen:** the app logs
monotonic timestamps for shortcut received, `setVisible`, first frame, active
change and input focus. If a screenshot is really needed I will ask first and
capture only the popup's window, never the whole screen.
