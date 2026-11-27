# ZenithOS project instructions

Follow all existing instructions in `DEVELOPMENT_RULES.md`, `INTERFACES.md`,
`ARCHITECTURE.md`, and `DECISIONS.md`. This policy supplements them.

## Permanent release codename policy

Official format: **ZenithOS v<version> — <codename>**.
Current development identity: **ZenithOS v0.4.0 — Aurora**.

| Version series | Codename |
|---|---|
| 0.1.x | Genesis |
| 0.2.x | Horizon |
| 0.3.x | Nightfall |
| 0.4.x | Aurora |
| 0.5.x | Nebula |
| 0.6.x | Eclipse |
| 0.7.x | Pulsar |
| 0.8.x | Equinox |
| 0.9.x | Solstice |
| 1.0.x | Ascension |
| 1.1.x | Polaris |
| 1.2.x | Vega |
| 1.3.x | Orion |
| 1.4.x | Andromeda |
| 1.5.x | Supernova |
| 2.0.x | Infinity |

1. Every major/minor series must use its assigned codename. Patch releases
   retain that codename; never invent alternatives for listed series.
2. Request approval for an unassigned series. Never bump the version without
   an explicit request.
3. Keep version and codename synchronized across code, configuration and
   documentation. The centralized version is `workspace.package.version` in
   `Cargo.toml`; `release-codenames.txt` is the machine-readable registry.
   Kernel metadata inherits that version and validates the registry at build.
4. Release codenames are separate from the desktop environment (Zenith UI),
   theme (Afterglow), and wallpaper artwork.
5. During release preparation check source constants, build configuration,
   kernel information, Settings, About, terminal output, boot screens,
   README.md, RELEASE_NOTES.md, STATUS.md, relevant wiki pages, GitHub titles,
   and generated distribution filenames.
6. GitHub titles use the official format. Obtain the title with
   `python3 tools/release_metadata.py title`. Artifact names use lowercase
   codenames, e.g. `zenithos-v0.3.1-nightfall-x86_64.iso`. Keep semantic-version
   Git tags such as `v0.3.1`; never rename existing tags.
7. ASCII-only framebuffer fonts may display `-` instead of the em dash.
   Preserve the canonical Unicode title in metadata and documentation.
8. Do not rename unrelated files, Rust modules or internal APIs. Preserve all
   existing project instructions. Do not create releases, commit or push
   changes without explicit authorization for that action.

Run `cargo build --workspace --locked`, `cargo test --workspace --locked`,
and metadata checks when integrating release identity changes.
