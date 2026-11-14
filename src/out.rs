//! Where printed lines go. Commands print with `say!`; the MCP server captures the same lines and
//! returns them as a tool result, so both surfaces show identical text.

use std::cell::RefCell;
use std::io::Write as _;

thread_local! {
    static CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Print one line to standard output (or into the capture buffer). A closed pipe is not an error:
/// `council plan ... | head -2` must end quietly.
pub fn line(text: &str) {
    let captured = CAPTURE.with(|c| match c.borrow_mut().as_mut() {
        Some(buffer) => {
            buffer.push_str(text);
            buffer.push('\n');
            true
        }
        None => false,
    });
    if !captured {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout
            .write_all(text.as_bytes())
            .and_then(|()| stdout.write_all(b"\n"));
    }
}

/// Print one line to standard error.
pub fn error_line(text: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr
        .write_all(text.as_bytes())
        .and_then(|()| stderr.write_all(b"\n"));
}

/// Run `work` and return what it printed along with its result.
pub fn capture<T>(work: impl FnOnce() -> T) -> (T, String) {
    let before = CAPTURE.with(|c| c.borrow_mut().replace(String::new()));
    let result = work();
    let text = CAPTURE
        .with(|c| std::mem::replace(&mut *c.borrow_mut(), before))
        .unwrap_or_default();
    (result, text)
}

/// Print a line to standard output, `println!` style.
#[macro_export]
macro_rules! say {
    () => { $crate::out::line("") };
    ($($arg:tt)*) => { $crate::out::line(&format!($($arg)*)) };
}

/// Print a line to standard error, `eprintln!` style.
#[macro_export]
macro_rules! complain {
    ($($arg:tt)*) => { $crate::out::error_line(&format!($($arg)*)) };
}
