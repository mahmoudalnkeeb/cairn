# Decisions log

| Date | Decision | Status |
|------|----------|--------|
| 2026-11-02 | Working name "Cairn" | Placeholder, pending trademark search |
| 2026-11-02 | Copy Orbit's shelf code instead of sharing a crate | Decided for now |
| 2026-11-02 | Edge tab is an OLE drop target, not a mouse hook | Decided |
| 2026-11-02 | Default hotkey Ctrl+Alt+C | Decided |

## Copy, don't share (yet)

Cairn started from copies of Orbit's `shelf`, `dnd`, `shelfview` (now `stackview`), `gfx`, `store` and `settings` modules. Orbit's repository was left untouched. The PRD's plan for a shared crate (§10, M1) is deferred: the two copies may drift, so fixes to the shelf code must be applied in both places until a shared crate is extracted. Candidates for that crate: `shelf`, `dnd`, `store`, and the non-theme parts of `gfx`.

## Edge tab as a drop target

The tab window keeps a fixed size (120 × 200 DIP). While idle, only a 10 × 120 strip along the edge has non-zero alpha, so only that strip takes clicks and drops; layered-window hit testing passes everything else through. When a supported drag enters the strip, Windows calls `IDropTarget::DragEnter` and the card grows to 88 × 160 under the cursor. Nothing watches the mouse system-wide, so window drags and text selection never trigger the tab.

Trade-off: the strip can cover part of a maximized window's scrollbar on the same edge. Mitigation for now: choose the other edge. Planned: move the tab along the edge, and per-monitor placement (M3).

## Hotkey

Ctrl+Alt+C reads as "Cairn" and was free on the dev machine. Ctrl+Alt+K and Ctrl+Alt+Shift+C are fallbacks. A fallback becomes the saved choice, so the notice shows only once.
