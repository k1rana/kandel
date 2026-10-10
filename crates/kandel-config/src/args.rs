//! Shell-like argument string parsing and formatting.
//!
//! The TOML config stores arguments as an argv array, but the management UI
//! edits them as a single shell-like string. These helpers convert between the
//! two representations.

/// Error produced by [`parse_args`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseArgsError {
    /// A single quote was opened but never closed.
    #[error("unterminated single quote in args string")]
    UnterminatedSingleQuote,
    /// A double quote was opened but never closed.
    #[error("unterminated double quote in args string")]
    UnterminatedDoubleQuote,
}

/// Splits a shell-like argument string into individual arguments.
///
/// Respects single- and double-quoted strings, and backslash escapes inside
/// double-quoted strings. Only `\` inside double quotes is special; elsewhere
/// it is a literal character (important for Windows paths).
///
/// # Examples
///
/// ```text
/// parse_args("-v --debug")            => ["-v", "--debug"]
/// parse_args("-c \"hello world\"")     => ["-c", "hello world"]
/// ```
pub fn parse_args(s: &str) -> Result<Vec<String>, ParseArgsError> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for r in s.chars() {
        if escaped {
            current.push(r);
            escaped = false;
            continue;
        }
        if r == '\\' && in_double {
            escaped = true;
            continue;
        }
        if r == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if r == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if r.is_whitespace() && !in_single && !in_double {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(r);
    }

    if in_single {
        return Err(ParseArgsError::UnterminatedSingleQuote);
    }
    if in_double {
        return Err(ParseArgsError::UnterminatedDoubleQuote);
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

/// Joins arguments into a single shell-safe string for display.
///
/// Arguments containing spaces or shell-significant characters are
/// double-quoted, and any internal double-quotes and backslashes are escaped.
pub fn format_args(args: &[String]) -> String {
    args.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(s: &str) -> String {
    let needs_quote = s
        .chars()
        .any(|r| r.is_whitespace() || "\"'\\&|;<>()$".contains(r));

    if !needs_quote && !s.is_empty() {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for r in s.chars() {
        if r == '"' || r == '\\' {
            out.push('\\');
        }
        out.push(r);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_args() {
        assert_eq!(parse_args("-v --debug").unwrap(), ["-v", "--debug"]);
        assert_eq!(
            parse_args(r#"-c "hello world""#).unwrap(),
            ["-c", "hello world"]
        );
    }

    #[test]
    fn parse_collapses_whitespace() {
        assert_eq!(parse_args("  -a   -b\t-c ").unwrap(), ["-a", "-b", "-c"]);
    }

    #[test]
    fn parse_double_quotes_group() {
        assert_eq!(
            parse_args(r#"-c "hello world""#).unwrap(),
            ["-c", "hello world"]
        );
    }

    #[test]
    fn parse_single_quotes_group() {
        assert_eq!(
            parse_args("-c 'hello world'").unwrap(),
            ["-c", "hello world"]
        );
    }

    #[test]
    fn parse_empty_quotes_are_dropped() {
        // Matches the reference parser: an empty quoted token produces no
        // argument (only tokens with content are pushed).
        assert_eq!(parse_args(r#"-c """#).unwrap(), ["-c"]);
    }

    #[test]
    fn parse_backslash_literal_outside_double_quotes() {
        // Windows paths must survive unquoted.
        assert_eq!(
            parse_args(r"C:\Users\alice\app.exe").unwrap(),
            [r"C:\Users\alice\app.exe"]
        );
    }

    #[test]
    fn parse_backslash_escapes_inside_double_quotes() {
        assert_eq!(parse_args(r#""a\"b""#).unwrap(), ["a\"b".to_string()]);
    }

    #[test]
    fn parse_unterminated_single_quote() {
        assert_eq!(
            parse_args("'oops").unwrap_err(),
            ParseArgsError::UnterminatedSingleQuote
        );
    }

    #[test]
    fn parse_unterminated_double_quote() {
        assert_eq!(
            parse_args(r#""oops"#).unwrap_err(),
            ParseArgsError::UnterminatedDoubleQuote
        );
    }

    #[test]
    fn format_plain() {
        let args = vec!["-v".to_string(), "--debug".to_string()];
        assert_eq!(format_args(&args), "-v --debug");
    }

    #[test]
    fn format_quotes_arguments_with_spaces() {
        let args = vec!["-c".to_string(), "hello world".to_string()];
        assert_eq!(format_args(&args), r#"-c "hello world""#);
    }

    #[test]
    fn format_preserves_empty_argument() {
        let args = vec![String::new()];
        assert_eq!(format_args(&args), r#""""#);
    }

    #[test]
    fn round_trip() {
        let original = vec![
            "-c".to_string(),
            "echo hello".to_string(),
            r"C:\path with space\file.txt".to_string(),
        ];
        let formatted = format_args(&original);
        assert_eq!(parse_args(&formatted).unwrap(), original);
    }

    #[test]
    fn round_trip_with_special_characters() {
        let original = vec!["a&b|c".to_string(), "x>y<z".to_string(), "$VAR".to_string()];
        let formatted = format_args(&original);
        assert_eq!(parse_args(&formatted).unwrap(), original);
    }
}
