# Zenith x86_64 application ABI v1 (experimental)

[`zenith.h`](../userland/include/zenith.h) is the freestanding C interface;
[`system_info.c`](../userland/examples/system_info.c) is a complete example.
It reads kernel statistics and the release title, writes to the console,
posts a notification and exits.

## Startup and exit

Build with `--features native-run`. Supply a Limine entry with the `zenapp`
command-line argument and a static x86_64 `ET_EXEC` ELF as its first module.
This selects the Zenith dispatcher, separate from the `runelf` compatibility
dispatcher. No dynamic linker, libc, argv or environment is supplied. `_start`
receives an empty SysV-aligned stack and must call `zen_exit` rather than
return. Compile without PIE or the red zone.

`python3 tools/test_zenith_app.py` builds and boots the example, then boots a
second app which explicitly aborts. Both runs must reach the normal shell.
Requirements: the BIOS ISO dependencies, `cc`, QEMU and 2 GiB guest RAM.
Set `LIMINE_DIR` for a nonstandard installation. Rebuild the regular image
after testing.

## Calls

`syscall` on the boot CPU: RAX = operation; RDI and RSI = arguments. RAX holds
the signed result; RCX and R11 are clobbered. Errors: `-14` invalid buffer,
`-22` invalid argument, `-28` insufficient capacity, `-38` unknown operation.

| RAX | Wrapper | Result |
|---:|---|---|
| 0 | `zen_abi_version()` | `1` |
| 1 | `zen_exit(code)` | Terminates with an 8-bit status; never returns |
| 2 | `zen_write(ptr, len)` | At most 4096 console bytes; returns count |
| 3 | `zen_release(ptr, capacity)` | UTF-8 release title into writable user pages; returns bytes, no NUL |
| 4 | `zen_uptime_ticks()` | PIT ticks since boot (~100 Hz) |
| 5 | `zen_heap_bytes()` | Occupied **kernel** heap bytes, not app RSS |
| 6 | `zen_notify(ptr, len)` | 1–96 printable ASCII bytes to Control Center; returns 0 |

The loader validates segments and maps executable and writable pages
separately. The dispatcher validates the full user range against page
permissions before copying. An unhandled user exception terminates the
one-shot process; its status is reported on COM1. Boot then continues.

**Boundary:** one boot-time app in a dedicated user address space. Desktop
windows still run inside the privileged compositor. Interactive ELF launching,
simultaneous address spaces, a window/event protocol and general IPC are not
part of this ABI. No filesystem mutation or process-kill syscall is exposed.
