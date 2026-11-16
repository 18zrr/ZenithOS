//! Calculator application for Zenith OS
//!
//! This module provides basic calculator functionality with support for 
//! arithmetic operations and scientific functions.

#![no_std]

/// Calculator error types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalculatorError {
    /// Division by zero encountered
    DivisionByZero,
    /// Invalid mathematical operation
    InvalidOperation,
    /// Input value is out of range
    OutOfRange,
}

/// Calculator structure with basic operations
pub struct Calculator {
    // Current calculation state
    current_value: f64,
}

impl Calculator {
    /// Create a new calculator instance
    pub fn new() -> Self {
        Self {
            current_value: 0.0,
        }
    }

    /// Perform addition operation
    pub fn add(&mut self, value: f64) -> Result<f64, CalculatorError> {
        self.current_value += value;
        Ok(self.current_value)
    }

    /// Perform subtraction operation
    pub fn subtract(&mut self, value: f64) -> Result<f64, CalculatorError> {
        self.current_value -= value;
        Ok(self.current_value)
    }

    /// Perform multiplication operation
    pub fn multiply(&mut self, value: f64) -> Result<f64, CalculatorError> {
        self.current_value *= value;
        Ok(self.current_value)
    }

    /// Perform division operation
    pub fn divide(&mut self, value: f64) -> Result<f64, CalculatorError> {
        if value == 0.0 {
            Err(CalculatorError::DivisionByZero)
        } else {
            self.current_value /= value;
            Ok(self.current_value)
        }
    }

    /// Clear the calculator
    pub fn clear(&mut self) {
        self.current_value = 0.0;
    }

    /// Get current value
    pub fn get_value(&self) -> f64 {
        self.current_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_arithmetic() {
        let mut calc = Calculator::new();
        assert_eq!(calc.add(5.0).unwrap(), 5.0);
        assert_eq!(calc.subtract(3.0).unwrap(), 2.0);
        assert_eq!(calc.multiply(4.0).unwrap(), 8.0);
        assert_eq!(calc.divide(2.0).unwrap(), 4.0);
        calc.clear();
        assert_eq!(calc.get_value(), 0.0);
    }

    #[test]
    fn division_by_zero_is_rejected() {
        let mut calc = Calculator::new();
        assert_eq!(calc.divide(0.0), Err(CalculatorError::DivisionByZero));
        assert_eq!(calc.get_value(), 0.0);
    }
}