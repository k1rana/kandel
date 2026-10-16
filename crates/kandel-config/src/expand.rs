//! One-pass placeholder expansion for Kandel templates.
//!
//! Supported syntax:
//!
//! * `${NAME}` — replaced by the value returned from the lookup function.
//! * `$$` — produces a literal `$`.
//! * Any other `$` is a literal `$`. Only `${` begins a placeholder, so a bare
//!   `$` (a regex anchor like `^(a|b)$`, a price, or an unbraced `$HOME`) needs
//!   no escaping.
//! * An unterminated `${` is a syntax error.
//! * An empty `${}` is a syntax error.
//!
//! Expansion is **non-recursive**: values returned by the lookup are inserted
//! verbatim and are never re-scanned for placeholders.

/// Error produced by [`expand`] and friends.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpandError {
    /// A `${` was not terminated by `}`.
    #[error("unterminated ${{ in input")]
    UnterminatedBrace,
    /// A placeholder had no name: `${}`.
    #[error("empty ${{}} placeholder")]
    EmptyBraces,
    /// A placeholder referenced a name the lookup did not define.
    #[error("undefined variable: ${{{0}}}")]
    Undefined(String),
}

/// Performs one-pass placeholder expansion on `s`, resolving names with `lookup`.
///
/// `lookup` returns `Some(value)` for a defined name and `None` otherwise; a
/// `None` result produces [`ExpandError::Undefined`].
pub fn expand<F>(s: &str, lookup: F) -> Result<String, ExpandError>
where
    F: Fn(&str) -> Option<String>,
{
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        if c != b'$' {
            out.push(c);
            i += 1;
            continue;
        }

        // A `$` is only special as `$$` (a literal dollar) or `${...}` (a
        // placeholder). Anywhere else it is literal, so a regex anchor such as
        // `^(a|b)$` or an unbraced `$HOME` needs no escaping.
        match bytes.get(i + 1).copied() {
            Some(b'$') => {
                out.push(b'$');
                i += 2;
            }
            Some(b'{') => {
                // ${...}
                let rest = &bytes[i + 2..];
                let Some(end) = rest.iter().position(|&b| b == b'}') else {
                    return Err(ExpandError::UnterminatedBrace);
                };
                let name = std::str::from_utf8(&rest[..end])
                    .map_err(|_| ExpandError::UnterminatedBrace)?;
                if name.is_empty() {
                    return Err(ExpandError::EmptyBraces);
                }
                let value = lookup(name).ok_or_else(|| ExpandError::Undefined(name.to_string()))?;
                out.extend_from_slice(value.as_bytes());
                i += 2 + end + 1;
            }
            _ => {
                out.push(b'$');
                i += 1;
            }
        }
    }

    // Input plus inserted lookup values are both valid UTF-8, so this is infallible.
    String::from_utf8(out).map_err(|_| ExpandError::UnterminatedBrace)
}

/// Expands `${NAME}` using environment variables and converts `$$` to `$`.
///
/// The reserved name `${PID}` is rejected here; it is only meaningful inside
/// `stop_args`, where [`expand_env_with_pid`] is used instead.
pub fn expand_env(s: &str) -> Result<String, ExpandError> {
    expand(s, |name| {
        if name == "PID" {
            return None;
        }
        std::env::var(name).ok()
    })
}

/// Like [`expand_env`] but additionally expands `${PID}` to `pid`.
pub fn expand_env_with_pid(s: &str, pid: u32) -> Result<String, ExpandError> {
    expand(s, |name| {
        if name == "PID" {
            return Some(pid.to_string());
        }
        std::env::var(name).ok()
    })
}

/// Validates that `s` is a syntactically well-formed template without
/// substituting any real values.
///
/// `${PID}` is accepted only when `allow_pid` is `true`; every other variable
/// is treated as defined for syntax-only validation.
pub fn validate_template(s: &str, allow_pid: bool) -> Result<(), ExpandError> {
    expand(s, |name| {
        if name == "PID" {
            if allow_pid {
                return Some(String::new());
            }
            return None;
        }
        Some(String::new())
    })
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn literal_passthrough() {
        assert_eq!(expand("hello world", none).unwrap(), "hello world");
    }

    #[test]
    fn dollar_escape() {
        assert_eq!(expand("price: $$5", none).unwrap(), "price: $5");
    }

    #[test]
    fn escaped_placeholder() {
        let got = expand("$${PID}", |name| {
            if name == "PID" {
                Some("999".into())
            } else {
                None
            }
        })
        .unwrap();
        // $$ -> $, then {PID} is literal (no leading $).
        assert_eq!(got, "${PID}");
    }

    #[test]
    fn dollar_then_placeholder() {
        let got = expand("$$${PID}", |name| {
            if name == "PID" {
                Some("42".into())
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(got, "$42");
    }

    #[test]
    fn placeholder() {
        let got = expand("home=${HOME}/bin", |name| {
            if name == "HOME" {
                Some(r"C:\Users\alice".into())
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(got, r"home=C:\Users\alice/bin");
    }

    #[test]
    fn undefined_variable() {
        let err = expand("${NOPE}", none).unwrap_err();
        assert!(err.to_string().contains("NOPE"), "{err}");
    }

    #[test]
    fn unterminated_brace() {
        assert_eq!(
            expand("${HOME", |_| Some("x".into())).unwrap_err(),
            ExpandError::UnterminatedBrace
        );
    }

    #[test]
    fn empty_braces() {
        assert_eq!(
            expand("${}", |_| Some(String::new())).unwrap_err(),
            ExpandError::EmptyBraces
        );
    }

    #[test]
    fn bare_dollar_is_literal() {
        assert_eq!(expand("price $5", none).unwrap(), "price $5");
        assert_eq!(expand("trailing$", none).unwrap(), "trailing$");
        assert_eq!(expand("$HOME", none).unwrap(), "$HOME");
    }

    #[test]
    fn regex_anchor_is_literal() {
        let regex = "^(alpha|beta_.*|gamma)$";
        assert_eq!(expand(regex, none).unwrap(), regex);
    }

    #[test]
    fn non_recursive() {
        // Expanded values must NOT be re-scanned for placeholders.
        let got = expand("${FOO}", |name| {
            assert_eq!(name, "FOO", "unexpected lookup for {name}");
            Some("${BAR}".into())
        })
        .unwrap();
        assert_eq!(got, "${BAR}");
    }

    #[test]
    fn expand_env_rejects_pid() {
        assert!(expand_env("prefix-${PID}").is_err());
    }

    #[test]
    fn expand_env_looks_up_env() {
        std::env::set_var("KANDEL_TEST_VAR", "beacon");
        let got = expand_env("x=${KANDEL_TEST_VAR}").unwrap();
        std::env::remove_var("KANDEL_TEST_VAR");
        assert_eq!(got, "x=beacon");
    }

    #[test]
    fn expand_env_with_pid_expands_pid() {
        assert_eq!(expand_env_with_pid("pid=${PID}", 4242).unwrap(), "pid=4242");
    }

    #[test]
    fn validate_template_allows_pid() {
        assert!(validate_template("${PID}", true).is_ok());
        assert!(validate_template("${PID}", false).is_err());
    }

    #[test]
    fn validate_template_catches_syntax_errors() {
        assert!(validate_template("${unterminated", false).is_err());
        assert!(validate_template("${}", false).is_err());
    }

    #[test]
    fn validate_template_allows_bare_dollar() {
        assert!(validate_template("oops $", false).is_ok());
        assert!(validate_template("^(a|b)$", false).is_ok());
    }
}
