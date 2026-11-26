# Release identity

Current identity: **ZenithOS v0.3.2 — Nightfall**.

The OS version is `workspace.package.version` in the root `Cargo.toml`.
`kernel/Cargo.toml` inherits it. The official major/minor codename registry is
`release-codenames.txt`; patch releases never select a different codename.
Other crates have independent component versions, not OS release versions.

`kernel/build.rs` validates the version series and generates the kernel's
release labels. An unassigned series fails the build: ask for approval rather
than inventing a codename. The current framebuffer fonts support ASCII, so
screens use a hyphen in place of the canonical title's em dash.

`python3 tools/release_metadata.py title` prints the official GitHub release
title. The `iso`, `bios` and `uefi` fields print canonical artifact filenames:

- `zenithos-v0.3.2-nightfall-x86_64.iso`
- `zenithos-v0.3.2-nightfall-x86_64-bios.img`
- `zenithos-v0.3.2-nightfall-x86_64-uefi.img`

Image builders retain the short compatibility copies for existing launchers.
Git tags remain semantic versions, e.g. `v0.3.1`.

Zenith UI is the desktop environment, Afterglow is a theme, and Nightfall is
the 0.3.x release codename. The existing Nightfall landscape is artwork; it
need not be renamed when the OS enters a new series.

Follow the complete policy and integration checklist in
[AGENTS.md](../../AGENTS.md) and
[DEVELOPMENT_RULES.md](../../DEVELOPMENT_RULES.md).
Do not bump versions, create a release, commit, or push without authorization.
