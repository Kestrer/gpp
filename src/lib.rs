//! gpp is a Generic PreProcessor written in Rust.
//!
//! It supports:
//! - Simple macros, no function macros
//! - #define
//! - #undef
//! - #ifdef
//! - #ifndef
//! - #elifdef
//! - #elifndef
//! - #else
//! - #endif
//!
//! It does not support #if or #elif.
//!
//! This library is heavily inspired by [minipre](https://docs.rs/minipre/0.2.0/minipre/), however
//! this library supports more commands like #define-s and #undef-s.
//!
//! # Examples
//!
//! ```
//! // Create a context for preprocessing
//! let mut context = gpp::Context::new();
//!
//! // Add a macro to that context manually (context.macros is a HashMap)
//! context.macros.insert(String::from("my_macro"), String::from("my_value"));
//!
//! // Process some text using that
//! assert_eq!(gpp::process_str("My macro is my_macro\n", &mut context).unwrap(), "My macro is my_value\n");
//!
//! // Process some multi-line text, changing the context
//! assert_eq!(gpp::process_str("
//!     #define Line Row
//!     Line One
//!     Line Two
//!     The Third Line", &mut context).unwrap(), "
//!     Row One
//!     Row Two
//!     The Third Row\n");
//!
//! // The context persists
//! assert_eq!(context.macros.get("Line").unwrap(), "Row");
//!
//! // Try some more advanced statements
//! assert_eq!(gpp::process_str("
//!     Line Four
//!     #ifdef Line
//!     #undef Line
//!     #endif
//!     Line Five", &mut context).unwrap(), "
//!     Row Four
//!     Line Five\n");
//! ```

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead};

/// Context of the current processing.
///
/// Contains a set of currently defined macros, as well as the number of nested if statements that
/// are being ignored; this is so that if the parser failed an if statement, and it is currently
/// ignoring data, it knows how many endifs it needs to encounter before resuming reading data
/// again. Only if this value is 0 then the parser will read data.
///
/// There are no limits on what variable names can be; by directly altering Context::macros, you
/// can set variable names not possible with #defines. However, when replacing variable names in
/// text the variable name must be surrounded by two characters that are **not** alphanumeric or an
/// underscore.
pub struct Context {
    /// Map of all currently defined macros.
    pub macros: HashMap<String, String>,
    /// Number of layers of inactive if statements
    pub inactive_stack: u32,
}

impl Context {
    /// Create a new empty context with no macros or inactive stack.
    pub fn new() -> Context {
        Context {
            macros: HashMap::new(),
            inactive_stack: 0,
        }
    }
}

/// Error enum for parsing errors.
///
/// This type implements std::fmt::Display, and so can easily be printed with println!.
///
/// # Examples
///
/// ```
/// let error = gpp::Error::TooManyParameters { command: "my_command" };
/// assert_eq!(format!("{}", error), "Too many parameters for my_command");
/// ```
#[derive(Debug)]
pub enum Error {
    InvalidCommand { command_name: String },
    InvalidMacroName { macro_name: String },
    TooManyParameters { command: &'static str },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidCommand { command_name } => {
                write!(f, "Invalid command '{}'", command_name)
            }
            Error::InvalidMacroName { macro_name } => {
                write!(f, "Invalid macro name '{}'", macro_name)
            }
            Error::TooManyParameters { command } => {
                write!(f, "Too many parameters for {}", command)
            }
        }
    }
}

fn process_define(line: &str, context: &mut Context) -> Result<(), Error> {
    let (name, value) = match line.find(' ') {
        Some(index) => line.split_at(index),
        None => (line, " 1"),
    };
    // remove leading space
    let value = &value[1..];
    context
        .macros
        .insert(String::from(name), String::from(value));
    Ok(())
}

fn process_undef(line: &str, context: &mut Context) -> Result<(), Error> {
    context.macros.remove(line);
    Ok(())
}

fn process_ifdef(line: &str, context: &mut Context, inverted: bool) -> Result<(), Error> {
    if context.inactive_stack > 0 {
        context.inactive_stack += 1;
    } else if context.macros.contains_key(line) == inverted {
        context.inactive_stack = 1;
    }
    Ok(())
}

fn process_elifdef(line: &str, context: &mut Context, inverted: bool) -> Result<(), Error> {
    if context.inactive_stack == 0 {
        context.inactive_stack = 1;
    } else if context.inactive_stack == 1 && context.macros.contains_key(line) != inverted {
        context.inactive_stack = 0;
    }
    Ok(())
}

fn process_else(line: &str, context: &mut Context) -> Result<(), Error> {
    if !line.is_empty() {
        return Err(Error::TooManyParameters { command: "else" });
    }
    context.inactive_stack = match context.inactive_stack {
        0 => 1,
        1 => 0,
        val => val,
    };
    Ok(())
}

fn process_endif(line: &str, context: &mut Context) -> Result<(), Error> {
    if !line.is_empty() {
        return Err(Error::TooManyParameters { command: "endif" });
    }
    if context.inactive_stack > 0 {
        context.inactive_stack -= 1;
    }
    Ok(())
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// Checks whether the given string with position pos and length len in s is surrounded by non-word
// chars; like the regex \B\w+\B.
fn is_word(s: &str, pos: usize, len: usize) -> bool {
    let mut prev_char = pos;
    if prev_char != 0 {
        prev_char -= 1;
        while !s.is_char_boundary(prev_char) {
            prev_char -= 1;
        }
    }
    if pos > 0 && is_word_char(s[prev_char..pos].chars().next().unwrap()) {
        return false;
    }
    if pos + len < s.len() && is_word_char(s[pos + len..].chars().next().unwrap()) {
        return false;
    }
    true
}

// Finds the next macro name word in the line, and replaces it with its value, returning None when
// it can't find a macro.
fn replace_next_macro(line: &str, macros: &HashMap<String, String>) -> Option<String> {
    for (name, value) in macros {
        let index = match line.find(name) {
            Some(i) => i,
            None => continue,
        };
        if !is_word(line, index, name.len()) {
            continue;
        }
        let mut new_line = String::new();
        new_line.reserve(line.len() - name.len() + value.len());
        new_line.push_str(&line[..index]);
        new_line.push_str(value);
        new_line.push_str(&line[index + name.len()..]);
        return Some(new_line);
    }
    None
}

/// Process a string line of input.
///
/// This is the smallest processing function, and all other processing functions are wrappers
/// around it. It only processes singular lines, and will not work on any string that contains
/// newlines.
///
/// It returns a Result<Option<String>, Error>. If an error occurs, then the Result will be that
/// error. Otherwise, if the Option is None then it means that the line parsed was a command or the
/// result of a failed if, and a blank line should not be placed in the output; if the option is
/// Some(String), then place that String followed by a newline in the output.
///
/// # Examples
///
/// ```
/// let mut context = gpp::Context::new();
/// context.macros.insert("Foo".to_string(), "Two".to_string());
///
/// assert_eq!(gpp::process_line("One Foo Three", &mut context).unwrap().unwrap(), "One Two Three");
/// ```
/// ```
/// let mut context = gpp::Context::new();
///
/// assert!(gpp::process_line("#define Foo Bar", &mut context).unwrap().is_none());
/// assert_eq!(context.macros.get("Foo").unwrap(), "Bar");
/// ```
pub fn process_line(line: &str, context: &mut Context) -> Result<Option<String>, Error> {
    if let Some('#') = line.trim_start().chars().next() {
        let after_hash = line.trim_start()[1..].trim_start();
        let (statement, content) = match after_hash.find(' ') {
            Some(index) => after_hash.split_at(index),
            None => (after_hash, ""),
        };
        let content = content.trim_start();
        match statement {
            "define" if context.inactive_stack == 0 => process_define(content, context)?,
            "undef" if context.inactive_stack == 0 => process_undef(content, context)?,
            "ifdef" => process_ifdef(content, context, false)?,
            "ifndef" => process_ifdef(content, context, true)?,
            "elifdef" => process_elifdef(content, context, false)?,
            "elifndef" => process_elifdef(content, context, true)?,
            "else" => process_else(content, context)?,
            "endif" => process_endif(content, context)?,
            command => {
                return Err(Error::InvalidCommand {
                    command_name: command.to_owned(),
                })
            }
        }
        return Ok(None);
    }
    if context.inactive_stack > 0 {
        return Ok(None);
    }
    let mut line = String::from(line);
    while let Some(s) = replace_next_macro(&line, &context.macros) {
        line = s;
    }
    Ok(Some(line))
}

/// Error struct for errors and a line number.
///
/// These errors are wrappers around the regular Error type, but also contain a usize that shows on
/// which line the error occurred.
///
/// It implements std::fmt::Display, and so can be easily printed with println!.
///
/// # Examples
///
/// ```
/// let error = gpp::LineError { line: 40, error: gpp::Error::InvalidCommand { command_name: String::from("my_invalid_command") } };
/// assert_eq!(format!("{}", error), "Error on line 40: Invalid command 'my_invalid_command'");
/// ```
#[derive(Debug)]
pub struct LineError {
    /// The line on which the error occurred.
    pub line: usize,
    /// The error itself.
    pub error: Error,
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error on line {}: {}", self.line, self.error)
    }
}

/// Process a multi-line string of text.
///
/// This function is a wrapper around `process_line`. It splits up the text into lines, adding a
/// newline on the end if there isn't one, and processes it.
///
/// It returns either a String representing the final processed text or a LineError if something
/// went wrong.
///
/// # Examples
///
/// ```
/// assert_eq!(gpp::process_str("#define A\n A 2 3 \n", &mut gpp::Context::new()).unwrap(), " 1 2 3 \n");
/// ```
pub fn process_str(s: &str, context: &mut Context) -> Result<String, LineError> {
    let mut result = String::new();

    for (num, line) in s.lines().enumerate() {
        match process_line(line, context) {
            Ok(Some(result_line)) => {
                result.push_str(&result_line);
                result.push('\n');
            }
            Ok(None) => {}
            Err(e) => {
                return Err(LineError {
                    line: num,
                    error: e,
                })
            }
        };
    }

    return Ok(result);
}

/// Error enum for errors caused by BufRead.
///
/// This error enum is a wrapper around either a LineError, or an IoError. It is generated by
/// process_buf, which deals with io::Error-prone BufReads, not strings.
///
/// It also implements fmt::Display, and so can be easily formatted and printed.
///
/// # Examples
/// ```
/// use std::io;
///
/// let error = gpp::BufError::IoError(io::Error::new(io::ErrorKind::Other, "Failed to read"));
/// assert_eq!(format!("{}", error), "I/O error: Failed to read");
/// ```
#[derive(Debug)]
pub enum BufError {
    LineError(LineError),
    IoError(io::Error),
}

impl fmt::Display for BufError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BufError::LineError(e) => write!(f, "{}", e),
            BufError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

/// Process a generic BufRead.
///
/// This function is exactly like `process_str`, but works for any type that implements std::io::BufRead.
///
/// Unlike `process_str`, its errors are more complicated. It returns a Result<String, BufError>.
/// BufError is a wrapper enum error type that can contain LineError(LineError) or
/// IoError(io::Error).
pub fn process_buf<T: BufRead>(buf: T, context: &mut Context) -> Result<String, BufError> {
    let mut result = String::new();

    for (num, line) in buf.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(e) => return Err(BufError::IoError(e)),
        };
        match process_line(&line, context) {
            Ok(Some(result_line)) => {
                result.push_str(&result_line);
                result.push('\n');
            }
            Ok(None) => {}
            Err(e) => {
                return Err(BufError::LineError(LineError {
                    line: num,
                    error: e,
                }))
            }
        };
    }

    return Ok(result);
}