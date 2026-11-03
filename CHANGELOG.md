# Changelog

All notable changes to Cairn are listed here. Versions follow [Semantic Versioning](https://semver.org/).

## [0.4.1] - 2026-11-03

### Added

- Hold a drag on the edge tab for half a second and the stack opens beside it. Point at a stack's name to switch to it, then let go anywhere over the stack. The stack closes by itself afterwards. A quick drop on the tab still adds to the current stack.

### Changed

- The whole stack lights up when something is dragged over it, in place of the dashed "Drop here" tile.
- An empty stack says how to add things.

## [0.4.0] - 2026-11-03

### Added

- Images with no file behind them, such as a screenshot or an image copied from an app, can be dropped or added with Ctrl+Alt+V. Cairn saves its own copy.
- "Keep copies of files from USB and network drives" in the tray menu. With it on, files dropped from those drives are copied into Cairn in the background, so they are still there after the drive is gone.
- "Shake while dragging to show a drop zone" in the tray menu. Shake the mouse left and right while dragging and a drop zone opens next to the cursor. Off by default.

### Changed

- Cairn's own copies live in `%APPDATA%\Cairn\files` and are deleted once no stack holds them.

## [0.3.0] - 2026-11-03

### Added

- Named stacks, shown as tabs at the top of the stack. Add one with + or Ctrl+N, switch with a click or Ctrl+Tab, rename with a double-click or F2, and delete from the right-click menu. Deleted items can be brought back for 10 seconds.
- Right-click items (or press the Menu key) to copy their paths, copy them as Markdown links, save them as a zip, copy them to a folder, or move them to another stack.
- Ctrl+Alt+V, or "Add clipboard to stack" in the tray menu, adds the files, link or text on the clipboard.
- Screen readers announce the stack's name.

### Changed

- Stacks saved by 0.1 and 0.2 open as one stack called "Stack".

## [0.2.0] - 2026-11-03

### Added

- The edge tab appears on every monitor. Choose which monitors show it from "Show tab on" in the tray menu.
- Drag the tab up or down the edge to move it. "Center the tab" under "Screen edge" puts it back.
- Screen readers such as Narrator and NVDA read the stack: each item's name, kind, selection and focus, and the file path or link. Items can be opened from the screen reader.

### Changed

- A fullscreen game, video or presentation hides the tab only on its own monitor.
- The stack opens beside the tab on the monitor you are using.

## [0.1.0] - 2026-11-03

The first public version.

### Added

- Edge tab: drag files, folders, text or links to the edge of the screen to keep them. The tab hides while a fullscreen app, game or presentation is in front.
- Stack: open it from the edge tab, the tray icon or <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>C</kbd>. Items show their icon or thumbnail, name and kind, and missing files are marked.
- Drag out into any app. Select several items with Ctrl or Shift click, Shift and the arrow keys, or Ctrl+A, and drag them out together.
- Double-click or Enter opens a file or link; on a text item it pastes into the app you came from.
- Remove items with Del or the × on a tile. "Clear stack" in the tray menu can be undone for 10 seconds.
- The stack is kept across restarts, stored encrypted on your PC.
- Tray settings: shortcut, screen edge (left or right) and run at login.

### Known limits

- Main monitor only.
- Windows blocks dragging into or out of apps running as administrator.
- Not signed yet, so Windows SmartScreen may warn on first run.

[0.4.1]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.4.1
[0.4.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.4.0
[0.3.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.3.0
[0.2.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.2.0
[0.1.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.1.0
