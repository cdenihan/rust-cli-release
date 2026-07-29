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
    /// The command asked to run in a directory that is not there. Recorded at
    /// spawn time because the operating system reports it as the same
    /// `NotFound` a missing executable produces, and by the time anyone reads
    /// the error the two are indistinguishable.
    missing_working_directory: bool,
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

    /// True when the program itself is not installed or not on `PATH`.
    ///
    /// Deliberately false when the spawn failed because the requested working
    /// directory is absent: the operating system reports that as the same
    /// `NotFound`, and answering `true` would send the user off to install a
    /// program they already have.
    pub fn is_not_found(&self) -> bool {
        !self.missing_working_directory
            && matches!(
                &self.kind,
                CommandErrorKind::Spawn(error) if error.kind() == io::ErrorKind::NotFound
            )
    }

    /// True when the command could not start because the directory it was
    /// told to run in does not exist.
    pub fn is_missing_working_directory(&self) -> bool {
        self.missing_working_directory
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
            CommandErrorKind::Spawn(_) if self.missing_working_directory => write!(
                formatter,
                "could not run {}: its working directory does not exist",
                self.command
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
        missing_working_directory: false,
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
        missing_working_directory: false,
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

/// The command line, quoted so that it round-trips through the host's shell.
///
/// Without quoting, a command carrying a message or a path with spaces reads
/// back as a different command than the one that ran. Quoting is
/// platform-specific because the conventions genuinely differ: POSIX shells
/// group with single quotes, while `cmd.exe` and the Windows C runtime treat
/// those as ordinary characters and group with double quotes.
pub fn display(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a word can be written with no quoting at all, on any platform.
fn is_bare(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:=@".contains(byte))
}

fn spawn_error(command: &Command, rendered: String, error: io::Error) -> CommandError {
    // Checked here, while the command is still in hand: the OS reports an
    // absent working directory with the same `NotFound` as an absent program.
    let missing_working_directory = error.kind() == io::ErrorKind::NotFound
        && command
            .get_current_dir()
            .is_some_and(|directory| !directory.is_dir());
    CommandError {
        program: program_of(command),
        command: rendered,
        kind: CommandErrorKind::Spawn(error),
        missing_working_directory,
    }
}

fn program_of(command: &Command) -> String {
    command.get_program().to_string_lossy().into_owned()
}

/// POSIX shell quoting.
///
/// Arguments and paths on Unix are arbitrary bytes, not necessarily UTF-8, so
/// a lossy conversion here would render two different commands identically and
/// show a path that was never run. Bytes that are not valid UTF-8 fall back to
/// `$'...'` escapes, which carry them exactly.
#[cfg(unix)]
fn quote(value: &OsStr) -> String {
    use std::{fmt::Write as _, os::unix::ffi::OsStrExt as _};

    let bytes = value.as_bytes();
    if is_bare(bytes) {
        // Every accepted byte is ASCII, so this cannot lose anything.
        return String::from_utf8_lossy(bytes).into_owned();
    }
    if let Some(text) = value.to_str() {
        return format!("'{0}'", text.replace('\'', "'\\''"));
    }
    let mut escaped = String::from("$'");
    for byte in bytes {
        match byte {
            b'\'' => escaped.push_str("\\'"),
            b'\\' => escaped.push_str("\\\\"),
            0x20..=0x7e => escaped.push(*byte as char),
            _ => {
                let _ = write!(escaped, "\\x{byte:02x}");
            }
        }
    }
    escaped.push('\'');
    escaped
}

/// Windows command-line quoting.
///
/// `cmd.exe` and the C runtime group with double quotes; a backslash run that
/// immediately precedes a quote has to be doubled, and a trailing run doubled
/// as well, or the closing quote is swallowed.
#[cfg(windows)]
fn quote(value: &OsStr) -> String {
    let text = value.to_string_lossy();
    if is_bare(text.as_bytes()) {
        return text.into_owned();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0_usize;
    for character in text.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                for _ in 0..backslashes * 2 + 1 {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push('"');
            }
            _ => {
                for _ in 0..backslashes {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push(character);
            }
        }
    }
    for _ in 0..backslashes * 2 {
        quoted.push('\\');
    }
    quoted.push('"');
    quoted
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
        let mut command = Command::new("git");
        command.arg("not-a-git-subcommand");
        let error = output(&mut command, false).unwrap_err();

        assert!(!error.is_not_found());
        assert!(error.status().is_some_and(|status| !status.success()));
        assert!(
            error.message().contains("not-a-git-subcommand"),
            "git's own complaint must survive: {}",
            error.message()
        );
    }

    /// Some programs report their failure on stdout, so an error that only
    /// reads stderr would show the user an empty explanation. Needs a shell to
    /// arrange, so it is checked where one is guaranteed.
    #[cfg(unix)]
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
        let mut command = Command::new("git");
        command.arg("--version");
        let produced = output(&mut command, false).unwrap();
        assert!(String::from_utf8_lossy(&produced.stdout).starts_with("git version"));
    }

    #[test]
    fn status_reports_failure_without_capturing() {
        let mut command = Command::new("git");
        command.arg("not-a-git-subcommand");
        let error = status(&mut command, false).unwrap_err();
        assert!(error.status().is_some_and(|status| !status.success()));
        // Nothing was captured, so there is no explanation to report.
        assert_eq!(error.message(), "");
    }

    /// A deleted working directory fails to spawn with the same `NotFound`
    /// that a missing program produces. Reporting it as a missing program
    /// sends the user to install something they already have.
    #[test]
    fn a_missing_working_directory_is_not_a_missing_program() {
        let directory = tempfile::tempdir().unwrap();
        let absent = directory.path().join("gone");
        let mut command = Command::new("git");
        command.arg("--version").current_dir(&absent);

        let error = output(&mut command, false).unwrap_err();

        assert!(
            !error.is_not_found(),
            "git is installed; the directory is what is missing"
        );
        assert!(error.is_missing_working_directory());
        assert!(
            error
                .to_string()
                .contains("working directory does not exist"),
            "the message must point at the directory: {error}"
        );
    }

    #[test]
    fn a_missing_program_is_still_reported_when_the_directory_is_fine() {
        let directory = tempfile::tempdir().unwrap();
        let mut command = Command::new("wombat-cc-definitely-not-a-real-program");
        command.current_dir(directory.path());

        let error = output(&mut command, false).unwrap_err();

        assert!(error.is_not_found());
        assert!(!error.is_missing_working_directory());
    }

    /// Unix arguments are bytes, not necessarily UTF-8. A lossy rendering
    /// would collapse distinct commands into the same text.
    #[cfg(unix)]
    #[test]
    fn rendering_preserves_arguments_that_are_not_utf8() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let mut command = Command::new("cat");
        command.arg(OsString::from_vec(vec![0xff, 0xfe]));
        let rendered = display(&command);

        assert!(
            !rendered.contains('\u{fffd}'),
            "the replacement character means the bytes were lost: {rendered}"
        );
        assert_eq!(rendered, r"cat $'\xff\xfe'");

        // Two different byte strings must not render alike.
        let mut other = Command::new("cat");
        other.arg(OsString::from_vec(vec![0xfe, 0xff]));
        assert_ne!(display(&other), rendered);
    }

    /// An argument with a space in it has to come back quoted, or the rendered
    /// command means something different from the one that ran.
    #[test]
    fn rendering_quotes_arguments_that_need_it() {
        let mut command = Command::new("git");
        command.args(["commit", "-m", "a message", "--allow-empty"]);
        let rendered = display(&command);

        // Bare words are untouched on every platform; only the grouping
        // syntax around "a message" differs.
        assert!(rendered.starts_with("git commit -m "));
        assert!(rendered.ends_with(" --allow-empty"));
        assert!(rendered.contains("a message"));

        #[cfg(unix)]
        assert_eq!(rendered, "git commit -m 'a message' --allow-empty");
        #[cfg(windows)]
        assert_eq!(rendered, "git commit -m \"a message\" --allow-empty");
    }

    #[cfg(unix)]
    #[test]
    fn rendering_escapes_embedded_quotes_and_empty_arguments() {
        let mut command = Command::new("sh");
        command.args(["-c", "it's", ""]);
        assert_eq!(display(&command), r"sh -c 'it'\''s' ''");
    }

    /// Single quotes are ordinary characters to `cmd.exe`, so a command that
    /// used them could not be pasted back. Backslash runs before a quote, and
    /// at the end, have to be doubled or the closing quote is swallowed.
    #[cfg(windows)]
    #[test]
    fn rendering_uses_windows_grouping_rules() {
        let mut command = Command::new("tool");
        command.args(["a b", "say \"hi\"", r"C:\path\", ""]);
        assert_eq!(
            display(&command),
            r#"tool "a b" "say \"hi\"" "C:\path\\" """#
        );
    }

    #[test]
    fn availability_distinguishes_real_programs_from_invented_ones() {
        assert!(available("git"));
        assert!(!available("wombat-cc-definitely-not-a-real-program"));
    }

    #[test]
    fn git_runs_inside_the_requested_directory() {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), ["init"], false).unwrap();
        assert!(directory.path().join(".git").is_dir());
    }
}
