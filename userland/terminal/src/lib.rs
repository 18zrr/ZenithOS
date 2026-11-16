//! Zenith OS Terminal Implementation

#![no_std]
#![deny(warnings)]

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

/// Terminal command trait
pub trait Command {
    fn name(&self) -> &str;
    fn help(&self) -> &str;
    fn execute(&self, args: &[&str]) -> Result<String, String>;
}

/// Terminal struct for handling user input and output
pub struct Terminal {
    commands: Vec<Box<dyn Command>>,
    prompt: String,
}

impl Terminal {
    /// Create a new terminal instance
    pub fn new() -> Self {
        let mut term = Terminal {
            commands: Vec::new(),
            prompt: "zenith> ".to_string(),
        };
        
        // Register built-in commands
        term.register_command(Box::new(HelloCommand));
        term.register_command(Box::new(ClsCommand));
        term.register_command(Box::new(LsCommand));
        term.register_command(Box::new(PwdCommand));
        term.register_command(Box::new(ExitCommand));
        
        term
    }

    /// Register a new command
    pub fn register_command(&mut self, command: Box<dyn Command>) {
        self.commands.push(command);
    }

    /// Process a command line input
    pub fn process_line(&mut self, input: &str) -> Result<String, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(String::new());
        }

        // Parse command and arguments
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let command_name = parts[0];

        // `help` needs access to the whole command registry, so it is handled
        // here instead of by a standalone Command implementation.
        if command_name == "help" {
            return Ok(self.help_text(parts.get(1).copied()));
        }

        // Find matching command
        for cmd in &self.commands {
            if cmd.name() == command_name {
                return cmd.execute(&parts[1..]);
            }
        }

        Err(format!("Command not found: {}", command_name))
    }

    /// Build help text for all commands, or for a single command when a
    /// topic is supplied.
    fn help_text(&self, topic: Option<&str>) -> String {
        if let Some(topic) = topic {
            if topic == "help" {
                return "help: Show this help message or help for a specific command\n".to_string();
            }
            for cmd in &self.commands {
                if cmd.name() == topic {
                    return format!("{}: {}\n", cmd.name(), cmd.help());
                }
            }
            return format!("Command not found: {}", topic);
        }

        let mut output = String::new();
        output.push_str("Available commands:\n");
        output.push_str("  help: Show this help message or help for a specific command\n");
        for cmd in &self.commands {
            let _ = writeln!(&mut output, "  {}: {}", cmd.name(), cmd.help());
        }
        output
    }

    /// Get the prompt string
    pub fn get_prompt(&self) -> &str {
        &self.prompt
    }
}

/// Hello command - prints a greeting
struct HelloCommand;

impl Command for HelloCommand {
    fn name(&self) -> &str {
        "hello"
    }
    
    fn help(&self) -> &str {
        "Prints a greeting message"
    }
    
    fn execute(&self, _args: &[&str]) -> Result<String, String> {
        Ok("Hello from Zenith OS!".to_string())
    }
}

/// Clear screen command
struct ClsCommand;

impl Command for ClsCommand {
    fn name(&self) -> &str {
        "cls"
    }
    
    fn help(&self) -> &str {
        "Clear the terminal screen"
    }
    
    fn execute(&self, _args: &[&str]) -> Result<String, String> {
        Ok("\x1B[2J\x1B[1;1H".to_string()) // ANSI escape codes to clear screen
    }
}

/// List directory contents command
struct LsCommand;

impl Command for LsCommand {
    fn name(&self) -> &str {
        "ls"
    }
    
    fn help(&self) -> &str {
        "List directory contents"
    }
    
    fn execute(&self, _args: &[&str]) -> Result<String, String> {
        // In a real implementation this would interact with Zenith FS
        Ok("Directory listing (simulated):\n  file1.txt\n  file2.txt\n  dir1/".to_string())
    }
}

/// Print working directory command
struct PwdCommand;

impl Command for PwdCommand {
    fn name(&self) -> &str {
        "pwd"
    }
    
    fn help(&self) -> &str {
        "Print current working directory"
    }
    
    fn execute(&self, _args: &[&str]) -> Result<String, String> {
        Ok("/home/user".to_string())
    }
}

/// Exit command
struct ExitCommand;

impl Command for ExitCommand {
    fn name(&self) -> &str {
        "exit"
    }
    
    fn help(&self) -> &str {
        "Exit the terminal"
    }
    
    fn execute(&self, _args: &[&str]) -> Result<String, String> {
        Err("Exiting terminal".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_commands_work() {
        let mut terminal = Terminal::new();
        assert_eq!(terminal.process_line("hello").unwrap(), "Hello from Zenith OS!");
        assert_eq!(terminal.process_line("pwd").unwrap(), "/home/user");
        assert!(terminal.process_line("nope").is_err());
    }

    #[test]
    fn help_lists_all_commands() {
        let mut terminal = Terminal::new();
        let help = terminal.process_line("help").unwrap();
        assert!(help.contains("hello"));
        assert!(help.contains("cls"));
        assert!(help.contains("ls"));
        assert!(help.contains("help"));

        let topic = terminal.process_line("help hello").unwrap();
        assert!(topic.contains("greeting"));
    }
}