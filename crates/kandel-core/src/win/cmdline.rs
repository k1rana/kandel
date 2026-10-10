//! Windows command-line quoting.
//!
//! Ported to match the behavior of Go's `windows.EscapeArg`, which follows the
//! parsing rules of `CommandLineToArgvW`. The command line handed to
//! `CreateProcessW` is built by escaping the executable and each argument and
//! joining them with spaces.

/// Quotes a single argument for inclusion in a Windows command line.
///
/// * An empty string becomes `""`.
/// * A string with no spaces, tabs, or quotes is returned unchanged.
/// * Otherwise the string is wrapped in double quotes, and runs of backslashes
///   immediately before a quote or the closing quote are doubled.
pub fn escape_arg(arg: &str) -> String {
    let needs_quote = arg.is_empty() || arg.chars().any(|c| c == ' ' || c == '\t' || c == '"');

    if !needs_quote {
        return arg.to_string();
    }

    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');

    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Double the backslashes, then escape the quote itself.
                for _ in 0..backslashes {
                    out.push('\\');
                }
                out.push('\\');
                out.push('"');
                backslashes = 0;
            }
            _ => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(c);
            }
        }
    }

    // Backslashes immediately before the closing quote must be doubled:
    // write the original run, then an identical run, then the quote.
    for _ in 0..backslashes {
        out.push('\\');
    }
    for _ in 0..backslashes {
        out.push('\\');
    }
    out.push('"');
    out
}

/// Joins a program and its arguments into a command line.
pub fn build_command_line(program: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(escape_arg(program));
    for arg in args {
        parts.push(escape_arg(arg));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_arg_unchanged() {
        assert_eq!(escape_arg("-v"), "-v");
        assert_eq!(escape_arg(r"C:\bin\app.exe"), r"C:\bin\app.exe");
    }

    #[test]
    fn empty_arg_is_quoted() {
        assert_eq!(escape_arg(""), r#""""#);
    }

    #[test]
    fn space_is_quoted() {
        assert_eq!(escape_arg("hello world"), r#""hello world""#);
    }

    #[test]
    fn embedded_quote_is_escaped() {
        assert_eq!(escape_arg(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn backslash_is_literal_outside_quotes() {
        // Trailing backslashes are preserved verbatim when no quoting is needed
        // (matches Go's `windows.EscapeArg`).
        assert_eq!(escape_arg(r"C:\dir\"), r"C:\dir\");
    }

    #[test]
    fn trailing_backslash_is_doubled_when_quoted() {
        // A trailing backslash before a closing quote must be doubled, which
        // only happens when the argument is quoted (here: it contains a space).
        assert_eq!(escape_arg(r"C:\Program Files\"), r#""C:\Program Files\\""#);
    }

    #[test]
    fn backslash_before_quote_is_doubled() {
        // One pending backslash before an embedded quote yields `\\"`.
        assert_eq!(escape_arg(r#"a\"b"#), r#""a\\"b""#);
    }

    #[test]
    fn tab_is_quoted() {
        assert_eq!(escape_arg("a\tb"), "\"a\tb\"");
    }

    #[test]
    fn build_command_line_joins_with_spaces() {
        let args = vec!["/c".to_string(), "echo hello".to_string()];
        assert_eq!(
            build_command_line("cmd.exe", &args),
            r#"cmd.exe /c "echo hello""#
        );
    }

    #[test]
    fn build_command_line_handles_no_args() {
        assert_eq!(build_command_line("app.exe", &[]), "app.exe");
    }

    #[test]
    fn build_command_line_quotes_program_with_spaces() {
        let args = vec!["--use-pt-config".to_string()];
        assert_eq!(
            build_command_line(r"C:\Program Files\App\app.exe", &args),
            r#""C:\Program Files\App\app.exe" --use-pt-config"#
        );
    }
}
