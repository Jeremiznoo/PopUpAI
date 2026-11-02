# Spike S1 to S3: layer-shell window on KWin

A throwaway measurement tool, not product code. It answers the open questions of
[ADR 0002](../../docs/ADR/0002-linux-desktop-integration.md) about showing a popup
on KDE Plasma / Wayland with `wlr-layer-shell` through LayerShellQt:

| Spike | Question | How it is observed |
|---|---|---|
| S1 | Does the window land bottom-right, above the panel? | By eye. A Wayland client cannot read its own screen position. |
| S2 | Does it get keyboard focus without a click? | `active` becomes true after being shown; typed keys are counted. |
| S3 | Is it deactivated when another window is clicked? | `active` becomes false; the window then closes. |

Typed characters are **never** logged, only how many keys arrived. The program
makes no network access and reads nothing from the user's session beyond the screen
list.

## Build and run

```bash
cmake -S spikes/s1-layer-shell -B target/spike-s1 -DCMAKE_BUILD_TYPE=Release
cmake --build target/spike-s1
QT_QPA_PLATFORM=wayland target/spike-s1/spike_s1 --seconds 45
```

Options: `--seconds N` (default 40), `--interactivity ondemand|exclusive|none`
(default `ondemand`), `--margin PX` (default 16), `--keep-on-focus-loss`.
`exclusive` grabs the keyboard until the window closes, so use it only with a short
`--seconds`.

`QT_QPA_PLATFORM=offscreen` runs the program logic with no window and no compositor.
That checks the timers and the summary only: its `active=1` means nothing for KWin.

## Findings so far

LayerShellQt 6.8.2 / Qt 6.11.2 on KDE Plasma 6.8.2, one output (`eDP-1`,
1920x1200, scale 1), 2026-11-02. `Shell::useLayerShell()` is deprecated since
Qt 6.6 and not needed, so the application should not call it.

**Run 1** (`--interactivity ondemand`, launched from the Claude desktop app's
shell tool, no keyboard or mouse input intended):

```
[    72 ms] showing the window
[   106 ms] first frame presented 34 ms after being shown
[   128 ms] focus object: QQuickTextInput
[   128 ms] active=1
[   128 ms] became active 56 ms after being shown, without any click
[   377 ms] focus object: none
[   378 ms] active=0
[   378 ms] SUMMARY reason="focus lost ..." shown_to_active_ms=56 keys=0 ever_active=1
```

What this shows, and what it does not:

- **S2, promising but not closed.** The window became active 56 ms after being
  shown with no click, so KWin grants keyboard focus to an on-demand layer surface
  when it is mapped. No key was typed during the run, so delivery of typed keys is
  not confirmed yet.
- **S3, not established.** The window lost focus 250 ms later. No pointer event
  reached the window and no key was typed. Whether a click elsewhere caused it is
  unknown, and a human click within 250 ms is unlikely. The cause is unidentified:
  it could be something that re-took focus, or an effect of how the program was
  launched. A rerun with `--keep-on-focus-loss` is needed to see whether focus
  comes back and what the pattern is.
- **S1, not confirmed.** The placement has to be confirmed by eye; Qt reports the
  whole screen as available and cannot see the panel.
- **Local latency is promising:** about 34 ms from show to first frame, and 56 ms
  to focus, on this machine with the window created at start-up (a cold start, not
  yet the warm path the product will use). These are single runs, not a benchmark.
