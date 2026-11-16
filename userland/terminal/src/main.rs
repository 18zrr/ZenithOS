//! Main entry point for the Zenith OS terminal.
//!
//! Host-side harness: the library itself is `no_std` and will be driven by the
//! Zenith OS console once kernel integration is in place.

#![deny(warnings)]

use std::io::{self, BufRead, Write};

use terminal::Terminal;

fn main() {
    let mut terminal = Terminal::new();

    println!("Welcome to Zenith OS Terminal");
    println!("Type 'help' for available commands, 'exit' to quit.");

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("Error reading input: {}", error);
                break;
            }
        };

        print!("{}", terminal.get_prompt());
        let _ = io::stdout().flush();

        match terminal.process_line(&line) {
            Ok(output) => {
                if !output.is_empty() {
                    println!("{}", output);
                }
            }
            Err(error) => {
                if error == "Exiting terminal" {
                    break;
                }
                eprintln!("Error: {}", error);
            }
        }
    }
}
