# ZenithOS v0.3.2 — Nightfall desktop guide

Right-click empty desktop space for Terminal, File Manager, Settings, About,
or Refresh. Use Up/Down and Enter, or click an item; Esc dismisses the menu.
F1 opens Start. Hover dock icons for labels. The clock and date use firmware
time; `--:--` indicates an unavailable RTC.

Use titlebar controls to minimize, maximize/restore or close windows. Drag
the titlebar to move a window; drag its resize handle to resize. Clicking a
running dock icon focuses/restores it, or minimizes the active window.
Alt+Tab / Alt+Shift+Tab cycle applications. About shows release metadata,
boot mode, memory and available display/pointer information.

## File Manager

Use arrows, Home/End and Page Up/Down to select entries. Enter opens a folder
or previews a file; Backspace goes up. F5 refreshes and retains the selected
name when it still exists.

- N: new file; D: new folder.
- F2: rename (prefilled); Ctrl+A clears the name field.
- C: copy a file to a new name in the same directory.
- Delete: confirmation; Enter deletes, Esc cancels.
- P: properties (type, bytes, mode and owner/group); Enter or Esc returns.
- E: edit an ASCII text file.

Names must be single non-empty components; existing destinations are never
overwritten by Copy or Save As. Errors are shown in the status area.

## Editor

A `*` marks unsaved changes. Arrows/Home/End move the cursor; hold Shift to
select, or Ctrl+A to select all. Typing replaces the selection.

Ctrl+S saves and keeps editing. Ctrl+Shift+S opens Save As: enter a new name
in the current folder, then Enter. Esc returns to the editor.

Esc or the window close control asks before losing unsaved changes:
**S** saves and closes, **D** discards, **Esc** cancels. A failed save retains
the draft. Desktop session actions also stop for an unsaved draft; resolve it
then retry the session action.

Documents are limited to 4 KiB of printable ASCII and LF newlines. No Unicode,
clipboard or undo support is implied. File copy is limited to 256 KiB.
