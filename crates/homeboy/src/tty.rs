use crate::error::{Error, ErrorCode, Result};
use std::io::{self, BufRead, IsTerminal, Write};

pub fn is_stdin_tty() -> bool {
    io::stdin().is_terminal()
}

pub fn is_stdout_tty() -> bool {
    io::stdout().is_terminal()
}

pub fn is_stderr_tty() -> bool {
    io::stderr().is_terminal()
}

pub fn require_tty_for_interactive() -> bool {
    is_stdin_tty() && is_stdout_tty()
}

pub fn prompt(message: &str) -> Result<String> {
    eprint!("{}", message);
    io::stderr().flush().ok();

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).map_err(|e| {
        Error::new(
            ErrorCode::InternalIoError,
            format!("Failed to read input: {}", e),
            serde_json::Value::Null,
        )
    })?;

    Ok(line.trim().to_string())
}

pub fn prompt_password(message: &str) -> Result<String> {
    prompt(message)
}
