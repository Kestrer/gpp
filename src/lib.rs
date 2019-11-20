//! gpp is a Generic PreProcessor written in Rust.
//!
//! It supports:
//! - Simple macros, no function macros
//! - #include
//! - #define and #undef
//! - #ifdef, #ifndef, #elifdef, #elifndef, #else and #endif
//! - #exec for running commands
//! - #in and #endin for giving input to commands
//!
//! #includes work differently from C, as they do not require (and do not work with) quotes or <>,
//! so `#include file.txt` is the correct syntax. It does not support #if or #elif, and recursive
//! macros will cause the library to get stuck.
//!
//! # About
//!
//! The hash in any command may be succeeded by optional whitespace, so for example `# undef Macro`
//! is valid, but ` # undef Macro` is not.
//!
//! ## #define and #undef
//!
//! #define works similar to C: `#define [name] [value]`, and #undef too: `#undef [name]`. Be
//! careful though, because unlike C macro expansion is recursive: if you `#define A A` and then
//! use A, then gpp will run forever.
//! If #define is not given a value, then it will default to an empty string.
//!
//! ## #include
//!
//! Includes, unlike C, do not require quotes or angle brackets, so this: `#include "file.txt"` or
//! this: `#include <file.txt>` will not work; you must write `#include file.txt`.
//!
//! ## Ifs
//!
//! The #ifdef, #ifndef, #elifdef, #elifndef, #else and #endif commands work exactly as you expect.
//! I did not add generic #if commands to gpp, as it would make it much more complex and require a
//! lot of parsing, and most of the time these are all you need anyway.
//!
//! ## #exec, #in and #endin
//!
//! The exec command executes the given command with `cmd /C` for Windows and `sh -c` for
//! everything else, and captures the command's standard output. For example, `#exec echo Hi!` will
//! output `Hi!`. It does not capture the command's standard error, and parsing stops if the
//! command exits with a nonzero status.
//!
//! Due to the security risk enabling #exec causes, by default exec is disabled, however you can
//! enable it by changing the `allow_exec` flag in your context. If the input tries to `#exec` when
//! exec is disabled, it will cause an error.
//!
//! The in command is similar to exec, but all text until the endin command is passed into the
//! program's standard input. For example,
//! ```text
//! #in sed 's/tree/three/g'
//! One, two, tree.
//! #endin
//! ```
//! Would output `One, two, three.`. Note that you shouldn't do this, just using `#define tree
//! three` would be much faster and less platform-dependant. You can also place more commands in
//! the in block, including other in blocks. For a useful example:
//! ```text
//! <style>
//! #in sassc -s
//! # include styles.scss
//! #endin
//! </style>
//! ```
//! This compiles your scss file into css using Sassc and includes in the HTML every time you
//! generate your webpage with gpp.
//!
//! ## Literal hashes
//!
//! In order to insert literal hash symbols at the start of the line, simply use two hashes.
//! `##some text` will convert into `#some text`, while `#some text` will throw an error as `some`
//! is not a command.
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
//! #define Line Row
//! Line One
//! Line Two
//! The Third Line", &mut context).unwrap(), "
//! Row One
//! Row Two
//! The Third Row
//! ");
//!
//! // The context persists
//! assert_eq!(context.macros.get("Line").unwrap(), "Row");
//!
//! // Try some more advanced commands
//! assert_eq!(gpp::process_str("
//! Line Four
//! #ifdef Line
//! #undef Line
//! #endif
//! Line Five", &mut context).unwrap(), "
//! Row Four
//! Line Five
//! ");
//! ```

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::env;
use std::error;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::string::FromUtf8Error;

/// Context of the current processing.
///
/// Contains a set of currently defined macros, as well as the number of nested if statements that
/// are being ignored; this is so that if the parser failed an if statement, and it is currently
/// ignoring data, it knows how many endifs it needs to encounter before resuming reading data
/// again. Only if this value is 0 then the parser will read data. It also stores whether the
/// current if group has been accepted; this is for if groups with over three parts.
///
/// There are no limits on what variable names can be; by directly altering Context::macros, you
/// can set variable names not possible with #defines. However, when replacing variable names in
/// text the variable name must be surrounded by two characters that are **not** alphanumeric or an
/// underscore.
#[derive(Default)]
pub struct Context {
    /// Map of all currently defined macros.
    pub macros: HashMap<String, String>,
    /// Number of layers of inactive if statements.
    pub inactive_stack: u32,
    /// Whether the current if statement has been accepted.
    pub used_if: bool,
    /// Whether #exec and #in commands are allowed.
    pub allow_exec: bool,
    /// The stack of processes that #in is piping to.
    pub in_stack: Vec<Child>,
}

impl Context {
    /// Create a new empty context with no macros or inactive stack and exec commands disallowed.
    pub fn new() -> Context {
        Context {
            macros: HashMap::new(),
            inactive_stack: 0,
            used_if: false,
            allow_exec: false,
            in_stack: Vec::new(),
        }
    }
    /// Create a new empty context with no macros or inactive stack and exec commands allowed.
    pub fn new_exec() -> Context {
        Context {
            macros: HashMap::new(),
            inactive_stack: 0,
            used_if: false,
            allow_exec: true,
            in_stack: Vec::new(),
        }
    }
    /// Create a context from an existing HashMap
    pub fn from_map(macros: HashMap<String, String>) -> Context {
        Context {
            macros,
            inactive_stack: 0,
            used_if: false,
            allow_exec: false,
            in_stack: Vec::new(),
        }
    }
    /// Create a context from a vector of tuples
    pub fn from_vec(macros: Vec<(&str, &str)>) -> Context {
        Context {
            macros: macros
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            inactive_stack: 0,
            used_if: false,
            allow_exec: false,
            in_stack: Vec::new(),
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
/// assert_eq!(format!("{}", error), "Too many parameters for #my_command");
/// ```
/// ```
/// let error = gpp::Error::FileError {
///     filename: String::from("my_file"),
///     line: 10,
///     error: Box::new(gpp::Error::UnexpectedCommand {
///         command: "this_command",
///     }),
/// };
/// assert_eq!(format!("{}", error), "Error in my_file:10: Unexpected command #this_command");
/// ```
#[derive(Debug)]
pub enum Error {
    /// An unknown command was encountered.
    InvalidCommand { command_name: String },
    /// Too many parameters were given for a command (for example using #endif with parameters).
    TooManyParameters { command: &'static str },
    /// There was an unexpected command; currently only generated for unexpected #endins.
    UnexpectedCommand { command: &'static str },
    /// The child process for an #exec exited with a nonzero status.
    ChildFailed { status: ExitStatus },
    /// A pipe was unable to be set up to the child.
    PipeFailed,
    /// An error with I/O occurred.
    IoError(io::Error),
    /// An error occurred parsing a child's standard output as UTF-8.
    FromUtf8Error(FromUtf8Error),
    /// An error occurred in another file.
    FileError {
        filename: String,
        line: usize,
        error: Box<Error>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidCommand { command_name } => {
                write!(f, "Invalid command '{}'", command_name)
            }
            Error::TooManyParameters { command } => {
                write!(f, "Too many parameters for #{}", command)
            }
            Error::UnexpectedCommand { command } => write!(f, "Unexpected command #{}", command),
            Error::ChildFailed { status } => write!(f, "Child failed with exit code {}", status),
            Error::PipeFailed => write!(f, "Pipe to child failed"),
            Error::IoError(e) => write!(f, "I/O Error: {}", e),
            Error::FromUtf8Error(e) => write!(f, "UTF-8 Error: {}", e),
            Error::FileError {
                filename,
                line,
                error,
            } => write!(f, "Error in {}:{}: {}", filename, line, error),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::IoError(e) => Some(e),
            Error::FromUtf8Error(e) => Some(e),
            Error::FileError { error: e, .. } => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<FromUtf8Error> for Error {
    fn from(e: FromUtf8Error) -> Self {
        Error::FromUtf8Error(e)
    }
}

fn shell(cmd: &str) -> Command {
    let mut command;
    if cfg!(target_os = "windows") {
        command = Command::new("cmd");
        command.args(&["/C", cmd]);
    } else {
        command = Command::new("sh");
        command.args(&["-c", cmd]);
    }
    command
}

fn process_exec(line: &str, _: &mut Context) -> Result<String, Error> {
    let output = shell(line).output()?;
    if !output.status.success() {
        return Err(Error::ChildFailed {
            status: output.status,
        });
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn process_in(line: &str, context: &mut Context) -> Result<String, Error> {
    let child = shell(line)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    context.in_stack.push(child);
    Ok(String::new())
}

fn process_endin(line: &str, context: &mut Context) -> Result<String, Error> {
    if !line.is_empty() {
        return Err(Error::TooManyParameters { command: "endin" });
    }
    if context.in_stack.is_empty() {
        return Err(Error::UnexpectedCommand { command: "endin" });
    }
    let child = context.in_stack.pop().unwrap();
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Error::ChildFailed {
            status: output.status,
        });
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn process_include(line: &str, context: &mut Context) -> Result<String, Error> {
    process_file(line, context)
}

fn process_define(line: &str, context: &mut Context) -> Result<String, Error> {
    let (name, value) = match line.find(' ') {
        Some(index) => line.split_at(index),
        None => (line, " "),
    };
    // remove leading space
    let value = &value[1..];
    context
        .macros
        .insert(String::from(name), String::from(value));
    Ok(String::new())
}

fn process_undef(line: &str, context: &mut Context) -> Result<String, Error> {
    context.macros.remove(line);
    Ok(String::new())
}

fn process_ifdef(line: &str, context: &mut Context, inverted: bool) -> Result<String, Error> {
    if context.inactive_stack > 0 {
        context.inactive_stack += 1;
    } else if context.macros.contains_key(line) == inverted {
        context.inactive_stack = 1;
        context.used_if = false;
    } else {
        context.used_if = true;
    }
    Ok(String::new())
}

fn process_elifdef(line: &str, context: &mut Context, inverted: bool) -> Result<String, Error> {
    if context.inactive_stack == 0 {
        context.inactive_stack = 1;
    } else if context.inactive_stack == 1
        && !context.used_if
        && context.macros.contains_key(line) != inverted
    {
        context.inactive_stack = 0;
    }
    Ok(String::new())
}

fn process_else(line: &str, context: &mut Context) -> Result<String, Error> {
    if !line.is_empty() {
        return Err(Error::TooManyParameters { command: "else" });
    }
    context.inactive_stack = match context.inactive_stack {
        0 => 1,
        1 if !context.used_if => 0,
        val => val,
    };
    Ok(String::new())
}

fn process_endif(line: &str, context: &mut Context) -> Result<String, Error> {
    if !line.is_empty() {
        return Err(Error::TooManyParameters { command: "endif" });
    }
    if context.inactive_stack != 0 {
        context.inactive_stack -= 1;
    }
    Ok(String::new())
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
/// newlines unless that newline is at the end.
///
/// It returns a Result<String, Error>. If an error occurs, then the Result will be that error.
/// Otherwise, the returned string is the output. If the input did not contain a newline at the
/// end, then this function will add it.
///
/// # Examples
///
/// ```
/// let mut context = gpp::Context::new();
/// context.macros.insert("Foo".to_string(), "Two".to_string());
///
/// assert_eq!(gpp::process_line("One Foo Three", &mut context).unwrap(), "One Two Three\n");
/// ```
/// ```
/// let mut context = gpp::Context::new();
///
/// assert_eq!(gpp::process_line("#define Foo Bar", &mut context).unwrap(), "");
/// assert_eq!(context.macros.get("Foo").unwrap(), "Bar");
/// ```
pub fn process_line(line: &str, context: &mut Context) -> Result<String, Error> {
    let mut chars = line.chars();
    let first = chars.next();
    let second = chars.next();
    let line = if first == Some('#') && second != Some('#') {
        let after_hash = line[1..].trim_start();
        let (command, content) = match after_hash.find(' ') {
            Some(index) => after_hash.split_at(index),
            None => (after_hash, ""),
        };
        let content = content.trim_start();
        match command {
            "exec" if context.inactive_stack == 0 && context.allow_exec => {
                process_exec(content, context)
            }
            "in" if context.inactive_stack == 0 && context.allow_exec => {
                process_in(content, context)
            }
            "endin" if context.inactive_stack == 0 && context.allow_exec => {
                process_endin(content, context)
            }
            "include" if context.inactive_stack == 0 => process_include(content, context),
            "define" if context.inactive_stack == 0 => process_define(content, context),
            "undef" if context.inactive_stack == 0 => process_undef(content, context),
            "ifdef" => process_ifdef(content, context, false),
            "ifndef" => process_ifdef(content, context, true),
            "elifdef" => process_elifdef(content, context, false),
            "elifndef" => process_elifdef(content, context, true),
            "else" => process_else(content, context),
            "endif" => process_endif(content, context),
            command => Err(Error::InvalidCommand {
                command_name: command.to_owned(),
            }),
        }?
    } else {
        if context.inactive_stack > 0 {
            return Ok(String::new());
        }
        let mut line = String::from(if first == Some('#') && second == Some('#') {
            &line[1..]
        } else {
            line
        });
        while let Some(s) = replace_next_macro(&line, &context.macros) {
            line = s;
        }
        if line.chars().rev().next() != Some('\n') {
            line.push('\n');
        }
        line
    };
    if let Some(child) = context.in_stack.last_mut() {
        let input = child.stdin.as_mut().ok_or(Error::PipeFailed)?;
        input.write_all(line.as_bytes())?;
        Ok(String::new())
    } else {
        Ok(line)
    }
}

/// Process a multi-line string of text.
///
/// See `process_buf` for more details.
///
/// # Examples
///
/// ```
/// assert_eq!(gpp::process_str("#define A 1\n A 2 3 \n", &mut gpp::Context::new()).unwrap(), " 1 2 3 \n");
/// ```
pub fn process_str(s: &str, context: &mut Context) -> Result<String, Error> {
    process_buf(s.as_bytes(), "<string>", context)
}

/// Process a file.
///
/// See `process_buf` for more details.
pub fn process_file(filename: &str, context: &mut Context) -> Result<String, Error> {
    let file_raw = File::open(filename)?;
    let file = BufReader::new(file_raw);

    let old_dir = env::current_dir()?;
    let parent_dir = Path::new(filename).parent().unwrap();
    if parent_dir != Path::new("") {
        env::set_current_dir(parent_dir)?;
    }

    let result = process_buf(file, filename, context);

    env::set_current_dir(old_dir)?;

    result
}

/// Process a generic BufRead.
///
/// This function is a wrapper around `process_line`. It splits up the input into lines (adding a
/// newline on the end if there isn't one) and then processes each line.
pub fn process_buf<T: BufRead>(
    buf: T,
    buf_name: &str,
    context: &mut Context,
) -> Result<String, Error> {
    let mut result = String::new();

    for (num, line) in buf.lines().enumerate() {
        let line = line?;
        let result_line = process_line(&line, context).map_err(|e| Error::FileError {
            filename: String::from(buf_name),
            line: num,
            error: Box::new(e),
        })?;
        result.push_str(&result_line);
    }

    Ok(result)
}
