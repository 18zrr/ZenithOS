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