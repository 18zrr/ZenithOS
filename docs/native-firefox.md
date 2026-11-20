# Native Firefox bring-up

**Native command-line startup works; native browsing does not yet.** The
experimental `native-run` kernel has run unmodified Firefox 160.0.1 in user
mode, loaded Gecko and printed `Mozilla Firefox 160.0.1` with exit status 0.
To reproduce on Linux x86-64 with the matching Firefox, compiler, dynamic
linker and system libraries installed:

```sh
python3 tools/test_native_run.py
```

This builds temporary test ISOs and uses 2 GiB QEMU guests with no NIC or
host browser. The bring-up path is opt-in; `./build_iso.sh` builds the normal
desktop kernel. The separate image-loader check below only tests page staging.

## Reproduce the image-loader check

With the normal BIOS ISO build dependencies, QEMU and an unpacked Firefox
package on the build machine, run:

```sh
python3 tools/test_firefox_elf.py /usr/lib/firefox
```

The directory must contain the `firefox` ELF executable (not `/usr/bin`'s
wrapper script) and `libxul.so`. The test builds a temporary ISO containing
both as Limine modules and boots Zenith with 1 GiB RAM and no network. Only
the test configuration requests the `elfcheck` boot diagnostic. The normal ISO
does not bundle or distribute Mozilla binaries.

`zenith-elf` validates ELF64 load headers and requested permissions, reads
the interpreter name, copies file-backed bytes, and clears BSS and partial
pages. The diagnostic stages **one 4 KiB page at a time in a scratch buffer**.
The test independently checks page counts and byte sums and waits for the
normal shell prompt after the check. No process mappings are installed.

## What blocks browsing

The tested Firefox 160.0.1 executable requests `/lib64/ld-linux-x86-64.so.2`.
Its Gecko library alone spans approximately 177 MiB of load pages. Copying
both files into the boot image does not supply a Linux-compatible runtime.

A headless local-page trial reached Firefox's “You are running in headless
mode” message, reported an unimplemented resource-limit call, attempted
`socketpair`, then faulted inside `libxul.so`. No rendered page was verified.
The runner has a read-only boot archive, a small thread group and pipes.
It still needs more IPC and filesystem support, writable profiles, networking,
and a display/input backend for a native desktop browser.
