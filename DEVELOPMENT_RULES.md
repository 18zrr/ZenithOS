# Development Rules for Zenith OS

## Coding Standards

### Language and Environment  
- Primary language: Rust (edition 2021)
- Architecture target: x86_64
- Operating system: Custom monolithic kernel design

### Code Quality Requirements
1. **No_std compliance**: All kernel code must compile without standard library 
2. **Memory safety**: Use Rust's ownership and borrowing rules for memory safety  
3. **Performance focus**: Optimize for performance while maintaining correctness
4. **Minimal dependencies**: Only use crates that are absolutely necessary

### Documentation Requirements  
1. Every public function and struct should have documentation comments  
2. Complex logic must be clearly explained in code comments 
3. Architecture decisions should be recorded in DECISIONS.md
4. All major changes should include updated documentation

## Project Structure Rules

### Directory Organization
- Use the standard monorepo structure as defined in specification
- Each component directory should contain its own Cargo.toml and source files  
- Separation between kernel, drivers, filesystems, userland, etc.
- Keep all components modular and loosely coupled through well-defined interfaces

### Version Control Practices
1. Follow semantic versioning for releases (initially 0.1.0)
2. Use feature branches for major development work 
3. Merge only after code review and testing
4. Commit messages should be descriptive and follow conventional commit format

## Build System Requirements

### Dependencies  
- Rust toolchain with target `x86_64-unknown-none`
- Limine bootloader (UEFI compatible)  
- QEMU for development and testing
- Cargo for dependency management

### Build Process
1. Always test builds in virtual environment first
2. Never modify host system files without explicit approval
3. Use Makefile for consistent build and test workflows
4. Verify kernel boot sequence works before implementing features

## Testing Requirements  

### Development Environment 
1. Use QEMU for initial development testing  
2. Test on actual hardware only after reliable virtual testing
3. Automated tests should cover critical system components 

### Verification Criteria
- Kernel must successfully boot in QEMU
- Basic diagnostic messages must appear during startup  
- System should not crash during initialization sequence

## Security and Safety Guidelines

1. Never assume hardware behavior or memory layout without explicit verification
2. All kernel code must be carefully reviewed for safety issues
3. Follow the principle of least privilege for system components  
4. Implement proper error handling in all subsystems

## Collaboration Rules

### Team Communication
- Document architectural decisions clearly in DECISIONS.md
- Use pull requests and code reviews for major changes
- Maintain consistent naming conventions across files  

### Development Process
1. Focus on completing foundational functionality before adding features 
2. Follow the roadmap progression outlined in ROADMAP.md  
3. Avoid implementing unapproved or unsupported features until ready

These rules ensure consistent development practices while maintaining the quality and safety standards required for operating system development.

## Permanent release identity requirements

The complete official codename table and release checklist in `AGENTS.md`
are mandatory for all future releases and supplement every rule above.
Use `ZenithOS v<version> — <codename>` (currently
**ZenithOS v0.4.0 — Aurora** in development). All patches retain their major/minor series
codename. Never substitute names for an assigned series; request approval
before naming an unassigned series. Never change versions without an explicit
request.

Keep the OS version centralized in `Cargo.toml` (`workspace.package.version`)
and codenames in `release-codenames.txt`. Runtime and build metadata must derive
from these sources wherever feasible. Keep code, build configuration, kernel,
Settings/About, terminal, boot screens, README, release notes, status, relevant
wiki documentation, GitHub titles and artifact names synchronized.

Use lowercase codenames in distribution filenames, for example
`zenithos-v0.3.1-nightfall-x86_64.iso`, and semantic-version tags (`v0.3.1`).
Release codenames are not desktop environment or theme names. ASCII-only
screens may substitute a hyphen for the canonical title's em dash.
Do not rename unrelated source files, modules, APIs or existing tags. Do not
create a release, commit or push without explicit authorization.
