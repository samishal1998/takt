# Takt — Brand and Documentation Guide

**Selected direction:** Concept 02 Stack · **Kit:** v0.1 · **Date:** 16 October 2026

## Identity

The mark consists of three horizontal rounded bars, staggered left and right. Preserve this geometry: it suggests queued work and controlled overlap without using the waveform or clock concepts. The wordmark is **Takt**, with a capital T. The executable is `takt`.

Primary lockup: the gradient mark plus light wordmark on a dark navy background. Use the dark-wordmark variant on light backgrounds. Core tagline: **Schedule. Balance. Flow.** Supporting descriptor: **A resource-aware command runner.** The descriptor is positioning, not an assertion that implementation is complete.

## Production provenance

These SVGs are clean vector reconstructions of the chosen raster reference, with standardized geometry and an outlined wordmark. They are not original editable vector files extracted from the image generator. Original concept boards are archived under `reference/`; several contain rejected directions and must not be mistaken for production alternatives.

All production wordmarks and signature text are paths. They render without font downloads or installed fonts. Geometry remains editable in an SVG editor. No font binaries are included.

## Mark construction and spacing

The source grid is 120 × 92 units. Each bar is 92 × 24 units with radius 12. Top origin: (12, 0). Middle: (28, 34). Bottom: (0, 68). The diagonal color progression runs from blue at the lower-left through cyan to mint at the upper-right. The machine-readable definition is `source/mark-geometry.json`.

Keep at least one bar thickness (24 source units) of clear space around visible logo bounds. Some convenience exports use a compact viewport; add external layout space as necessary. Never stretch the mark independently in one axis, reorder its bars, rotate it into the waveform, add a fourth bar, or use the rejected hybrid as the product logo.

Proposed minimum rendered sizes: mark-only 20 px wide for ordinary UI; supplied favicon artwork may be used at 16 px; horizontal wordmark lockup 140 px wide; signature with tagline 320 px wide. Omit the tagline before it becomes unreadable. At tiny sizes prefer the supplied icon/mark rather than scaling an entire banner down.

## Palette

| Token | Value | Use |
|---|---|---|
| Navy / ink | `#071A23` | Primary dark background and light-theme text |
| Paper | `#F6FAFC` | Light background and dark-theme body text |
| Blue | `#009DF5` | Lower end of mark gradient, decorative accent |
| Cyan | `#00BFD1` | Central brand accent |
| Mint | `#00D6A0` | Upper gradient, dark-theme links |
| Deep teal | `#00766D` | Light-theme links and readable accent text |
| Light muted | `#526879` | Secondary text on paper |
| Dark muted | `#A8BECA` | Secondary text on navy |

Use the exact gradient stored in the SVG rather than approximating each bar separately. One-color assets must remain a single flat ink or white. Use the dedicated teal mark when a flat chromatic mark is needed.

## Typography and layout

The standardized outlined wordmark uses Inter Display Bold glyph shapes with optical spacing. Documentation CSS uses an optional Inter family followed by platform UI fallbacks; the preview fetches no font files. Technical examples use the operating system's monospace stack. Use the supplied wordmark SVG instead of recreating it with live text.

Use a readable 16 px base size, around 1.7 line height, a maximum reading width of 76 characters, and generous section spacing. Keep commands in code blocks. Use restrained 8/16 px rounding and a 4/8/12/16/24/32/48/64/96 px spacing scale. Avoid gradients behind body text, excessive shadows, ornamental waveform graphics, and color-only statuses.

## Contrast and accessibility

Measured solid-color contrast pairs: ink/paper 16.93:1; muted-light/paper 5.53:1; deep-teal/paper 5.25:1; muted-dark/navy 9.21:1; mint/navy 9.42:1. Bright cyan on white is only 2.24:1, and bright mint on white is 1.89:1. Do not use those bright accents for small text on white.

The calculation follows the WCAG sRGB relative-luminance formula. WCAG's ordinary-text contrast threshold is 4.5:1; passing these token pairs is not a complete accessibility audit of a website. See the [W3C contrast guidance](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) and `contrast-report.json`.

Give a linked product logo the accessible name “Takt.” Use empty alt text for a purely decorative repeated mark beside an already labelled heading. Preserve keyboard focus indicators. Label states with text, not color alone. Respect reduced-motion settings.

## Picking assets

| Placement | Preferred asset |
|---|---|
| Dark README/docs header | `svg/takt-horizontal-on-dark.svg` |
| Light README/docs header | `svg/takt-horizontal-on-light.svg` |
| Print / single-ink | `svg/takt-horizontal-mono-ink.svg` |
| Dark one-color surface | `svg/takt-horizontal-mono-white.svg` |
| Square avatar/app preview | `icons/takt-icon-color-dark-512.png` |
| Favicon | `icons/favicon.ico` or size-specific PNG |
| Monochrome pinned-tab input | `icons/pinned-tab.svg` |
| Documentation hero | `banners/takt-docs-banner-dark.svg` or light equivalent |
| Small UI label | `svg/takt-mark-color.svg` |

Paths in this table are relative to the `brand/` root. Transparent `on-dark` assets intentionally contain a light wordmark; they may appear blank on a white preview background. The square icon exports have a designed tile background with transparent rounded corners. No `.icns` or platform application bundle is claimed to be included.

## Documentation starter files

`takt-docs.css` is a standalone, no-build stylesheet with light/dark variables, tables, code blocks, cards, focus styles, and responsive layout. `documentation-template.html` previews a documentation page. `readme-snippet.md` contains a copyable repository header. `design-tokens.json` supplies machine-readable color, typography, spacing, and layout values. `preview/index.html` is the asset gallery.

Typography reference: [Inter's official project](https://rsms.me/inter/). The kit does not assign a software license, grant a new font license, or claim trademark/name availability; those owner decisions remain separate.
