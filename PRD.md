# Cairn: Product Requirements Document

Status: Draft v0.1 · Date: 2026-11-02 · Platform: Windows 10 (19045+) and 11

Cairn is the working name; see Open questions. Brand and visual identity live in [BRAND.md](BRAND.md).

## 1. Summary

Cairn is a drop shelf for Windows. While you drag something, a tab appears at the screen edge. Drop files, text, images or links on it to park them, then drag them out later into any app, all at once or one at a time. It is a small native app, with no account and nothing sent over the network.

It grows out of the shelf tool in [Orbit](../orbit/PRD.md) (F5). Orbit puts the shelf behind a radial menu next to other tools. Cairn is the shelf alone, with the shelf's own way in: drag to the edge.

## 2. Problem

Moving things between apps on Windows assumes both ends are visible at once. In practice:

- The target is often buried: a different virtual desktop, a minimized window, a browser tab, or a folder you haven't opened yet. You have to arrange windows side by side before you can drag at all.
- Collecting things from several places (three files from Downloads, a link, a paragraph of text) and delivering them in one go has no home. People abuse the desktop, a temp folder or the clipboard, which holds only one item.
- Windows has no built-in shelf. macOS users have well-known options (Dropover, Yoink, Dropzone), and the Windows options are small projects with visible gaps (section 5).

Evidence is light so far: AlternativeTo listings, forum threads and maintainers' own pages. Section 11 lists the research still needed.

## 3. Goals and non-goals

Goals
- Park something in one gesture: drag to the edge and drop. Take it out in one gesture.
- Stay invisible until needed: no window, no dock and no bubble in the way while idle.
- Feel native and stay small: under 40 MB RAM idle, under 0.5% CPU idle, cold start under 1 second.
- Work fully offline. No telemetry by default.

Non-goals (v1)
- Not a file manager, sync tool or cloud uploader.
- Not a clipboard manager (Orbit covers that). Cairn takes clipboard content only when you ask it to.
- No macOS or Linux build.

## 4. Users and jobs

Primary: Windows power users and knowledge workers who move material between apps all day. That includes designers handing assets across tools, developers collecting logs and screenshots, writers and researchers gathering sources, and support staff attaching files to tickets.

Jobs to be done
1. "When the place I'm dragging to is hidden, hold this for me while I go find it."
2. "When I'm collecting things from several places, keep them together until I deliver them in one drop."
3. "When I'll need something again in a minute, keep it within reach without saving it somewhere."

## 5. Competitive landscape

| Product | Platform | Model | Notes |
|---------|----------|-------|-------|
| Dropover | macOS | Free with in-app purchases | The reference experience on Mac: shake or hold a key while dragging to summon a shelf near the cursor. |
| Yoink | macOS | Paid, about $9, also on Setapp | Mature, polished shelf at the screen edge. |
| Dropzone | macOS | Free on the Mac App Store | Its shelf (Drop Bar) is one feature among actions. |
| Dropshelf | Windows | Free, Pro about €4 one-time | Shelf slides out when you drag to the screen edge. The free version waits 3 seconds before the shelf appears. One user calls it the closest Windows equivalent to Yoink. |
| DropPoint | Windows, macOS, Linux | Free, open source | Electron app. AlternativeTo lists it as discontinued. |
| Traydex | Windows | Free, open source | Native (Rust, GPUI) shelf with file actions (zip, convert, OCR, upload). Not code-signed, so SmartScreen warns on first run. |
| Stash, Edge Drop, Droper, Tokri | Windows (Tokri cross-platform) | Mostly free | Smaller projects. Stash keeps file references, not copies. |
| WindowSill | Windows | Free with paid tier | A toolbar above the taskbar with drag-and-drop converters, not a shelf. |

Takeaways
- Windows has demand but no clear leader. The incumbents are niche, unsigned, Electron-based or have friction built in (Dropshelf's free-tier delay).
- Edge-summoned shelves are an established pattern, so users won't need teaching.
- Price expectations are low: a one-time purchase in the single digits, or free with a paid tier.

## 6. Positioning and differentiators

For Windows users who move things between apps, Cairn is the drop shelf that feels built into Windows. It appears the moment you drag, holds anything, and costs nothing at idle.

1. Native and tiny. Rust and Win32 with Direct2D: no webview, no runtime, one signed executable. Orbit's release build, which includes this shelf along with three other tools, is about 0.5 MB and idles at about 17 MB RAM.
2. No delay, no global hooks. The edge tab is a real OLE drop target, so Windows itself says when a drag arrives. Nothing watches the mouse system-wide.
3. Keeps references, never moves your files. Cairn always asks the drag source to copy or link, so a file can't be moved by mistake.
4. Keyboard first. One hotkey opens the stack. Arrow keys, Enter, Del and Esc do the rest.
5. Private by default. Text and links are stored encrypted with Windows DPAPI. Nothing leaves the machine.
6. Signed from day one, so it installs without SmartScreen warnings. That is a visible gap in the open-source alternatives.

## 7. Features

### MVP (v0.1)

| ID | Feature | Requirement |
|----|---------|-------------|
| C1 | Edge tab | A slim tab (about 6 by 120 px, nearly transparent) on a chosen screen edge, on every monitor the user enables. It grows into a drop zone when a supported drag enters it. Hidden over fullscreen apps and games. |
| C2 | Drop in | Accepts files and folders (CF_HDROP), links (UniformResourceLocatorW) and plain text. Duplicates are skipped. The drop effect is always Copy or Link, never Move. |
| C3 | The stack | A flyout grid of 96 by 88 tiles with an icon or thumbnail, name and kind (File, Folder, Text, Link, Missing). It opens beside the edge tab after a drop, from the hotkey or from the tray. |
| C4 | Drag out | Drag one tile out, or select several (Ctrl or Shift click) and drag them all, into any app. Files use the shell's own data object, so Explorer, browsers and mail clients all accept them. |
| C5 | Open and paste | Enter or double-click opens a file or link. On a text item it pastes into the app that had focus. |
| C6 | Remove | Del, or the × on hover. "Clear stack" in the tray menu, with an undo notification for 10 seconds. |
| C7 | Persist | Items stay until removed, across restarts. Stored in `%APPDATA%\Cairn`, encrypted with DPAPI. Files missing at their path show as Missing. |
| C8 | Settings | From the tray: hotkey (default Ctrl+Alt+C, with fallbacks when taken), edge (left or right) and monitors, run at login. |

### Later (v0.2+)

| ID | Feature |
|----|---------|
| C9 | Several named stacks (for example "Invoice", "Screens for Sam"), shown as tabs in the flyout |
| C10 | Quick actions on a selection: zip, copy paths, copy as Markdown links, send to a folder |
| C11 | "Add clipboard to stack" hotkey |
| C12 | Shake-to-summon: show the stack near the cursor during a drag, like Dropover. Opt-in, since it needs a mouse hook. |
| C13 | Image items keep a full-size copy as well as the reference, for screenshots that only ever lived in another app |
| C14 | Optional copy-into-Cairn for files on removable or network drives |

## 8. User flows

1. Park and deliver. Drag a file from Explorer to the right edge → the tab grows into a drop zone → drop → the tab pulses once → switch to the browser → press Ctrl+Alt+C → drag the tile into the upload field.
2. Collect then deliver. Drop three files, a link and a quote over ten minutes → open the stack → Ctrl+A → drag all five into an email.
3. Keyboard only. Ctrl+Alt+C → arrows to the text item → Enter pastes it into the focused app.

## 9. Non-functional requirements

- Idle: under 40 MB RAM, under 0.5% CPU, no timers firing more than once per second.
- The flyout opens in under 100 ms. The edge tab reacts to DragEnter in under 50 ms.
- Per-monitor DPI v2. Works across monitors with different DPI.
- Accessible: full keyboard use, visible focus, text at 4.5:1 and control borders at 3:1 in both themes, honors the system animation setting, UI Automation names on tiles.
- Signed installer (MSIX for the Store, plus a signed .exe for winget). No admin rights needed.
- Data: `%APPDATA%\Cairn`, DPAPI-encrypted index. File references are never followed or opened unless the user acts.

## 10. Technical approach

Reuse Orbit's proven code, moved into a shared crate:

- `shelf` model and encrypted storage, `dnd` (IDropTarget, IDropSource and shell data objects), `shelfview` flyout, `gfx` (Direct2D and DirectWrite on layered windows), `store` (DPAPI).
- New: the edge tab window (layered, topmost, tool window) registered as a drop target, which grows on DragEnter. Multi-select and multi-item drag out (SHCreateShellItemArrayFromIDLists, then BHID_DataObject). Tray app shell.
- Orbit then depends on the same crate, so both apps share one shelf.

Detecting drags with an OLE drop target instead of a global mouse hook avoids false triggers from window drags and text selection, and avoids the security-tool scrutiny that hooks attract.

## 11. Business model and distribution

Hypothesis to validate:
- Free: one stack, everything in the MVP. No delays or nag screens. Friction in the free tier is what users resent most about the incumbents.
- Cairn Pro, one-time purchase of about $6 to $8: several stacks, quick actions, shake-to-summon.
- Distribution: Microsoft Store (MSIX), winget and a direct download.

Research needed before pricing: interview 8 to 10 Windows power users, run a landing page with a waitlist, and read the reviews of Dropshelf and the Mac apps for what people pay for.

## 12. Success metrics

- Activation: 60% of installs drop at least one item within 24 hours.
- Habit: 30% of activated users use Cairn on 3 or more days in their second week.
- Quality: zero crashes in a 7-day soak test, and idle resources within section 9 limits.
- For Pro: 3 to 5% of weekly active users upgrade within 60 days.

## 13. Risks

| Risk | Mitigation |
|------|-----------|
| An edge tab blocks scrollbars or edge gestures | Keep it narrow and nearly transparent, let it pass clicks through while idle, and let users move it along the edge or turn it off |
| Drag and drop to or from elevated (admin) windows is blocked by Windows (UIPI) | Detect it and explain in the tab ("Can't accept from an admin window"); document it |
| Missing or moved files confuse users | Show Missing clearly; v0.2 offers to find the file or keep copies (C13, C14) |
| Crowded, low-priced category | Win on native feel, no free-tier friction, and signing |
| Name conflict | Trademark search before any public launch (Open questions) |
| Code-signing cost and setup | Budget for an OV certificate or use Microsoft Store signing |

## 14. Open questions

1. Name: "Cairn" is the working name. A small Windows device-manager app sold on Payhip uses it, so a trademark and store-name search is needed. Backups: Kerb, Tarn, Pebble.
2. Default edge: right (closer to most scrollbars) or left? Test both.
3. Should images be copied into Cairn by default (C13), or stay as references?
4. Pricing and tiers (section 11).
5. Open source the core crate, as a shared base for Orbit and Cairn?

## 15. Milestones

1. M1: shared crate extracted from Orbit, plus the edge tab drop target and tray shell.
2. M2: stack flyout with multi-select and multi-item drag out.
3. M3: settings, multi-monitor, accessibility pass, signed installer. Private beta.
4. M4: Store and winget release (v0.1).
5. M5: several stacks and quick actions (v0.2, Pro).

## Sources

- [Dropover on the App Store](https://apps.apple.com/app/id1355679052)
- [Yoink on the App Store](https://apps.apple.com/us/app/yoink-improved-drag-and-drop/id457622435)
- [Dropzone 4 review, Macworld](https://www.macworld.com/article/3531311/dropzone-4-review.html)
- [Dropshelf (Windows) on AlternativeTo](https://alternativeto.net/software/dropshelf-windows-)
- [Dropshelf write-up, TechPP](https://techpp.com/2025/04/08/dropshelf-drag-and-drop-on-windows/)
- [DropPoint on AlternativeTo](https://alternativeto.net/software/droppoint) and [GitHub](https://www.github.com/GameGodS3/DropPoint)
- [Traydex on GitHub](https://github.com/ipavlin98/traydex/wiki) and [AlternativeTo](https://alternativeto.net/software/traydex/about/)
- [Stash on Product Hunt](https://www.producthunt.com/products/stash-20/makers)
- [WindowSill coverage, Neowin](https://www.neowin.net/news/this-windows-11-app-is-a-fantastic-addition-to-the-stock-taskbar/)
- [Cairn device manager on Payhip](https://payhip.com/b/m8ZsW) (name conflict)
