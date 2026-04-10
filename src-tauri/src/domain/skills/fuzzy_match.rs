//! Fuzzy text matching for skill file patching.
//!
//! Implements an 8-strategy matching chain (inspired by Hermes / OpenCode),
//! tried in order until a match is found:
//!
//! 1. Exact                  — direct string comparison
//! 2. Line-trimmed           — strip leading/trailing whitespace per line
//! 3. Whitespace-normalized  — collapse multiple spaces/tabs to single space
//! 4. Indentation-flexible   — ignore all leading whitespace
//! 5. Escape-normalized      — convert \\n, \\t literals to real chars
//! 6. Trimmed-boundary       — trim only the first and last line
//! 7. Block-anchor           — match first+last line; similarity check for middle
//! 8. Context-aware          — ≥50% of lines have ≥80% character similarity

type ByteRange = (usize, usize);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Find and replace text using a chain of increasingly fuzzy strategies.
///
/// Returns `Ok((new_content, replacement_count))` or `Err(human-readable reason)`.
pub fn fuzzy_find_and_replace(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<(String, usize), String> {
    if old_string.is_empty() {
        return Err("old_string cannot be empty".to_string());
    }
    if old_string == new_string {
        return Err("old_string and new_string are identical".to_string());
    }

    type StrategyFn = fn(&str, &str) -> Vec<ByteRange>;
    let strategies: &[(&str, StrategyFn)] = &[
        ("exact", strategy_exact),
        ("line_trimmed", strategy_line_trimmed),
        ("whitespace_normalized", strategy_whitespace_normalized),
        ("indentation_flexible", strategy_indentation_flexible),
        ("escape_normalized", strategy_escape_normalized),
        ("trimmed_boundary", strategy_trimmed_boundary),
        ("block_anchor", strategy_block_anchor),
        ("context_aware", strategy_context_aware),
    ];

    for (_name, strategy_fn) in strategies {
        let matches = strategy_fn(content, old_string);
        if matches.is_empty() {
            continue;
        }
        if matches.len() > 1 && !replace_all {
            return Err(format!(
                "Found {} matches for old_string. \
                 Provide more context to make it unique, or set replace_all=true.",
                matches.len()
            ));
        }
        let count = matches.len();
        let new_content = apply_replacements(content, matches, new_string);
        return Ok((new_content, count));
    }

    Err("Could not find a match for old_string in the file".to_string())
}

// ---------------------------------------------------------------------------
// Core helpers
// ---------------------------------------------------------------------------

fn apply_replacements(content: &str, mut matches: Vec<ByteRange>, new_string: &str) -> String {
    // Replace from end to start to preserve earlier byte positions
    matches.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let mut result = content.to_string();
    for (start, end) in matches {
        result.replace_range(start..end, new_string);
    }
    result
}

/// Returns the byte offset of the start of each line.
fn line_start_offsets(content: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

/// Byte range in `content` for lines `[start_line, end_line)`.
/// Does NOT include the trailing `\n` of the last matched line.
fn lines_byte_range(
    content: &str,
    offsets: &[usize],
    start_line: usize,
    end_line: usize,
) -> ByteRange {
    let start = offsets[start_line];
    let end = if end_line < offsets.len() {
        offsets[end_line].saturating_sub(1) // byte just before the \n of next line
    } else {
        content.len()
    };
    (start, end)
}

/// Search `content_norm_lines` for a consecutive sequence matching `pat_norm_lines`.
/// Returns byte ranges in the ORIGINAL `content`.
fn find_line_pattern(
    content: &str,
    offsets: &[usize],
    content_norm_lines: &[String],
    pat_norm_lines: &[String],
) -> Vec<ByteRange> {
    let n = pat_norm_lines.len();
    if n == 0 || n > content_norm_lines.len() {
        return vec![];
    }
    let mut matches = vec![];
    for i in 0..=content_norm_lines.len() - n {
        if content_norm_lines[i..i + n] == pat_norm_lines[..] {
            matches.push(lines_byte_range(content, offsets, i, i + n));
        }
    }
    matches
}

// ---------------------------------------------------------------------------
// Strategy 1: Exact match
// ---------------------------------------------------------------------------

fn strategy_exact(content: &str, pattern: &str) -> Vec<ByteRange> {
    let mut matches = vec![];
    let mut start = 0;
    while start < content.len() {
        match content[start..].find(pattern) {
            None => break,
            Some(pos) => {
                let abs = start + pos;
                matches.push((abs, abs + pattern.len()));
                start = abs + 1;
            }
        }
    }
    matches
}

// ---------------------------------------------------------------------------
// Strategy 2: Line-trimmed (strip each line)
// ---------------------------------------------------------------------------

fn strategy_line_trimmed(content: &str, pattern: &str) -> Vec<ByteRange> {
    let offsets = line_start_offsets(content);
    let content_norm: Vec<String> =
        content.split('\n').map(|l| l.trim().to_string()).collect();
    let pat_norm: Vec<String> =
        pattern.split('\n').map(|l| l.trim().to_string()).collect();
    find_line_pattern(content, &offsets, &content_norm, &pat_norm)
}

// ---------------------------------------------------------------------------
// Strategy 3: Whitespace-normalized (collapse spaces/tabs)
// ---------------------------------------------------------------------------

fn collapse_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' || ch == '\t' {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(ch);
            prev_space = false;
        }
    }
    result
}

fn strategy_whitespace_normalized(content: &str, pattern: &str) -> Vec<ByteRange> {
    let offsets = line_start_offsets(content);
    let content_norm: Vec<String> =
        content.split('\n').map(|l| collapse_whitespace(l)).collect();
    let pat_norm: Vec<String> =
        pattern.split('\n').map(|l| collapse_whitespace(l)).collect();
    find_line_pattern(content, &offsets, &content_norm, &pat_norm)
}

// ---------------------------------------------------------------------------
// Strategy 4: Indentation-flexible (strip leading whitespace)
// ---------------------------------------------------------------------------

fn strategy_indentation_flexible(content: &str, pattern: &str) -> Vec<ByteRange> {
    let offsets = line_start_offsets(content);
    let content_norm: Vec<String> =
        content.split('\n').map(|l| l.trim_start().to_string()).collect();
    let pat_norm: Vec<String> =
        pattern.split('\n').map(|l| l.trim_start().to_string()).collect();
    find_line_pattern(content, &offsets, &content_norm, &pat_norm)
}

// ---------------------------------------------------------------------------
// Strategy 5: Escape-normalized (convert \n \t literals to actual chars)
// ---------------------------------------------------------------------------

fn unescape_literals(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r")
}

fn strategy_escape_normalized(content: &str, pattern: &str) -> Vec<ByteRange> {
    let unescaped = unescape_literals(pattern);
    if unescaped == pattern {
        return vec![]; // No escape sequences — skip this strategy
    }
    strategy_exact(content, &unescaped)
}

// ---------------------------------------------------------------------------
// Strategy 6: Trimmed-boundary (trim only first and last line)
// ---------------------------------------------------------------------------

fn strategy_trimmed_boundary(content: &str, pattern: &str) -> Vec<ByteRange> {
    let pat_lines: Vec<&str> = pattern.split('\n').collect();
    let n = pat_lines.len();
    if n == 0 {
        return vec![];
    }

    let first_trimmed = pat_lines[0].trim();
    let last_trimmed = pat_lines[n - 1].trim();

    let offsets = line_start_offsets(content);
    let content_lines: Vec<&str> = content.split('\n').collect();
    if n > content_lines.len() {
        return vec![];
    }

    let mut matches = vec![];
    for i in 0..=content_lines.len() - n {
        let block = &content_lines[i..i + n];

        if block[0].trim() != first_trimmed {
            continue;
        }
        if n > 1 && block[n - 1].trim() != last_trimmed {
            continue;
        }
        // Middle lines must match exactly
        if n > 2 && block[1..n - 1] != pat_lines[1..n - 1] {
            continue;
        }
        matches.push(lines_byte_range(content, &offsets, i, i + n));
    }
    matches
}

// ---------------------------------------------------------------------------
// Strategy 7: Block-anchor (first+last exact, middle by similarity)
// ---------------------------------------------------------------------------

/// Approximate SequenceMatcher.ratio() via bag-of-chars matching.
/// Returns a value in [0.0, 1.0].
fn similarity_ratio(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    if a_len == 0 && b_len == 0 {
        return 1.0;
    }
    if a_len == 0 || b_len == 0 {
        return 0.0;
    }
    let mut freq: std::collections::HashMap<char, i32> = std::collections::HashMap::new();
    for c in a.chars() {
        *freq.entry(c).or_insert(0) += 1;
    }
    let mut matched = 0i32;
    let mut used: std::collections::HashMap<char, i32> = std::collections::HashMap::new();
    for c in b.chars() {
        let u = used.entry(c).or_insert(0);
        if *u < freq.get(&c).copied().unwrap_or(0) {
            matched += 1;
            *u += 1;
        }
    }
    (2 * matched) as f64 / (a_len + b_len) as f64
}

fn unicode_normalize(s: &str) -> String {
    s.replace('\u{201c}', "\"")
        .replace('\u{201d}', "\"")
        .replace('\u{2018}', "'")
        .replace('\u{2019}', "'")
        .replace('\u{2014}', "--")
        .replace('\u{2013}', "-")
        .replace('\u{2026}', "...")
        .replace('\u{00a0}', " ")
}

fn strategy_block_anchor(content: &str, pattern: &str) -> Vec<ByteRange> {
    let pat_norm = unicode_normalize(pattern);
    let content_norm = unicode_normalize(content);

    let pat_lines: Vec<&str> = pat_norm.split('\n').collect();
    let n = pat_lines.len();
    if n < 2 {
        return vec![];
    }

    let first_line = pat_lines[0].trim();
    let last_line = pat_lines[n - 1].trim();

    let content_norm_lines: Vec<&str> = content_norm.split('\n').collect();
    let offsets = line_start_offsets(content);

    if n > content_norm_lines.len() {
        return vec![];
    }

    let candidates: Vec<usize> = (0..=content_norm_lines.len() - n)
        .filter(|&i| {
            content_norm_lines[i].trim() == first_line
                && content_norm_lines[i + n - 1].trim() == last_line
        })
        .collect();

    let threshold = if candidates.len() == 1 { 0.10_f64 } else { 0.30_f64 };
    let mut matches = vec![];

    for i in candidates {
        let sim = if n <= 2 {
            1.0
        } else {
            let content_mid = content_norm_lines[i + 1..i + n - 1].join("\n");
            let pat_mid = pat_lines[1..n - 1].join("\n");
            similarity_ratio(&content_mid, &pat_mid)
        };
        if sim >= threshold {
            matches.push(lines_byte_range(content, &offsets, i, i + n));
        }
    }
    matches
}

// ---------------------------------------------------------------------------
// Strategy 8: Context-aware (≥50% of lines with ≥80% character similarity)
// ---------------------------------------------------------------------------

fn strategy_context_aware(content: &str, pattern: &str) -> Vec<ByteRange> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let pat_lines: Vec<&str> = pattern.split('\n').collect();
    let n = pat_lines.len();
    let offsets = line_start_offsets(content);

    if n == 0 || n > content_lines.len() {
        return vec![];
    }

    let mut matches = vec![];
    for i in 0..=content_lines.len() - n {
        let block = &content_lines[i..i + n];
        let high_sim = block
            .iter()
            .zip(pat_lines.iter())
            .filter(|(c, p)| similarity_ratio(c.trim(), p.trim()) >= 0.80)
            .count();
        if high_sim as f64 >= n as f64 * 0.5 {
            matches.push(lines_byte_range(content, &offsets, i, i + n));
        }
    }
    matches
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_replace() {
        let (out, n) =
            fuzzy_find_and_replace("Hello WORLD", "WORLD", "Omiga", false).unwrap();
        assert_eq!(out, "Hello Omiga");
        assert_eq!(n, 1);
    }

    #[test]
    fn line_trimmed_strips_indentation() {
        let content = "    fn foo() {\n        pass\n    }";
        let pattern = "fn foo() {\n    pass\n}";
        let (out, n) =
            fuzzy_find_and_replace(content, pattern, "fn bar() {\n    return\n}", false)
                .unwrap();
        assert_eq!(n, 1, "expected 1 replacement");
        assert!(out.contains("fn bar()"), "replacement not applied: {out}");
    }

    #[test]
    fn indentation_flexible_ignores_leading_spaces() {
        let content = "  hello\n  world";
        let pattern = "hello\nworld";
        let (out, n) =
            fuzzy_find_and_replace(content, pattern, "foo\nbar", false).unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("foo"));
    }

    #[test]
    fn whitespace_normalized_collapses_spaces() {
        let content = "x  =  1\ny  =  2";
        let pattern = "x = 1\ny = 2";
        let (out, n) =
            fuzzy_find_and_replace(content, pattern, "x = 10\ny = 20", false).unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("10"));
    }

    #[test]
    fn not_found_returns_err() {
        let err = fuzzy_find_and_replace("hello", "xyz", "abc", false).unwrap_err();
        assert!(err.contains("Could not find"), "wrong error: {err}");
    }

    #[test]
    fn multiple_exact_without_replace_all_errors() {
        let err = fuzzy_find_and_replace("aa bb aa", "aa", "cc", false).unwrap_err();
        assert!(err.contains("2 matches"), "wrong error: {err}");
    }

    #[test]
    fn replace_all_replaces_every_occurrence() {
        let (out, n) =
            fuzzy_find_and_replace("aa bb aa", "aa", "cc", true).unwrap();
        assert_eq!(out, "cc bb cc");
        assert_eq!(n, 2);
    }

    #[test]
    fn identical_old_new_returns_err() {
        let err = fuzzy_find_and_replace("hello", "hello", "hello", false).unwrap_err();
        assert!(err.contains("identical"));
    }

    #[test]
    fn trimmed_boundary_handles_trailing_space() {
        let content = "  foo  \n  bar\n  baz  ";
        let pattern = "foo\n  bar\nbaz";
        let (out, n) =
            fuzzy_find_and_replace(content, pattern, "X\n  Y\nZ", false).unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("X"), "got: {out}");
    }

    #[test]
    fn block_anchor_matches_similar_middle() {
        let content = "START\nhello world here\nEND";
        let pattern = "START\nhello world there\nEND"; // middle differs slightly
        let (out, n) =
            fuzzy_find_and_replace(content, pattern, "START\nnew middle\nEND", false)
                .unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("new middle"), "got: {out}");
    }

    #[test]
    fn multiline_exact_replace() {
        let content = "line1\nTARGET_A\nTARGET_B\nline4";
        let pattern = "TARGET_A\nTARGET_B";
        let (out, n) =
            fuzzy_find_and_replace(content, pattern, "REPLACED_A\nREPLACED_B", false)
                .unwrap();
        assert_eq!(n, 1);
        assert_eq!(out, "line1\nREPLACED_A\nREPLACED_B\nline4");
    }
}
