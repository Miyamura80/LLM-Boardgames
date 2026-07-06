---
name: secret-hitler-brand
description: The Secret Hitler visual brand (colors, fonts, design language) shared by every frontend in this repo — the Vite eval console and the Next.js/Fumadocs docs. Use whenever building or restyling any UI, adding a new frontend, choosing colors/typography, or generating screenshots/marketing so the look stays consistent with the board game.
---

# Secret Hitler Brand

The single source of truth for how UIs in this repo look. Every frontend — the
Vite eval console (`frontend/`), the Next.js/Fumadocs docs (`docs/`), and any
new surface — **must** adopt these tokens instead of inventing its own palette.

Aesthetic target: the board game's **1930s Weimar / Art-Deco propaganda-poster**
look — aged parchment, fascist red-orange, liberal teal, mustard gold, heavy
ink, condensed display type, and a typewriter accent for "classified dossier"
flavor.

## Canonical assets (in this skill)

```
assets/secret-hitler-brand.css   ← the token file: palette + @font-face + type
assets/fonts/oswald-latin.woff2         ← display font (Art-Deco condensed)
assets/fonts/special-elite-latin.woff2  ← typewriter accent (dossier flavor)
```

`assets/secret-hitler-brand.css` is the source of truth for the numbers. Edit
the palette/type **there** and re-propagate; do not fork values into components.

## Palette

```
                          light "Parchment"     dark "Noir"
  ────────────────────────────────────────────────────────────
  Fascist   (primary)     #C33A22               #E0553B
  Liberal                 #2E7C8C               #4FA9BC
  Hitler / darkest ink    #17110B               #17110B
  Gold      (accent)      #C79A3E               #D8AE4E
  ────────────────────────────────────────────────────────────
  Page bg                 #ECE0C6               #16120C
  Surface (cards)         #F6EEDA               #20190F
  Sunken (wells/zebra)    #E3D4B4               #120E09
  Ink (text)              #211A12               #F1E6CF
  Ink soft (secondary)    #6D5E46               #B09A76
  Rule / border           #CDB98F               #3A2F1F
```

Faction color-coding is load-bearing: **liberal = teal, fascist = red-orange,
hitler = near-black.** Never swap these. Color is never the *only* signal —
always pair with text or a marker (accessibility + the game's own convention).

## Typography

```
  Display  → Oswald         condensed geometric; UPPERCASE + 0.06em tracking.
             Headers, logo/wordmark, tabs, table headers, buttons, stat labels.
  Body     → system sans    "Inter", ui-sans-serif, system-ui, …  Data, prose.
  Dossier  → Special Elite  typewriter; flavor / "classified" text — private
             reasoning, secret-role reveals, epigraphs. Use sparingly.
  Mono     → ui-monospace   numbers, seeds, ids, token counts.
```

Tokens: `--sh-font-display`, `--sh-font-body`, `--sh-font-typewriter`,
`--sh-font-mono`. Apply the display transform with
`text-transform: var(--sh-display-transform); letter-spacing: var(--sh-display-tracking);`.

## Design language

- **Art-Deco geometry:** crisp rectangles, thin rules, minimal rounding
  (`--sh-radius: 3px`). Avoid big pill shapes and soft shadows.
- **Official-document feel:** hairline `--sh-line` borders, occasional double
  rules, uppercase Oswald section labels.
- **Ink-first buttons:** solid ink (light) / parchment (dark) fills, or
  fascist-red for a primary/destructive call. Uppercase Oswald label.
- **Two themes:** light `Parchment` (default) and dark `Noir`. Support both.

## Applying the brand to a frontend

Two supported wiring patterns; both apps in this repo already use them.

### Plain CSS / Vite (`frontend/`)

1. Copy `assets/secret-hitler-brand.css` → `frontend/src/brand.css`.
2. Copy `assets/fonts/*` → `frontend/public/fonts/` (the `url(/fonts/…)` paths
   already match Vite's static root).
3. `@import "./brand.css";` at the top of the global stylesheet, then map the
   app's own tokens onto the brand:
   ```css
   :root {
     --color-bg: var(--sh-bg);
     --color-surface: var(--sh-surface);
     --color-text-primary: var(--sh-ink);
     --font-sans: var(--sh-font-body);
     /* headers → --sh-font-display, faction hues → --sh-fascist/liberal/hitler */
   }
   ```

### Tailwind v4 + Fumadocs (`docs/`)

1. Copy `assets/fonts/*` → `docs/public/fonts/`.
2. In `docs/app/global.css`, declare the two `@font-face`s (paths `/fonts/…`),
   then **map** Fumadocs vars to brand hues in both `@theme inline` (light) and
   `:root.dark` (dark):
   ```css
   @theme inline {
     --color-fd-background: #ece0c6;
     --color-fd-primary:    #c33a22;   /* fascist red */
     --color-fd-accent:     #c79a3e;   /* gold        */
     --font-display: "Oswald", …;      /* if wiring a display family */
   }
   ```
   Keep the Fumadocs sidebar/TOC dark overrides pointed at the Noir surfaces.

### A brand-new frontend

Copy the token file + fonts, import once, then express every color/font through
`--sh-*`. If it has its own design-token layer, map that layer onto `--sh-*`
rather than duplicating hexes.

## Screenshots / marketing

Render in the real UI (mock the API if needed), prefer the **Noir** theme for
hero shots, and caption fabricated data as *illustrative*. The repo README's
mid-game preview is produced this way.

## Guardrails

- Do **not** hardcode brand hexes in components — go through `--sh-*`.
- Do **not** redefine the palette per-app — map framework tokens onto `--sh-*`.
- Do **not** break faction coding (teal/red/black) or rely on color alone.
- When the brand changes, edit `assets/secret-hitler-brand.css` here first, then
  re-sync the copies in `frontend/` and `docs/`.
