use std::io;
use std::process::Command;

#[cfg(target_os = "linux")]
const DESKTOP_CODEX: &str = "/usr/lib/chatgpt/resources/codex";

pub(super) fn run<T>(operation: impl Fn(&mut Command) -> io::Result<T>) -> io::Result<T> {
    let result = operation(&mut Command::new("codex"));
    #[cfg(target_os = "linux")]
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        return operation(&mut Command::new(DESKTOP_CODEX));
    }
    result
}
