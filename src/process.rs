//! Running child programs, with failures that say what was actually run.
//!
//! Every CLI that shells out re-derives the same three fiddly details: telling
//! "the program is not installed" apart from "the program ran and failed",
//! rendering the command line back to the user without it becoming ambiguous,
//! and digging the explanation out of stderr — or stdout, when the program put
//! it there instead.
//!
//! The error carries those pieces separately rather than a finished sentence,
//! because consumers word their failures differently and a shared runner should
//! not flatten that.

use std::{
    ffi::OsStr,
    io,
    path::Path,
    process::{Command, ExitStatus, Output, Stdio},
};

/// A child program that did not produce a usable result.
#[derive(Debug)]
pub struct CommandError {
    program: String,
    command: String,
    kind: CommandErrorKind,
}

#[derive(Debug)]
pub enum CommandErrorKind {
    /// The program could not be started at all. A kind of
    /// [`io::ErrorKind::NotFound`] means it is not installed, or not on `PATH`.
    Spawn(io::Error),
    /// The program ran and reported failure.
    Failed {
        status: ExitStatus,
        stdout: String,
        stderr: String,
    },
}

impl CommandError {
    /// The program name alone, e.g. `git`.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The whole command line, quoted so it can be pasted into a shell.
    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn kind(&self) -> &CommandErrorKind {
        &self.kind
    }

    /// Consumes the error to yield its kind, for consumers that need to take
    /// ownership of the underlying [`io::Error`] rather than borrow it — a
    /// crate folding this into its own error enum, typically.
    pub fn into_kind(self) -> CommandErrorKind {
        self.kind
    }

    /// True when the program is not installed or not on `PATH`.
    pub fn is_not_found(&self) -> bool {
        matches!(
            &self.kind,
            CommandErrorKind::Spawn(error) if error.kind() == io::ErrorKind::NotFound
        )
    }

    pub fn status(&self) -> Option<ExitStatus> {
        match &self.kind {
            CommandErrorKind::Failed { status, .. } => Some(*status),
            CommandErrorKind::Spawn(_) => None,
        }
    }

    /// Captured stderr, lossily decoded. Empty when the program never started.
    pub fn stderr(&self) -> &str {
        match &self.kind {
            CommandErrorKind::Failed { stderr, .. } => stderr,
            CommandErrorKind::Spawn(_) => "",
        }
    }

    /// Captured stdout, lossily decoded. Empty when the program never started.
    pub fn stdout(&self) -> &str {
        match &self.kind {
            CommandErrorKind::Failed { stdout, .. } => stdout,
            CommandErrorKind::Spawn(_) => "",
        }
    }

    /// The program's own explanation: trimmed stderr, falling back to trimmed
    /// stdout for programs that report errors there. Empty when it said
    /// nothing useful.
    pub fn message(&self) -> &str {
        let stderr = self.stderr().trim();
        if stderr.is_empty() {
            self.stdout().trim()
        } else {
            stderr
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            CommandErrorKind::Spawn(_) if self.is_not_found() => write!(
                formatter,
                "required command `{}` was not found on PATH",
                self.program
            ),
            CommandErrorKind::Spawn(error) => {
                write!(formatter, "could not run {}: {error}", self.command)
            }
            CommandErrorKind::Failed { .. } => {
                let message = self.message();
                if message.is_empty() {
                    write!(formatter, "command failed: {}", self.command)
                } else {
                    write!(formatter, "command failed: {}: {message}", self.command)
                }
            }
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            CommandErrorKind::Spawn(error) => Some(error),
            CommandErrorKind::Failed { .. } => None,
        }
    }
}

pub type CommandResult<T> = std::result::Result<T, CommandError>;

/// Runs `command` to completion, capturing its output.
///
/// `verbose` echoes the command to stderr first, in the `+ program args` form
/// shells use for trace output.
pub fn output(command: &mut Command, verbose: bool) -> CommandResult<Output> {
    let rendered = display(command);
    if verbose {
        eprintln!("+ {rendered}");
    }
    // `Command::output` already closes the child's stdin, so a program that
    // decides to prompt sees EOF rather than stealing the terminal.
    let produced = command
        .output()
        .map_err(|error| spawn_error(command, rendered.clone(), error))?;
    if produced.status.success() {
        return Ok(produced);
    }
    Err(CommandError {
        program: program_of(command),
        command: rendered,
        kind: CommandErrorKind::Failed {
            status: produced.status,
            stdout: String::from_utf8_lossy(&produced.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&produced.stderr).into_owned(),
        },
    })
}

/// Runs `command` to completion, letting it write straight to the terminal.
///
/// Use this for programs whose output is the point — a compiler, a deploy —
/// where capturing it would hide progress from the user.
pub fn status(command: &mut Command, verbose: bool) -> CommandResult<()> {
    let rendered = display(command);
    if verbose {
        eprintln!("+ {rendered}");
    }
    // Nothing is captured here, so stdin has to be closed explicitly or a
    // prompting child would block on a terminal the caller is not watching.
    let status = command
        .stdin(Stdio::null())
        .status()
        .map_err(|error| spawn_error(command, rendered.clone(), error))?;
    if status.success() {
        return Ok(());
    }
    Err(CommandError {
        program: program_of(command),
        command: rendered,
        kind: CommandErrorKind::Failed {
            status,
            stdout: String::new(),
            stderr: String::new(),
        },
    })
}

/// Whether `program` can be run at all, by asking it for its version.
pub fn available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Runs `git` inside `cwd`.
pub fn git<I, S>(cwd: &Path, arguments: I, verbose: bool) -> CommandResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    output(
        Command::new("git").args(arguments).current_dir(cwd),
        verbose,
    )
}

/// The command line, quoted so that it round-trips through a shell.
///
/// Without quoting, a command carrying a message or a path with spaces reads
/// back as a different command than the one that ran.
pub fn display(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|part| quote(&part.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn spawn_error(command: &Command, rendered: String, error: io::Error) -> CommandError {
    CommandError {
        program: program_of(command),
        command: rendered,
        kind: CommandErrorKind::Spawn(error),
    }
}

fn program_of(command: &Command) -> String {
    command.get_program().to_string_lossy().into_owned()
}

fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:=@".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{0}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_reported_as_not_found() {
        let mut command = Command::new("wombat-cc-definitely-not-a-real-program");
        let error = output(&mut command, false).unwrap_err();
        assert!(error.is_not_found());
        assert_eq!(error.program(), "wombat-cc-definitely-not-a-real-program");
        assert!(error.status().is_none());
        assert!(error.to_string().contains("was not found on PATH"));
    }

    #[test]
    fn a_failing_program_carries_its_exit_status_and_stderr() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo trouble >&2; exit 3"]);
        let error = output(&mut command, false).unwrap_err();

        assert!(!error.is_not_found());
        assert_eq!(error.status().and_then(|status| status.code()), Some(3));
        assert_eq!(error.message(), "trouble");
    }

    /// Some programs report their failure on stdout, so an error that only
    /// reads stderr would show the user an empty explanation.
    #[test]
    fn stdout_is_used_when_stderr_is_silent() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo 'said on stdout'; exit 1"]);
        let error = output(&mut command, false).unwrap_err();
        assert_eq!(error.stderr().trim(), "");
        assert_eq!(error.message(), "said on stdout");
    }

    #[test]
    fn a_successful_program_returns_its_output() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo hello"]);
        let produced = output(&mut command, false).unwrap();
        assert_eq!(String::from_utf8_lossy(&produced.stdout).trim(), "hello");
    }

    #[test]
    fn status_reports_failure_without_capturing() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 7"]);
        let error = status(&mut command, false).unwrap_err();
        assert_eq!(error.status().and_then(|value| value.code()), Some(7));
        assert_eq!(error.message(), "");
    }

    /// An argument with a space in it has to come back quoted, or the rendered
    /// command means something different from the one that ran.
    #[test]
    fn rendering_quotes_arguments_that_need_it() {
        let mut command = Command::new("git");
        command.args(["commit", "-m", "a message", "--allow-empty"]);
        assert_eq!(display(&command), "git commit -m 'a message' --allow-empty");
    }

    #[test]
    fn rendering_escapes_embedded_quotes_and_empty_arguments() {
        let mut command = Command::new("sh");
        command.args(["-c", "it's", ""]);
        assert_eq!(display(&command), r"sh -c 'it'\''s' ''");
    }

    #[test]
    fn availability_distinguishes_real_programs_from_invented_ones() {
        assert!(available("sh"));
        assert!(!available("wombat-cc-definitely-not-a-real-program"));
    }

    #[test]
    fn git_runs_inside_the_requested_directory() {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), ["init"], false).unwrap();
        assert!(directory.path().join(".git").is_dir());
    }
}
