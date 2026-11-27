# ZenithOS v0.4.0 — Aurora desktop guide

**Zenith Settings:** choose a page in the sidebar (tabs on narrow windows), or
use Tab/Shift+Tab. On System, click the interval button or press Enter to
cycle Monitor's sampling interval. Settings persist on writable Zenith FS.
Display changes require a supported guest adapter.

**Zenith Monitor:** CPU activity is the share of elapsed scheduler time spent
outside the idle task on the boot CPU. The second graph shows usage of the
128 MiB kernel heap, not total physical RAM. Up to 60 samples are shown;
closing/minimizing resets the visible history. Scroll for storage, boot and
input details. Use `tasks` in Terminal for the raw task counters.
Press S to sort by CPU share or ID. Ctrl+F filters names; Enter keeps
the filter and Esc clears it. Disk counters are successful 512-byte PIO
sector transfers since boot, including metadata I/O.

**Window snapping:** Ctrl+Alt+Left/Right fits the active window to half the
work area. Ctrl+Alt+Down or the titlebar restore control returns it to its
previous bounds.

**Control Center:** press F3 or click the dock clock. Open Settings or Display,
use session power actions, or inspect the last eight notifications. Dismiss
latest or clear history. Volume control is unavailable. Logout clears both
notification history and the clipboard.

Right-click empty desktop space for Terminal, File Manager, Settings, About,
or Refresh. Use Up/Down and Enter, or click an item; Esc dismisses the menu.
F1 opens Launchpad. On short displays, wheel over the application tiles to
reveal the remaining rows; the search box and session buttons stay in place.
Arrow keys also bring the selected tile into view. Hover dock icons for labels. The clock and date use firmware
time; `--:--` indicates an unavailable RTC.

Use titlebar controls to minimize, maximize/restore or close windows. Drag
the titlebar to move a window; drag its resize handle to resize. Clicking a
running dock icon focuses/restores it, or minimizes the active window.
Alt+Tab / Alt+Shift+Tab cycle applications. About shows release metadata,
boot mode, memory and available display/pointer information.

## Zenith Explorer

The launcher still accepts `file`. Ctrl+T opens a directory tab, Ctrl+Tab /
Ctrl+Shift+Tab switches, and Ctrl+W closes one (up to four). Click a breadcrumb
or use Alt+Left/Right for per-tab back/forward history. Wide browse windows
show a Locations sidebar. Ctrl+F filters the current folder by name on Enter;
Esc clears the filter. Click the Name header to reverse sorting. Grid/List or
Ctrl+G changes the view.

Space marks or unmarks entries for transfer. Ctrl+C copies the marked files
(or the current file); Ctrl+X prepares files or directories for moving.
Navigate to another directory and press Ctrl+V. Copy accepts files up to
256 KiB; moving preserves the inode, including directory contents. Each item
commits atomically. A collision stops the batch without overwriting; the
status reports the completed count and remaining items can be retried.
Delete and Rename act on the **current** row only.

Wheel over the list or grid to browse without changing the selected file.
Use arrows, Home/End and Page Up/Down to move the selection. Single-click
selects; double-click, Enter and **Open** all activate the selected entry.
Folders open in the current tab. The built-in editor opens extensionless
notes and `.txt`, `.md`, `.log`, `.cfg`, `.conf`, `.ini` and `.csv` as plain
text (case-insensitive). It checks the contents before opening. Other
extensions show a status message. Backspace goes up; F5 refreshes.

- N: new file; D: new folder.
- F2: rename (prefilled); Ctrl+A clears the name field.
- C: copy a file to a new name in the same directory.
- Delete: confirmation; Enter deletes, Esc cancels.
- P: properties (type, bytes, mode and owner/group); Enter or Esc returns.
- V: preview the size and first line without opening the editor.
- E: explicitly edit an ASCII text file, regardless of extension.

Names must be single non-empty components; existing destinations are never
overwritten by Copy or Save As. Errors are shown in the status area.

## Editor

A `*` marks unsaved changes. Arrows/Home/End move the cursor; hold Shift to
select, or Ctrl+A to select all. Typing replaces the selection. Wheel
scrolls the document without moving the insertion point or changing the file.

Ctrl+C / Ctrl+X copies or cuts selected text. Ctrl+V pastes if the result
fits the document. Terminal shares this clipboard: Ctrl+Shift+C copies
the current command line; Ctrl+V inserts the first line without executing it.

Ctrl+S saves and keeps editing. Ctrl+Shift+S opens Save As: enter a new name
in the current folder, then Enter. Esc returns to the editor.

Esc or the window close control asks before losing unsaved changes:
**S** saves and closes, **D** discards, **Esc** cancels. A failed save retains
the draft. Desktop session actions also stop for an unsaved draft; resolve it
then retry the session action.

Documents are limited to 4 KiB of printable ASCII and LF newlines. There is no
Unicode editor or undo history. File copy is limited to 256 KiB.
