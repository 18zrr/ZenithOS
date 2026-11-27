# ZenithOS v0.4.0 — Aurora terminal manual

Open Terminal from Start, the dock or the desktop context menu. Commands run
at `zenith>`. Up/Down recalls history; normal line editing is supported.

| Command | Operation |
|---|---|
| `help` | List implemented commands |
| `clear` | Clear the terminal |
| `version` | OS version, codename and boot architecture |
| `uptime` | Time since boot |
| `date` | Firmware date/time, or RTC unavailable |
| `mem`, `meminfo` | Available kernel memory information |
| `sysinfo` | System overview |
| `tasks` | Scheduled kernel task IDs and cumulative TSC ticks; current heap occupancy in bytes |
| `pwd` | Print `/` (the shell has no change-directory command) |
| `ls [path]` | List a Zenith FS directory, default `/` |
| `mkdir path` | Create a directory; its parent must exist |
| `touch path` | Create an empty file; existing files retain their data |
| `cat path` | Print up to 64 KiB, replacing unsupported bytes with `.` |
| `history` | Recent commands |
| `pci`, `gpu`, `acpi` | Hardware diagnostics |
| `net` | Network status |
| `browser` | Open the Chromium companion |
| `install` | Open the installation wizard |
| `about` | Print OS information |
| `echo text` | Print text |
| `display` | Show display information; accepts a supported WIDTHxHEIGHT |
| `flip` | Change display orientation |
| `pointer` | Report mouse position |

Paths are relative to `/` unless prefixed with `/`. The entire remainder of
the command is the path; no shell quoting, pipelines or expansion is provided.
Ctrl+Shift+C copies the current input line. Ctrl+V inserts one clipboard line
only if it fits; press Enter yourself to execute it.
Filesystem errors are printed rather than simulated. Commands require a
mounted Zenith FS volume. Time is firmware time without timezone conversion.

Example:

```text
mkdir documents
touch documents/notes
ls documents
cat documents/notes
```

Use File Manager's editor to write the file. See the
[desktop guide](desktop_manual.md) for Save As and close confirmation.
