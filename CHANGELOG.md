# Changelog

All notable changes to Cairn are listed here. Versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Email attachments, and files or folders dragged straight out of a zip, can be dropped or added with Ctrl+Alt+V. They have no place on disk, so Cairn saves its own copy, keeping their names and dates.
- A picture dragged or copied from a web page is kept as a picture, not as its address, so it still works after the page is closed.
- A file moved to another folder on the same drive is found again when you open it from the stack.
- "Find file…" on a missing file's right-click menu, to point the item at where the file is now.

### Changed

- A missing file's icon and name are dimmed, so it stands out from files that are still there.
- The stack is saved in a new format. Earlier versions of Cairn can't read it, so going back to 0.5 starts with an empty stack.

## [0.5.1] - 2026-11-03

### Fixed

- Fewer antivirus false alarms. A few scanners flagged 0.5.0 by guesswork, with no named threat. Cairn.exe now names itself in its file details, states that it needs no admin rights, and is built by GitHub Actions with Microsoft's toolchain.

### Changed

- "Save as zip" writes the zip itself instead of running Windows' tar.exe in the background. Folders keep everything inside them, and files keep their dates.

## [0.5.0] - 2026-11-03

### Added

- Dragging items out of the stack shows them under the cursor: the tile you picked up, the rest of the selection stacked behind it, and a count when there are several.
- While you drag, the items you picked up fade in the stack, leaving their outline behind. They come back when the drag ends.
- A drag over the edge tab, the stack or the drop zone keeps showing the dragged picture, with "Add to" and the stack's name under it. Pointing at another stack's name updates the label.

### Changed

- The stack fades in and slides out from the edge tab when it opens, and fades away when it closes.
- The stack's drop highlight fades in and out instead of switching on and off.
- After a drop, the edge tab and the stack flash lichen once, and new tiles settle into place from just above.
- With "Show animations in Windows" off, all of this happens without transitions.

## [0.4.2] - 2026-11-03

### Changed

- A new Cairn mark. The stones now look like stones, and the top one is tilted, as if just set down. It shows on the drop zone, the app icon and the website.

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

[0.5.1]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.5.1
[0.5.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.5.0
[0.4.2]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.4.2
[0.4.1]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.4.1
[0.4.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.4.0
[0.3.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.3.0
[0.2.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.2.0
[0.1.0]: https://github.com/mahmoudalnkeeb/cairn/releases/tag/v0.1.0
