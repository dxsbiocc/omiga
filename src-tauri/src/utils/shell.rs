//! Shell quoting helpers shared across execution backends.

/// POSIX single-quote a value for safe embedding in shell commands.
/// Any embedded single-quote is escaped via the `'"'"'` idiom.
pub fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_string_wrapped_in_single_quotes() {
        assert_eq!(shell_single_quote("hello"), "'hello'");
    }

    #[test]
    fn empty_string_becomes_empty_quotes() {
        assert_eq!(shell_single_quote(""), "''");
    }

    #[test]
    fn single_quote_is_escaped_with_idiom() {
        // "it's" → 'it'"'"'s'
        assert_eq!(shell_single_quote("it's"), "'it'\"'\"'s'");
    }

    #[test]
    fn path_with_spaces_is_safe() {
        assert_eq!(shell_single_quote("/home/user/my dir"), "'/home/user/my dir'");
    }

    #[test]
    fn shell_metacharacters_are_neutralised() {
        // Semicolon, dollar, backtick — all inert inside single quotes
        let result = shell_single_quote("rm -rf; $HOME `id`");
        assert!(result.starts_with('\''));
        assert!(result.ends_with('\''));
        assert!(result.contains("rm -rf; $HOME `id`"));
    }

    #[test]
    fn multiple_single_quotes_all_escaped() {
        // "a'b'c" → 'a'"'"'b'"'"'c'
        assert_eq!(shell_single_quote("a'b'c"), "'a'\"'\"'b'\"'\"'c'");
    }

    #[test]
    fn conda_env_name_with_no_specials() {
        assert_eq!(shell_single_quote("my_env"), "'my_env'");
    }

    #[test]
    fn path_with_tilde_is_not_expanded() {
        // tilde inside single quotes is literal, not expanded
        let result = shell_single_quote("~/project/.venv");
        assert_eq!(result, "'~/project/.venv'");
    }
}
