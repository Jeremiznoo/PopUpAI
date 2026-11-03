# PopUpAI: art direction

One idea: **a small card that appears exactly where you are, and gets out of the way.** Everything
(the logo, the colours, the motion, the words) serves that: calm, fast, a little playful, never
corporate and never "robotic".

## The name

**PopUpAI**, written exactly like that (capital P, U, A, I). Never "Popup AI", "POPUPAI" or "Pop-Up AI".
The application id is `io.github.jeremiznoo.PopUpAI`; the command is `popupai`.

## The logo

A rounded-square **speech card** with a small tail at the bottom-right corner (where the popup
appears on screen), holding a **text caret** and a **four-point spark**: "type here, intelligence
appears".

| File | Use |
|---|---|
| `logo.svg` | The master. Everything else comes from it. |
| `png/logo-{16..1024}.png` | Raster sizes (icon theme, tray, store listings). |
| `logo-mono.svg`, `png/logo-mono-1024.png` | One colour (white outline), for dark or coloured surfaces. |
| `logo-symbol.svg` | The caret and spark alone, white, for tiny or decorative use. |
| `banner.png` | README banner (1600 x 533). |
| `social-preview.png` | GitHub social preview (1280 x 640): upload it in the repository's *Settings, Social preview* (GitHub has no API for it). |
| `source/` | The original generated images, kept untouched. The original icon had stray pixels around the card, so the SVG was redrawn from it. |

Rules
- **Clear space:** at least the width of the caret (about 1/19 of the logo's width) on every side.
- **Minimum size:** 16 px (the caret and spark still read). Below that, use `logo-symbol.svg`.
- On light backgrounds keep the full-colour logo; on photos or busy backgrounds use `logo-mono`.
- Do not recolour the gradient, rotate, stretch, add outlines or shadows, or put the card inside another shape.

## Colour

The card's gradient runs from top-left to bottom-right: pink-violet, violet, blue, cyan.

| Token | Hex | Use |
|---|---|---|
| Pink | `#B85CFF` | Gradient start |
| Violet | `#6B4BFA` | Main accent: focus ring, selection, send button |
| Blue | `#3A5BF5` | Gradient middle, send button end |
| Cyan | `#18D2FF` | Gradient end |
| Coral | `#FC7C79` | A single spark of warmth, used very sparingly |
| Night | `#101434` / `#181E51` | Banner and marketing backgrounds |
| Ink | `#FCFCFD` | Text and symbols on the gradient or on night |
| Violet text | `#A78BFA` on dark, `#5B3DE0` on light | Accent text (section titles, the sign-in code) |

**In the application the neutrals come from the user's system theme** (Plasma's window, base and text
colours), so it looks native in light and dark. The brand only supplies the **accents**: never paint
the whole window in brand colours.

## Shape and depth

- Rounded everywhere: 18 px card, 12 px field, 8 px rows, circles for buttons.
- One soft shadow under the card, no hard borders (a 1 px line at 12 % opacity at most).
- Flat colour with a gentle gradient or glow; no 3D, no glass overload, no photographic textures.

## Motion

Short and quiet. The card **fades and settles in** (about 150 ms, ease-out, from 96.5 % to 100 %, from
the bottom-right corner). Waiting is three violet dots that pulse in turn. Nothing bounces, nothing
loops except the waiting dots.

## Typography

- In the application: the system font, for a native feel; monospace for commands and code.
- In the banner and marketing: a rounded geometric sans-serif for the name (the generated banner uses
  one close to Outfit or Plus Jakarta Sans; use either when you need to set text yourself), regular weight
  for the tagline.

## Voice

Short, direct, human. Say what happens and what to do, never blame the user.

- Tagline: **Ask anything, anywhere on my desktop.** (French: *Posez votre question, là où vous êtes.*)
- Prefer "Paste your key" to "Authentication credentials required".
- Honest about limits: the interface says "not run", "declined", "not connected", never hides a failure.

## Where the brand lives in the product

| Place | What |
|---|---|
| Launcher and system icon | `packaging/icons/hicolor/*` (installed by `tools/install-local.sh`), `Icon=io.github.jeremiznoo.PopUpAI` |
| Notification-area icon | Pixmaps embedded in `popupai-platform` (`assets/tray-*.argb`, made from `png/`) |
| Popup, settings and picker | The mark drawn in QML from the SVG's coordinates, next to the model line and in the page titles; violet focus ring, send button, selection and pulsing dots |
| README | `banner.png` |
| GitHub | `social-preview.png`, repository description and topics |

## Regenerating

The icon sizes come from the SVG:

```bash
for s in 16 22 24 32 48 64 128 256 512 1024; do
  magick -background none -density 384 branding/logo.svg -resize ${s}x${s} branding/png/logo-$s.png
done
```

`branding/PROMPT.md` is the prompt that produced the original images.
