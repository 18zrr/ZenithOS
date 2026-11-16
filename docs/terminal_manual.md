# Zenith OS Terminal Manual

## Overview
The Zenith OS terminal is a custom command-line interface designed for the Zenith operating system. It provides users with direct access to system commands, file operations, and basic utilities.

## Getting Started
Upon booting Zenith OS, you'll be presented with a prompt:
```
zenith>
```

To exit the terminal, use the `exit` command (not implemented yet).

## Basic Commands

### help
**Usage:** `help [command]`
**Description:** Shows available commands or detailed information about a specific command.
**Example:**
```bash
zenith> help ls
ls: List directory contents
```

### hello
**Usage:** `hello`
**Description:** Prints a greeting message from Zenith OS.
**Example:**
```bash
zenith> hello
Hello from Zenith OS!
```

### cls
**Usage:** `cls`
**Description:** Clears the terminal screen.
**Example:**
```bash
zenith> cls
```

### ls
**Usage:** `ls [directory]`
**Description:** Lists directory contents. Shows files and subdirectories in the current location.
**Example:**
```bash
zenith> ls
Directory listing (simulated):
  file1.txt
  file2.txt
  dir1/
```

### pwd
**Usage:** `pwd`
**Description:** Prints the current working directory path.
**Example:**
```bash
zenith> pwd
/home/user
```

## Command Syntax

All commands follow a simple syntax:
```
command [arguments]
```

Arguments are separated by spaces. For example, to list files in a specific directory:
```bash
ls /home/user/documents/
```

## File Operations

The terminal provides basic file system operations through Zenith FS:

### Creating Files
Use the `touch` command (not yet implemented) to create new empty files.

### Reading Files
Use the `cat` command (not yet implemented) to display file contents.

### Deleting Files
Use the `rm` command (not yet implemented) to delete files.

## System Information

The terminal can provide system information using commands like:
- `uname`: Shows operating system information
- `df`: Displays disk space usage
- `free`: Shows memory usage

## Customization

The terminal supports customization through environment variables. For example, you can set the prompt format using:
```bash
export PS1="zenith> "
```

## Troubleshooting

### Command Not Found
If a command is not recognized, it will return an error message indicating that the command was not found.

### Permission Denied
Some commands may require elevated privileges. If you encounter permission issues, check your user permissions or contact system administrators.

## Future Features

Upcoming enhancements include:
- Tab completion for commands and file paths
- History of executed commands
- More advanced text editing capabilities
- File transfer operations using Zenith FS