//! Text we put in the **model-facing** tool result when output was spilled to a file.
//! Port of `getLargeOutputInstructions` from `src/utils/mcpOutputStorage.ts`.
//!
//! **Principle:** large payloads must **not** be inlined into the agent context. Persist to
//! a file, inject only a short instruction block, and have the model **read in chunks**
//! (`offset` / `limit`) and pull in **relevant** sections for the task (same contract as TS).

/// US-style thousands separators (matches `Number.prototype.toLocaleString('en-US')` for integers).
#[must_use]
pub fn format_us_integer(n: usize) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let len = chars.len();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    out
}

/// Instruction text telling the model to read saved output from disk in sequential chunks,
/// not to treat the full blob as already in context.
#[must_use]
pub fn get_large_output_instructions(
    raw_output_path: &str,
    content_length: usize,
    format_description: &str,
    max_read_length: Option<usize>,
) -> String {
    let n = format_us_integer(content_length);
    let mut base = format!(
        "Error: result ({n} characters) exceeds maximum allowed tokens. Output has been saved to {path}.\n\
Format: {fmt}\n\
Use offset and limit parameters to read specific portions of the file, search within it for specific content, and jq to make structured queries.\n\
REQUIREMENTS FOR SUMMARIZATION/ANALYSIS/REVIEW:\n\
- You MUST read the content from the file at {path} in sequential chunks until 100% of the content has been read.\n",
        path = raw_output_path,
        fmt = format_description,
    );

    if let Some(max) = max_read_length {
        base.push_str(&format!(
            "- If you receive truncation warnings when reading the file (\"[N lines truncated]\"), reduce the chunk size until you have read 100% of the content without truncation ***DO NOT PROCEED UNTIL YOU HAVE DONE THIS***. Bash output is limited to {} chars.\n",
            format_us_integer(max)
        ));
    } else {
        base.push_str(
            "- If you receive truncation warnings when reading the file, reduce the chunk size until you have read 100% of the content without truncation.\n",
        );
    }

    base.push_str(
        "- Before producing ANY summary or analysis, you MUST explicitly describe what portion of the content you have read. ***If you did not read the entire content, you MUST explicitly state this.***\n",
    );

    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_large_output_instructions_contains_path_and_chunk_guidance() {
        let t = get_large_output_instructions(
            "/tmp/session/tool-results/x.txt",
            999_999,
            "Plain text",
            None,
        );
        assert!(t.contains("/tmp/session/tool-results/x.txt"));
        assert!(t.contains("offset and limit"));
        assert!(t.contains("sequential chunks"));
        assert!(t.contains("999,999"));
    }
}
