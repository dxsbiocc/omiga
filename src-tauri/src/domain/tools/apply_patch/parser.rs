use std::fmt;

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const ADD: &str = "*** Add File: ";
const UPDATE: &str = "*** Update File: ";
const DELETE: &str = "*** Delete File: ";
const EOF_MARKER: &str = "*** End of File";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub hunks: Vec<FileHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileHunk {
    Add {
        path: String,
        contents: String,
    },
    Update {
        path: String,
        chunks: Vec<UpdateChunk>,
    },
    Delete {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateChunk {
    pub change_context: Option<String>,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub is_end_of_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse_patch(text: &str) -> Result<Patch, ParseError> {
    let lines: Vec<&str> = text.trim().lines().collect();
    if lines.first().map(|line| line.trim()) != Some(BEGIN) {
        return Err(ParseError::new(1, "first line must be '*** Begin Patch'"));
    }
    if lines.last().map(|line| line.trim()) != Some(END) {
        return Err(ParseError::new(
            lines.len().max(1),
            "last line must be '*** End Patch'",
        ));
    }

    let mut parser = Parser {
        lines,
        index: 1,
        hunks: Vec::new(),
    };
    parser.parse_body()?;
    Ok(Patch {
        hunks: parser.hunks,
    })
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    index: usize,
    hunks: Vec<FileHunk>,
}

impl Parser<'_> {
    fn parse_body(&mut self) -> Result<(), ParseError> {
        while self.index + 1 < self.lines.len() {
            let line = self.current();
            if let Some(path) = line.strip_prefix(ADD) {
                self.parse_add(path.trim().to_string())?;
            } else if let Some(path) = line.strip_prefix(UPDATE) {
                self.parse_update(path.trim().to_string())?;
            } else if let Some(path) = line.strip_prefix(DELETE) {
                self.parse_delete(path.trim().to_string())?;
            } else if line.trim().is_empty() {
                self.index += 1;
            } else {
                return Err(ParseError::new(self.line_no(), "expected file hunk header"));
            }
        }
        Ok(())
    }

    fn parse_add(&mut self, path: String) -> Result<(), ParseError> {
        let header_line = self.line_no();
        validate_path_text(&path, header_line)?;
        self.index += 1;

        let mut contents = Vec::new();
        while self.index + 1 < self.lines.len() && !is_file_header(self.current()) {
            let line = self.current();
            let Some(content) = line.strip_prefix('+') else {
                return Err(ParseError::new(
                    self.line_no(),
                    "Add File lines must start with '+'",
                ));
            };
            contents.push(content.to_string());
            self.index += 1;
        }

        let contents = if contents.is_empty() {
            String::new()
        } else {
            let mut text = contents.join("\n");
            text.push('\n');
            text
        };
        self.hunks.push(FileHunk::Add { path, contents });
        Ok(())
    }

    fn parse_update(&mut self, path: String) -> Result<(), ParseError> {
        let header_line = self.line_no();
        validate_path_text(&path, header_line)?;
        self.index += 1;

        let mut chunks = Vec::new();
        while self.index + 1 < self.lines.len() && !is_file_header(self.current()) {
            let line = self.current();
            if !line.starts_with("@@") {
                return Err(ParseError::new(
                    self.line_no(),
                    "Update File hunks must start with '@@'",
                ));
            }
            chunks.push(self.parse_update_chunk()?);
        }

        if chunks.is_empty() {
            return Err(ParseError::new(
                header_line,
                format!("Update file hunk for path '{path}' is empty"),
            ));
        }
        self.hunks.push(FileHunk::Update { path, chunks });
        Ok(())
    }

    fn parse_update_chunk(&mut self) -> Result<UpdateChunk, ParseError> {
        let header = self.current();
        let change_context = header
            .strip_prefix("@@")
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(ToOwned::to_owned);
        self.index += 1;

        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();
        let mut is_end_of_file = false;

        while self.index + 1 < self.lines.len()
            && !is_file_header(self.current())
            && !self.current().starts_with("@@")
        {
            let line = self.current();
            if line == EOF_MARKER {
                is_end_of_file = true;
                self.index += 1;
                continue;
            }
            let mut chars = line.chars();
            let Some(kind) = chars.next() else {
                return Err(ParseError::new(
                    self.line_no(),
                    "Update hunk lines must start with ' ', '-', or '+'",
                ));
            };
            let content = chars.as_str().to_string();
            match kind {
                ' ' => {
                    old_lines.push(content.clone());
                    new_lines.push(content);
                }
                '-' => old_lines.push(content),
                '+' => new_lines.push(content),
                _ => {
                    return Err(ParseError::new(
                        self.line_no(),
                        "Update hunk lines must start with ' ', '-', or '+'",
                    ))
                }
            }
            self.index += 1;
        }

        if old_lines.is_empty() && new_lines.is_empty() && !is_end_of_file {
            return Err(ParseError::new(
                self.line_no().saturating_sub(1),
                "Update hunk is empty",
            ));
        }
        Ok(UpdateChunk {
            change_context,
            old_lines,
            new_lines,
            is_end_of_file,
        })
    }

    fn parse_delete(&mut self, path: String) -> Result<(), ParseError> {
        validate_path_text(&path, self.line_no())?;
        self.index += 1;
        self.hunks.push(FileHunk::Delete { path });
        Ok(())
    }

    fn current(&self) -> &str {
        self.lines[self.index]
    }

    fn line_no(&self) -> usize {
        self.index + 1
    }
}

fn validate_path_text(path: &str, line: usize) -> Result<(), ParseError> {
    if path.is_empty() {
        return Err(ParseError::new(line, "file path must not be empty"));
    }
    Ok(())
}

fn is_file_header(line: &str) -> bool {
    line.starts_with(ADD)
        || line.starts_with(UPDATE)
        || line.starts_with(DELETE)
        || line.trim() == END
}
