pub(crate) fn command_with_log_capture(argv: &[String]) -> String {
    let rendered = argv
        .iter()
        .map(|arg| sh_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "set +e\n{rendered} > logs/stdout.txt 2> logs/stderr.txt\ncode=$?\nprintf '\\n__OMIGA_OPERATOR_EXIT_CODE=%s\\n' \"$code\"\nexit \"$code\""
    )
}

pub(crate) fn shell_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| sh_quote(token))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
