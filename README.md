# Cairn

A drop shelf for Windows: drag files, text, images or links to the edge of the screen to park them, then drag them out into any app.

Status: M2 built (multi-select, multi-item drag out, clear with undo) on top of M1 (edge tab, stack flyout, tray shell). Cairn is the working name, pending a trademark search.

- [PRD.md](PRD.md): product requirements, competition, scope and milestones
- [BRAND.md](BRAND.md): name, voice, logo, color, type and motion
- [brand/](brand/): logo and app icon (SVG)
- [docs/decisions.md](docs/decisions.md): decisions so far
- [site/](site/): the landing page, deployed to GitHub Pages by `.github/workflows/pages.yml`

## Build and run

Needs a stable Rust toolchain. The GNU toolchain works; see Orbit's `docs/decisions.md` for why `windows` is pinned to 0.58.

```
cargo run              # debug build, with a console for logs
cargo build --release  # target/release/cairn.exe, no console
cargo test             # stack, selection, drag out, storage, settings and layout tests
```

Only one instance runs at a time. Quit from the tray icon.

## What it does

- Edge tab (C1): a slim pill on the right edge of the primary monitor, vertically centered. Drag anything to that edge and the pill grows into a "Drop here" zone. Drop and it shows "Added 1" (or "Already here"), then folds back. It hides while a fullscreen app or game is in front.
- Drop in (C2): files and folders, links and plain text. Duplicates are skipped. Cairn always asks the source to copy, never to move, so your files stay where they are.
- The stack (C3): click the tab, press Ctrl+Alt+C, or left-click the tray icon. Tiles show the shell icon or thumbnail, the name and the kind (File, Folder, Text, Link, Missing). You can also drop onto the dashed tile.
- Select (C4): click a tile to select it, Ctrl+click to add or drop one, Shift+click or Shift+arrows for a range, Ctrl+A for all.
- Drag out (C4): drag a tile into any app. Dragging a selected tile takes the whole selection. Files arrive as real file drops, several at once even from different folders; text and links as text. If a selection mixes files with text, only the files go (Missing files are skipped). Several texts or links go as one text, one per line.
- Open and paste (C5): double-click or Enter opens a file or link. On a text item it pastes into the app you came from.
- Remove (C6): Del removes the selection, the × on hover removes one tile. "Clear stack" in the tray menu empties it; click the notification or choose "Undo clear" within 10 seconds to bring everything back.
- Persist (C7): the stack survives restarts, encrypted with DPAPI in `%APPDATA%\Cairn\stack.bin`.
- Settings (C8): the tray menu has Hotkey (Ctrl+Alt+C, Ctrl+Alt+K or Ctrl+Alt+Shift+C, with automatic fallback), Screen edge (left or right) and Run at login. Choices are saved in `%APPDATA%\Cairn\settings.txt`.

Measured on the release build: 331 KB executable, about 18.5 MB RAM at idle, no CPU use over 10 idle seconds.

## Known limits

- Primary monitor only. Choosing monitors comes in M3.
- While idle, the tab catches clicks in a 10 × 120 px strip at the edge, which can cover part of a maximized window's scrollbar. Moving the tab to the other edge avoids it. Moving it along the edge is not built yet.
- Windows blocks dragging into or out of apps running as administrator.
- Not signed and no installer yet (M3).

## Folder layout

```
cairn/
  PRD.md, BRAND.md, brand/   product and brand
  site/                      landing page (GitHub Pages)
  docs/decisions.md
  src/
    main.rs       app state, tray menu, hotkey, fullscreen detection, drop routing
    edgetab.rs    C1 edge tab: idle pill, drop zone, animation
    stackview.rs  C3 to C6 stack flyout
    select.rs     C4 selection: click, Ctrl click, Shift range
    shelf.rs      stack model, encrypted storage, shell icons
    dnd.rs        OLE drag and drop: drop targets and drag source
    clipboard.rs  paste a text item into the previous app
    settings.rs   hotkey, screen edge, run at login
    gfx.rs        Direct2D/DirectWrite on layered windows, brand tokens, the mark
    store.rs      %APPDATA%\Cairn and DPAPI helpers
```
