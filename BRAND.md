---
name: Cairn
description: Brand and visual identity for Cairn, a drop shelf for Windows 10 and 11. A quiet, native identity built on basalt grays, stone neutrals and one lichen accent.
colors:
  background: "#121417"
  surface-sunken: "#0d0f11"
  surface: "#1b1e22"
  surface-hover: "#23272c"
  surface-flyout: "#181b1ff2"
  divider: "#2a2f34"
  outline: "#30353b"
  outline-control: "#767d79"
  text: "#eef0ec"
  text-secondary: "#9ea4a0"
  stone: "#b9b4aa"
  primary: "#c6e36b"
  primary-hover: "#d3ea8a"
  primary-pressed: "#b0cc58"
  primary-subtle: "#c6e36b26"
  on-primary: "#121417"
  danger: "#ff8a80"
  focus-ring: "#eef0ec"
typography:
  wordmark:
    fontFamily: Segoe UI Variable Display
    fontSize: 32px
    fontWeight: 600
    letterSpacing: -0.01em
  title:
    fontFamily: Segoe UI Variable Display
    fontSize: 20px
    fontWeight: 600
    lineHeight: 28px
  body:
    fontFamily: Segoe UI Variable Text
    fontSize: 14px
    fontWeight: 400
    lineHeight: 20px
  body-strong:
    fontFamily: Segoe UI Variable Text
    fontSize: 14px
    fontWeight: 600
    lineHeight: 20px
  caption:
    fontFamily: Segoe UI Variable Text
    fontSize: 12px
    fontWeight: 400
    lineHeight: 16px
  code:
    fontFamily: Cascadia Mono
    fontSize: 13px
    fontWeight: 400
    lineHeight: 20px
rounded:
  control: 4px
  overlay: 8px
  tile: 8px
  pill: 999px
spacing:
  xs: 4px
  s: 8px
  m: 12px
  l: 16px
  xl: 24px
---

# Cairn brand

Cairn is a drop shelf for Windows. This file defines what the brand says and how it looks. Product scope is in [PRD.md](PRD.md). The logo files are in [brand/](brand/).

## The name

A cairn is a small stack of stones that hikers leave on a trail, so they and others can find the way back. It is put down on purpose, temporary, and easy to spot.

That is the product: you set things down as you go and pick them up where you need them. The name is short, real, easy to say, and not yet a cliché in the category, where most names lean on "drop", "shelf" or "stash".

Name status: working name, pending a trademark search ([PRD.md](PRD.md) §14).

## Brand idea

**Set it down. Pick it up anywhere.**

Cairn makes holding something while you move between apps feel as natural as putting it on a table.

Taglines, in order of preference:
1. Set it down. Pick it up anywhere.
2. A place to put things, at the edge of your screen.
3. Drag it here. Drop it there.

## Personality

| Cairn is | Cairn is not |
|----------|--------------|
| Calm: appears only when needed, never nags | Cute or mascot-driven |
| Sure-footed: does exactly what it says, every time | Clever for its own sake |
| Native: looks like it shipped with Windows | Flashy, glassy or gradient-heavy |
| Respectful: your files stay where they are; your data stays on your PC | Pushy about upgrades |

## Voice

Short and literal, like Windows at its best. Sentence case, no exclamation marks, no emoji.

- In the product, say what happened and what to do next: "File not found. It was moved or deleted. Press Del to remove it."
- Marketing may use the trail metaphor once per page, lightly: in a headline, never in UI labels.
- Name things by what they are. In the UI an item is an **item** and a group is a **stack**. Never "stones", "pebbles" or "trail" in UI text.

| Do | Don't |
|----|-------|
| "Drop here" | "Toss it on the pile!" |
| "3 items" | "3 stones stacked" |
| "Can't accept items from an admin window" | "Oops! Something went wrong" |
| "Cairn Pro adds more stacks and quick actions." | "Unlock the full power of Cairn!" |

## Logo

The mark is three stones, stacked slightly off-center the way hands stack them. The top stone, the one just placed, carries the lichen accent. It stands for the item you just dropped.

| File | Use |
|------|-----|
| [cairn-mark.svg](brand/cairn-mark.svg) | Mark on dark backgrounds |
| [cairn-mark-light.svg](brand/cairn-mark-light.svg) | Mark on light backgrounds |
| [cairn-app-icon.svg](brand/cairn-app-icon.svg) | App, Store and installer icon (basalt tile) |
| [cairn-mono.svg](brand/cairn-mono.svg) | One color, pixel-snapped for the tray at 16 px. Fill is `currentColor`. |

Construction: on a 64 grid, the stones are 48 × 13, 35 × 12 and 19 × 11 with fully rounded ends and 3-unit gaps. The middle stone sits left of center and the top stone right of it.

Rules
- Clear space: the height of the top stone on every side.
- Minimum size: 16 px (use the mono file below 24 px).
- Wordmark: "cairn" in lowercase, Segoe UI Variable Display Semibold, tracking −1%, set to the right of the mark with a gap of one top-stone width. The mark is 1.25 times the wordmark's font size (a 40 px mark with 32 px text) and sits on the text baseline. Outline the text before using it in shipped artwork.
- Don't recolor stones individually, add outlines or shadows, rotate the mark, rebalance the stack, or put the full-color mark on a busy photo.

## Color

Dark is the default, matching how Cairn sits beside dark Windows chrome. Basalt grays build the surfaces. **Stone** is a warm neutral for the mark and illustrations. **Lichen** (`primary`) is the one accent.

- Use lichen at most once per view: the drop zone while a drag hovers, the selected tile, or the primary button.
- Stone is for the brand mark and illustration, not UI text.
- Status colors always come with an icon or a word.

Light theme (same token names):

| Token | Light value |
|-------|-------------|
| background | `#f4f2ee` |
| surface-sunken | `#ebe8e2` |
| surface | `#ffffff` |
| surface-hover | `#efece6` |
| surface-flyout | `#fffffff2` |
| divider | `#ddd8d1` |
| outline | `#d4cec6` |
| outline-control | `#767676` |
| text | `#191b1d` |
| text-secondary | `#5a605c` |
| stone | `#6e675c` |
| primary | `#4a6410` |
| primary-hover | `#3f560d` |
| primary-pressed | `#34470a` |
| primary-subtle | `#4a641024` |
| on-primary | `#ffffff` |
| danger | `#b3261e` |
| focus-ring | `#191b1d` |

Measured contrast (WCAG 2.x). Every text pair passes 4.5:1, and every control border passes 3:1 against the surfaces controls sit on.

| Pair | Dark | Light |
|------|------|-------|
| text on background / surface | 16.1 / 14.6 | 15.5 / 17.3 |
| text-secondary on background / surface | 7.3 / 6.6 | 5.8 / 6.4 |
| primary on background | 12.8 | 6.0 |
| on-primary on primary | 12.8 | 6.7 |
| outline-control on surface | 4.0 | 4.5 |
| stone on background | 8.9 | 5.0 |

## Typography

Segoe UI Variable is the interface face, falling back to Segoe UI, so Cairn reads as part of Windows. Paths and copied text use Cascadia Mono, falling back to Consolas. Never go below 12 px. Tile names use `body-strong` and kinds use `caption` in `text-secondary`.

## Iconography

Segoe Fluent Icons (Windows 11) or Segoe MDL2 Assets (Windows 10), at 16 px in controls and 28 px on text and link tiles. File tiles use the shell's own icon or thumbnail, so files look the way Explorer shows them. Never use emoji.

## Motion

Motion is quick and physical, like setting something down.

- The edge tab grows into the drop zone in 140 ms with `cubic-bezier(0,0,0,1)` and shrinks back in 120 ms.
- On a drop, the zone flashes lichen once (600 ms), and new tiles settle in from 4 px above over 160 ms.
- Hover and press changes take 120 ms.
- When the Windows "Show animations" setting is off, change state with no transition.

## Signature UI

- **Edge tab:** about 6 × 120 px at the screen edge, `outline-control` at 40% opacity, pill corners. It is the only thing Cairn shows while idle.
- **Drop zone:** when a supported drag enters, the tab grows to 88 × 160 px with a `primary-subtle` fill, a 1.5 px dashed `primary` outline, the mark at 32 px and "Drop here" in `caption`.
- **Stack flyout:** a `surface-flyout` card with an 8 px radius and a 1 px `outline`, opened beside the tab. 96 × 88 tiles in a 4-column grid with 8 px gaps. The selected tile gets `primary-subtle` with a 1 px `primary` outline.

## Store listing (draft)

> **Cairn: drop shelf for Windows**
> Set it down. Pick it up anywhere. Drag files, text, images or links to the edge of your screen to park them, then drag them out into any app when you're ready. Cairn is a tiny native app that stays out of the way until you drag. It keeps references to your files and never moves them, stores everything locally and encrypted, and needs no account.

Screenshots: real apps on a plain dark wallpaper, one idea per shot (drop, the stack, drag out, keyboard). Never use fake UI.

## Relationship to Orbit

Cairn and Orbit share code and design principles: Windows-native type, WinUI radii, one accent per view, no gradients. They are separate brands. Orbit's accent is peach (`#ffc799`) and Cairn's is lichen (`#c6e36b`), so the two can run side by side without being confused.
