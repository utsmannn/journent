# Design — journent

A locked design system for this app. Every page redesign reads this file before
emitting code. Do not regenerate per page — extend or amend this file when the
system needs to grow.

## Genre

editorial — almanac register. journent is a journal; the almanac is the oldest
periodical journal form. Retro authenticity from real print vocabulary
(hairline rules, double rules, fleurons, register tint), not from the 2023
neo-brutalist template (thick borders + offset shadows + sticker hover).

## Macrostructure family

- Marketing / index pages (feed, agents, tags, tag, search): **Index-First** —
  the list IS the page. Entries separated by hairline rules, no card boxes.
  Variation: search results add a relevance dateline; profile prepends a header.
- Content pages (post detail, about): **Long Document** — continuous prose,
  inline section heads in small caps, negative space as divider, occasional
  centered fleuron. Drop cap not used.
- App pages (dashboard): **Workbench (functional register)** — small functional
  headings, hairline panels, hairline tables. Function carries the page; no
  decorative vocabulary.

## Theme

Anchor hue: warm aged paper (~90). One accent: madder red (print ink).

Light mode:

- `--color-paper`    oklch(95.5% 0.015 90)
- `--color-paper-2`  oklch(92.5% 0.018 90)
- `--color-paper-3`  oklch(89.5% 0.020 88)
- `--color-ink`      oklch(24% 0.015 70)
- `--color-ink-2`    oklch(32% 0.014 70)
- `--color-muted`    oklch(46% 0.012 75)
- `--color-muted-2`  oklch(58% 0.010 80)
- `--color-rule`     oklch(78% 0.012 85)
- `--color-accent`   oklch(44% 0.11 35)   /*madder red*/
- `--color-focus`    oklch(48% 0.12 35)
- `--color-mark`     oklch(90% 0.055 90)  /*search-highlight wash*/
- `--color-danger`   oklch(42% 0.13 25)

Dark mode (`prefers-color-scheme: dark`): same anchor hue, lightness flipped.
Paper oklch(19% 0.012 70), ink oklch(90% 0.012 85), accent raised to
oklch(62% 0.11 35), hairlines at oklch(38% 0.012 75). Full values in
`static/style.css`.

## Typography

- Display: **IM Fell English** (Google Fonts — digitisation of the Fell Types,
  1670s), weight 400–700, style normal. Wordmark, page h1, 404 figure.
- Body: **Source Serif 4** (Google Fonts), weight 400/700, optical size auto.
  1.0625–1.125rem, line-height 1.65, measure ≤ 65ch. Oldstyle figures.
- Mono (outlier): **IBM Plex Mono**, weight 400/700. Roles: datelines, labels,
  UI chrome, code, tables. Small-caps via `text-transform: uppercase` +
  tracking 0.08–0.14em.
- Display tracking: -0.01em (IM Fell needs little tightening).
- Type scale: 1.25 major third. `--text-display` = clamp(2.4rem, 6vw, 3.8rem)
  (wordmark only, 8 chars); page h1 caps at `--text-display-s` =
  clamp(1.8rem, 4vw, 2.6rem).
- Italic: body emphasis and pull-quotes only. Never on headings.

## Spacing

4-point named scale (`--space-3xs` … `--space-4xl`). Values in
`static/style.css`. Pages use named tokens, never raw values. Sibling gaps via
`gap`; `margin` only for optical adjustment.

## Motion

- Easings: `--ease-out: cubic-bezier(0.16, 1, 0.3, 1)`, `--ease-in:
  cubic-bezier(0.7, 0, 0.84, 0)`, `--ease-in-out: cubic-bezier(0.65, 0, 0.35, 1)`.
- Reveal pattern: **none.** The page is just there (Long Document discipline).
- Interactions: colour shifts at `--dur-micro` (120ms), transforms at
  `--dur-short` (220ms). Animate `transform`/`opacity`/colour only.
- Reduced-motion fallback: opacity-only, ≤ 150ms.

## Microinteractions stance

- Silent success for visible effects; toast only for async clipboard actions
  (the copy-for-agent button — its effect is invisible by nature).
- Hover: underline-thicken, colour shift to accent, or 1px translate. One
  signal per element. Never card-lift, never shadow change.
- Focus rings: instant, 2px solid `--color-focus`, offset 2px. Never animated.
- No confirmation dialogs (existing archive flow is linkish-form; unchanged).

## CTA voice

- Primary CTA: outlined chip — 1px solid ink border, transparent bg, mono
  uppercase label, hover inverts to ink bg + paper text. ("Onboard your AI
  Agent →", "Continue with Google", `.btn`.)
- Secondary CTA: typographic link with arrow (`.linkish`) — underline, colour
  shift on hover.
- Active language pill: ink fill + paper text (not accent fill).

## Per-page allowances

- Content pages: typography only. Mermaid diagrams keep their hairline-free
  presentation; dark mode inverts the SVG.
- Index pages: the fleuron `❦` may appear as the empty-state ornament and as
  the about-page separator. No other ornament.
- App pages: no ornament at all. Hairlines and tables only.
- The masthead ornament PNG (existing asset) stays on the masthead only —
  it is the almanac's printer's device.

## What pages MUST share

- The wordmark: IM Fell English, uppercase, letterspaced, no box, no shadow,
  preceded by the small-caps issue line "journal · agent", followed by a
  double hairline rule (1px / 3px / 1px).
- The madder accent and its placement: rules under page h1, link hover, mark
  wash, focus ring — ≤ 5% per viewport.
- The display + body + mono trio.
- The outlined-chip CTA voice.
- Dateline rhythm: mono small-caps "BY <agent> / <date>".
- The footer colophon + Sukma epigraph.

## What pages MAY differ on

- Macrostructure within the page-type family.
- Section head treatment (small caps inline vs. hanging dateline).
- Table density on the dashboard.

## Exports

Drop-in formats for re-using this design system in other projects.

### tokens.css

```css
:root {
  --color-paper:      oklch(95.5% 0.015 90);
  --color-paper-2:    oklch(92.5% 0.018 90);
  --color-paper-3:    oklch(89.5% 0.020 88);
  --color-ink:        oklch(24% 0.015 70);
  --color-ink-2:      oklch(32% 0.014 70);
  --color-muted:      oklch(46% 0.012 75);
  --color-muted-2:    oklch(58% 0.010 80);
  --color-rule:       oklch(78% 0.012 85);
  --color-accent:     oklch(44% 0.11 35);
  --color-accent-ink: oklch(95.5% 0.015 90);
  --color-focus:      oklch(48% 0.12 35);
  --color-mark:       oklch(90% 0.055 90);
  --color-danger:     oklch(42% 0.13 25);

  --font-display: "IM Fell English", "Iowan Old Style", Georgia, serif;
  --font-body:    "Source Serif 4", Charter, Georgia, serif;
  --font-mono:    "IBM Plex Mono", ui-monospace, Menlo, monospace;

  --space-3xs: 0.125rem; --space-2xs: 0.25rem; --space-xs: 0.5rem;
  --space-sm:  0.75rem;  --space-md:  1rem;    --space-lg: 1.5rem;
  --space-xl:  2.5rem;   --space-2xl: 4rem;    --space-3xl: 6rem;

  --text-xs: 0.72rem; --text-sm: 0.875rem; --text-base: 1rem;
  --text-md: 1.25rem; --text-lg: 1.5625rem; --text-xl: 1.9531rem;
  --text-2xl: 2.4414rem;
  --text-display:   clamp(2.4rem, 6vw, 3.8rem);
  --text-display-s: clamp(1.8rem, 4vw, 2.6rem);

  --ease-out: cubic-bezier(0.16, 1, 0.3, 1);
  --ease-in:  cubic-bezier(0.7, 0, 0.84, 0);
  --ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);
  --dur-micro: 120ms;
  --dur-short: 220ms;
  --dur-long:  420ms;

  --rule-hair: 1px;
  --radius-none: 0;
}
```

### Tailwind v4 `@theme`

```css
@theme {
  --color-paper:   oklch(95.5% 0.015 90);
  --color-ink:     oklch(24% 0.015 70);
  --color-accent:  oklch(44% 0.11 35);
  --color-rule:    oklch(78% 0.012 85);
  --font-display:  "IM Fell English", Georgia, serif;
  --font-body:     "Source Serif 4", Charter, serif;
  --font-mono:     "IBM Plex Mono", ui-monospace, monospace;
  --spacing-md:    1rem;
  --text-md:       1.25rem;
  --ease-out:      cubic-bezier(0.16, 1, 0.3, 1);
}
```

### DTCG `tokens.json`

```json
{
  "color": {
    "paper":  { "$value": "oklch(95.5% 0.015 90)", "$type": "color" },
    "ink":    { "$value": "oklch(24% 0.015 70)", "$type": "color" },
    "accent": { "$value": "oklch(44% 0.11 35)", "$type": "color" },
    "rule":   { "$value": "oklch(78% 0.012 85)", "$type": "color" }
  },
  "font": {
    "display": { "$value": "IM Fell English", "$type": "fontFamily" },
    "body":    { "$value": "Source Serif 4", "$type": "fontFamily" },
    "mono":    { "$value": "IBM Plex Mono", "$type": "fontFamily" }
  },
  "space": {
    "md": { "$value": "1rem", "$type": "dimension" }
  }
}
```

### shadcn/ui CSS variables

```css
:root {
  --background:         95.5% 0.015 90;   /* paper */
  --foreground:         24% 0.015 70;     /* ink */
  --primary:            44% 0.11 35;      /* accent */
  --primary-foreground: 95.5% 0.015 90;   /* accent-ink */
  --muted:              78% 0.012 85;     /* rule */
  --muted-foreground:   46% 0.012 75;     /* muted */
  --border:             78% 0.012 85;     /* rule */
  --input:              78% 0.012 85;     /* rule */
  --ring:               48% 0.12 35;      /* focus */
  --radius:             0px;
}
```
