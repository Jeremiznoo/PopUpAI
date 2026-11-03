# Branding prompt (logo and banner)

A prompt to paste into ChatGPT (image generation) to get the logo and the banner of PopUpAI.
Generate the **logo first**, then ask for the banner reusing it ("use the logo you just made").
Save the results as `docs/assets/logo.png` and `docs/assets/banner.png`, then uncomment the two
lines at the top of the README.

## The prompt

```text
Design the visual identity for "PopUpAI", a lightweight desktop app: you press a global keyboard
shortcut, a small floating card pops up in the bottom-right corner of the screen with a text field
already focused, you ask an AI a quick question, read the streamed answer, and click elsewhere to
make it disappear. It is fast, private, minimal and native on Linux (KDE Plasma), and works with
local models or AI subscriptions. Tone: calm, precise, modern, a little playful. Not corporate,
not "robotic".

Create these four images, one after the other, in the same style:

1. APP ICON, 1024x1024, transparent background, PNG. A rounded-square "pop-up card" seen straight
   on, with a soft inner glow and a subtle drop shadow, containing a single simple symbol: a text
   caret (a thin vertical bar) next to a small four-point sparkle, which together read as
   "type here, intelligence appears". A tiny corner notch or tail at the bottom-right of the card
   hints that it pops up from the corner of the screen. Must stay readable at 32x32 and 16x16:
   few shapes, strong silhouette, no fine detail, no text.

2. README BANNER, 1600x400 (4:1), dark background. On the left, the logo; next to it the word
   "PopUpAI" in a clean geometric sans-serif (spelled exactly P-o-p-U-p-A-I, no other text) and the
   tagline "Ask anything. Right where you are." in a lighter weight underneath. On the right, a
   stylised, simplified mockup of the floating card (rounded, with a text field showing a blinking
   caret and two short lines of an answer), slightly tilted, floating over soft abstract shapes.
   Wide safe margins on all sides so nothing is cut when cropped.

3. GITHUB SOCIAL PREVIEW, 1280x640 (2:1), same style as the banner, centred composition: logo,
   name and tagline, floating card behind them, generous empty space.

4. MONOCHROME LOGO, 1024x1024, transparent background, pure white symbol only (no card
   background, no glow), for use on dark and coloured surfaces.

Style and palette: flat/vector look with soft depth, no photorealism, no 3D render, no stock
"AI brain", robot, circuit or neon-matrix clichés. Dark navy background (#0F1220) with deep
indigo (#1B1F3B) surfaces; accent gradient from violet (#7C5CFF) to sky blue (#4CC3FF); warm
highlight dot in soft coral (#FF8A7A) used very sparingly; text in off-white (#F2F4FF). Rounded
corners everywhere (about 22% radius on the icon). It must look equally good on a light and a dark
README.

Hard rules: write only the exact text given above, spelled exactly, no extra words, no lorem
ipsum, no watermarks, no mock UI text other than abstract grey bars. Keep it original: do not
imitate an existing logo. Give me each image as a downloadable PNG.
```

## After generating

- If the text in the banner is misspelled, ask: "Redo it keeping everything, but spell the name
  exactly P-o-p-U-p-A-I and remove any other text." Or ask for the banner **without text** and add
  the name yourself in an image editor.
- For a crisp vector, ask a second time: "Now redraw the app icon as a clean SVG with at most 6
  shapes."
- Put the final files in `docs/assets/` and commit them.
