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
//! use A, gpp will run forever.
//! If #define is not given a value, then it will default to an empty string.
//!
//! ## #include
//!
//! Includes, unlike C, do not require quotes or angle brackets, so this: `#include "file.txt"` or
//! this: `#include <file.txt>` will not work; you must write `#include file.txt`.
//!
//! Also, unlike C the directory does not change when you #include; otherwise, gpp would change its
//! current directory and wouldn't be thread safe. This means that if you `#include dir/file.txt`
//! and in `dir/file.txt` it says `#include other_file.txt`, that would refer to `other_file.txt`,
//! not `dir/other_file.txt`.
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
//! three` would be much faster and less platform-dependent. You can also place more commands in
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
//! context.macros.insert("my_macro".to_owned(), "my_value".to_owned());
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
use std::error;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command as SystemCommand, ExitStatus, Stdio};
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
#[derive(Debug, Default)]
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
    pub fn new() -> Self {
        Self::default()
    }
    /// Create a new empty context with no macros or inactive stack and exec commands allowed.
    pub fn new_exec() -> Self {
        Self::new().exec(true)
    }
    /// Create a context from a map of macros.
    pub fn from_macros(macros: impl Into<HashMap<String, String>>) -> Self {
        Self {
            macros: macros.into(),
            ..Default::default()
        }
    }
    /// Create a context from an iterator over tuples.
    pub fn from_macros_iter(macros: impl IntoIterator<Item = (String, String)>) -> Self {
        Self::from_macros(macros.into_iter().collect::<HashMap<_, _>>())
    }
    /// Set whther exec commands are allowed.
    pub fn exec(mut self, allow_exec: bool) -> Self {
        self.allow_exec = allow_exec;
        self
    }
}

/// Error enum for parsing errors.
///
/// # Examples
///
/// ```
/// let error = gpp::Error::TooManyParameters { command: "my_command" };
/// assert_eq!(format!("{}", error), "Too many parameters for #my_command");
/// ```
/// ```
/// let error = gpp::Error::FileError {
///     filename: "my_file".to_string(),
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

fn shell(cmd: &str) -> SystemCommand {
    let (shell, flag) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("/bin/sh", "-c")
    };
    let mut command = SystemCommand::new(shell);
    command.args(&[flag, cmd]);
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
    let mut parts = line.splitn(2, ' ');
    let name = parts.next().unwrap();
    let value = parts.next().unwrap_or("");

    context.macros.insert(name.to_owned(), value.to_owned());
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

#[derive(Clone, Copy)]
struct Command {
    name: &'static str,
    requires_exec: bool,
    ignored_by_if: bool,
    execute: fn(&str, &mut Context) -> Result<String, Error>,
}

const COMMANDS: &[Command] = &[
    Command {
        name: "exec",
        requires_exec: true,
        ignored_by_if: false,
        execute: process_exec,
    },
    Command {
        name: "in",
        requires_exec: true,
        ignored_by_if: false,
        execute: process_in,
    },
    Command {
        name: "endin",
        requires_exec: true,
        ignored_by_if: false,
        execute: process_endin,
    },
    Command {
        name: "include",
        requires_exec: false,
        ignored_by_if: false,
        execute: process_include,
    },
    Command {
        name: "define",
        requires_exec: false,
        ignored_by_if: false,
        execute: process_define,
    },
    Command {
        name: "undef",
        requires_exec: false,
        ignored_by_if: false,
        execute: process_undef,
    },
    Command {
        name: "ifdef",
        requires_exec: false,
        ignored_by_if: true,
        execute: |line, context| process_ifdef(line, context, false),
    },
    Command {
        name: "ifndef",
        requires_exec: false,
        ignored_by_if: true,
        execute: |line, context| process_ifdef(line, context, true),
    },
    Command {
        name: "elifdef",
        requires_exec: false,
        ignored_by_if: true,
        execute: |line, context| process_elifdef(line, context, false),
    },
    Command {
        name: "elifndef",
        requires_exec: false,
        ignored_by_if: true,
        execute: |line, context| process_elifdef(line, context, true),
    },
    Command {
        name: "else",
        requires_exec: false,
        ignored_by_if: true,
        execute: process_else,
    },
    Command {
        name: "endif",
        requires_exec: false,
        ignored_by_if: true,
        execute: process_endif,
    },
];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Finds the next macro name word in the line, and replaces it with its value, returning None when
/// it can't find a macro.
fn replace_next_macro(line: &str, macros: &HashMap<String, String>) -> Option<String> {
    macros.iter().find_map(|(name, value)| {
        let mut parts = line.splitn(2, name);
        let before = parts.next().unwrap();
        let after = parts.next()?;

        dbg!(before.chars().next_back(), after.chars().next());

        if before.chars().next_back().map_or(false, is_word_char)
            || after.chars().next().map_or(false, is_word_char)
        {
            return None;
        }
        let mut new_line = String::with_capacity(before.len() + value.len() + after.len());
        new_line.push_str(before);
        new_line.push_str(value);
        new_line.push_str(after);
        Some(new_line)
    })
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
    let line = line
        .strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(line);

    enum Line<'a> {
        Text(&'a str),
        Command(Command, &'a str),
    }

    let line = if let Some(rest) = line.strip_prefix('#') {
        if rest.starts_with('#') {
            Line::Text(rest)
        } else {
            let mut parts = rest.trim_start().splitn(2, ' ');
            let command_name = parts.next().unwrap();
            let content = parts.next().unwrap_or("").trim_start();

            Line::Command(
                COMMANDS
                    .iter()
                    .copied()
                    .filter(|command| context.allow_exec || !command.requires_exec)
                    .find(|command| command.name == command_name)
                    .ok_or_else(|| Error::InvalidCommand {
                        command_name: command_name.to_owned(),
                    })?,
                content,
            )
        }
    } else {
        Line::Text(line)
    };

    let line = match line {
        Line::Text(_)
        | Line::Command(
            Command {
                ignored_by_if: false,
                ..
            },
            _,
        ) if context.inactive_stack > 0 => String::new(),
        Line::Text(text) => {
            let mut line = format!("{}\n", text);

            while let Some(s) = replace_next_macro(&line, &context.macros) {
                line = s;
            }

            line
        }
        Line::Command(command, content) => (command.execute)(content, context)?,
    };

    Ok(if let Some(child) = context.in_stack.last_mut() {
        let input = child.stdin.as_mut().ok_or(Error::PipeFailed)?;
        input.write_all(line.as_bytes())?;
        String::new()
    } else {
        line
    })
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

    process_buf(file, filename, context)
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
    buf.lines()
        .enumerate()
        .map(|(num, line)| {
            Ok({
                process_line(&line?, context).map_err(|e| Error::FileError {
                    filename: String::from(buf_name),
                    line: num,
                    error: Box::new(e),
                })?
            })
        })
        .collect()
}
