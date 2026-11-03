<p align="center">
  <img src="brand/cairn-app-icon.svg" width="96" height="96" alt="Cairn icon">
</p>

<h1 align="center">Cairn</h1>

<p align="center">A drop shelf for Windows. Park files, text and links at the edge of your screen, then drag them into any app.</p>

<p align="center">
  <a href="https://github.com/mahmoudalnkeeb/cairn/releases/latest"><img src="https://img.shields.io/github/v/release/mahmoudalnkeeb/cairn" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/mahmoudalnkeeb/cairn" alt="License: GPL-3.0"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-informational" alt="Platform: Windows 10 and 11">
</p>

<p align="center">
  <a href="https://mahmoudalnkeeb.github.io/cairn/">Website</a> ·
  <a href="https://github.com/mahmoudalnkeeb/cairn/releases/latest">Download</a> ·
  <a href="CHANGELOG.md">Changelog</a>
</p>

## Features

- Drag files, folders, text or links to the edge of the screen to keep them in your stack.
- Open the stack from the edge, the tray icon or <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd>, and drag items into any app that accepts drops: Explorer, browsers, email, chat.
- Select several items and drag them out in one go, even files from different folders.
- Keep separate stacks for separate jobs, shown as tabs at the top of the stack.
- A link dragged from a browser shows its page title.
- Right-click items to copy their paths, copy them as Markdown links, save them as a zip, copy them to a folder or move them to another stack.
- Press <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>V</kbd> to add whatever is on the clipboard.
- Open a file or link with a double-click. A text item pastes into the app you came from.
- Text from a web page, Word or an email keeps its formatting (shown as "Rich text"). Apps that only take plain text get plain text, and "Copy as plain text" is on the right-click menu.
- Your stack stays until you remove it, including after a restart.
- A file moved to another folder on the same drive is found again when you open it. If Cairn can't find it, the tile says Missing: right-click it and choose "Find file…".
- Cairn keeps references to your files and never moves them. It keeps its own copy only of things that had no file of their own, like a screenshot, an email attachment or a picture from a web page, and, if you turn it on, of files from USB sticks and network drives.
- Drop email attachments, or files and folders straight out of a zip. Cairn saves its own copy, since they have no place on disk yet.
- Shake the mouse while dragging to get a drop zone right next to the cursor (turn it on in the tray menu).
- An edge tab on every monitor, or only the ones you pick. Drag the tab up or down to where you want it.
- The edge tab hides on the monitor where a game, fullscreen video or presentation is in front.
- Works with screen readers such as Narrator and NVDA: each item is read with its name, kind and state.

## Install

1. Download `Cairn.exe` from the [latest release](https://github.com/mahmoudalnkeeb/cairn/releases/latest).
2. Run it. There is no installer and no account.

Cairn is not code-signed yet, so Windows SmartScreen may show "Windows protected your PC". Choose "More info", then "Run anyway".

To start Cairn with Windows, right-click the tray icon and turn on "Run at login".

## Usage

Drag something to the right edge of the screen. A drop zone appears; let go and the item is in your stack. Hold it on the drop zone for a moment instead and the stack opens, so you can point at a stack's name and drop it there.

| Action | How |
|---|---|
| Open or close the stack | Click the edge tab, left-click the tray icon, or press <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd> |
| Select | Click, <kbd>Ctrl</kbd>+click, <kbd>Shift</kbd>+click, arrow keys, <kbd>Ctrl</kbd>+<kbd>A</kbd> |
| Use items | Drag them out of the stack |
| Open or paste | Double-click or <kbd>Enter</kbd> |
| More actions | Right-click, or the <kbd>Menu</kbd> key |
| Add the clipboard | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>V</kbd>, or "Add clipboard to stack" in the tray menu |
| Remove | <kbd>Del</kbd>, or the × on a tile |
| Clear the stack | Tray menu, then "Clear stack" (undo within 10 seconds) |
| New stack | The + next to the stack names, or <kbd>Ctrl</kbd>+<kbd>N</kbd> |
| Switch stacks | Click a name, or <kbd>Ctrl</kbd>+<kbd>Tab</kbd> |
| Drop into a particular stack | Hold the drag on the edge tab until the stack opens, point at the stack's name, and let go over the stack |
| Rename or delete a stack | Double-click its name or press <kbd>F2</kbd> to rename; right-click it for both |
| Close the stack | <kbd>Esc</kbd> |
| Move the tab | Drag it up or down the edge |
| Drop without going to the edge | Turn on "Shake while dragging to show a drop zone", then shake the mouse left and right while dragging |

Settings are in the tray menu: the shortcut (with two alternatives if another app already uses <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd>), the screen edge, which monitors show the tab, shake to show a drop zone, keeping copies of files from USB and network drives, and run at login. "Screen edge" also has "Center the tab".

Shake to show is off by default. To notice a shake, Cairn watches mouse movement while it is on; it never changes or blocks your input.

## Your data

Everything stays on your PC. The stack is saved to `%APPDATA%\Cairn`, encrypted for your Windows account. Cairn's own copies of images, attachments, files from inside zips and drive files are kept in `%APPDATA%\Cairn\files` and deleted once no stack holds them. Cairn makes no network requests and has no account or telemetry.

To remove Cairn, quit it from the tray menu, delete `Cairn.exe`, and delete the `%APPDATA%\Cairn` folder.

## Known limitations

- The idle tab can sit over part of a maximized window's scrollbar. Drag the tab along the edge or switch edges to avoid it.
- Windows blocks dragging into or out of apps running as administrator.

## Roadmap

Planned next:

- [x] Edge tab on every monitor, with a choice of which ones (0.2)
- [x] Move the tab up or down along the edge (0.2)
- [x] Screen reader names for tiles (0.2)
- [ ] Signed installer and a signed download
- [ ] Microsoft Store and winget

Ideas for later:

- [x] Several named stacks, shown as tabs (0.3)
- [x] Actions on a selection: zip, copy paths, copy as Markdown links, send to a folder (0.3)
- [x] A shortcut that adds the clipboard to the stack (0.3)
- [x] Show a drop zone near the cursor while dragging (opt-in) (0.4)
- [x] Keep a full copy of image items, for screenshots that only existed in another app (0.4)
- [x] Optionally copy files from removable or network drives into Cairn (0.4)

## Building from source

You need Windows 10 or 11 and a stable [Rust](https://rustup.rs/) toolchain.

```
git clone https://github.com/mahmoudalnkeeb/cairn
cd cairn
cargo build --release
```

The app is written to `target\release\cairn.exe`. `cargo run` starts a debug build with a console for logs, and `cargo test` runs the tests.

## Contributing

Bug reports and feature requests are welcome in [Issues](https://github.com/mahmoudalnkeeb/cairn/issues). For a larger change, open an issue to discuss it before sending a pull request.

## Support

Cairn is free. If you would like to support it, you can donate on [Ko-fi](https://ko-fi.com/mahmoudalnakeeb). Donations pay for a code-signing certificate and time for fixes and new features.

## License

Cairn is licensed under the [GNU General Public License v3.0](LICENSE).
