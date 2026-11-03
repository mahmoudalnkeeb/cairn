# Changelog

All notable changes to Cairn are listed here. Versions follow [Semantic Versioning](https://semver.org/).

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

[0.2.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.2.0
[0.1.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.1.0
