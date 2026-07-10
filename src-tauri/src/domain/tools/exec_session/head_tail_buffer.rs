use std::collections::VecDeque;

/// A bounded byte buffer that preserves the beginning and end of output.
///
/// Once output exceeds `max_bytes`, bytes from the middle are omitted and
/// represented by a `[... N bytes truncated ...]` marker when materialized.
#[derive(Debug, Clone)]
pub struct HeadTailBuffer {
    max_bytes: usize,
    head_budget: usize,
    tail_budget: usize,
    head: VecDeque<Vec<u8>>,
    tail: VecDeque<Vec<u8>>,
    head_bytes: usize,
    tail_bytes: usize,
    omitted_bytes: usize,
}

impl HeadTailBuffer {
    pub fn new(max_bytes: usize) -> Self {
        let head_budget = max_bytes / 2;
        let tail_budget = max_bytes.saturating_sub(head_budget);
        Self {
            max_bytes,
            head_budget,
            tail_budget,
            head: VecDeque::new(),
            tail: VecDeque::new(),
            head_bytes: 0,
            tail_bytes: 0,
            omitted_bytes: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.max_bytes == 0 {
            self.omitted_bytes = self.omitted_bytes.saturating_add(bytes.len());
            return;
        }

        if self.head_bytes < self.head_budget {
            let remaining_head = self.head_budget.saturating_sub(self.head_bytes);
            if bytes.len() <= remaining_head {
                self.head.push_back(bytes.to_vec());
                self.head_bytes = self.head_bytes.saturating_add(bytes.len());
                return;
            }

            let (head_part, tail_part) = bytes.split_at(remaining_head);
            if !head_part.is_empty() {
                self.head.push_back(head_part.to_vec());
                self.head_bytes = self.head_bytes.saturating_add(head_part.len());
            }
            self.push_tail(tail_part);
            return;
        }

        self.push_tail(bytes);
    }

    pub fn omitted_bytes(&self) -> usize {
        self.omitted_bytes
    }

    pub fn retained_bytes(&self) -> usize {
        self.head_bytes.saturating_add(self.tail_bytes)
    }

    pub fn is_truncated(&self) -> bool {
        self.omitted_bytes > 0
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let marker = self.truncation_marker();
        let marker_len = marker.as_ref().map_or(0, Vec::len);
        let mut out = Vec::with_capacity(self.retained_bytes().saturating_add(marker_len));

        for chunk in &self.head {
            out.extend_from_slice(chunk);
        }
        if let Some(marker) = marker {
            out.extend_from_slice(&marker);
        }
        for chunk in &self.tail {
            out.extend_from_slice(chunk);
        }

        out
    }

    fn push_tail(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.tail_budget == 0 {
            self.omitted_bytes = self.omitted_bytes.saturating_add(bytes.len());
            return;
        }

        if bytes.len() >= self.tail_budget {
            let start = bytes.len().saturating_sub(self.tail_budget);
            let kept = bytes[start..].to_vec();
            let dropped = bytes.len().saturating_sub(kept.len());
            self.omitted_bytes = self
                .omitted_bytes
                .saturating_add(self.tail_bytes)
                .saturating_add(dropped);
            self.tail.clear();
            self.tail.push_back(kept);
            self.tail_bytes = self.tail_budget;
            return;
        }

        self.tail.push_back(bytes.to_vec());
        self.tail_bytes = self.tail_bytes.saturating_add(bytes.len());
        self.trim_tail();
    }

    fn trim_tail(&mut self) {
        let mut excess = self.tail_bytes.saturating_sub(self.tail_budget);
        while excess > 0 {
            match self.tail.front_mut() {
                Some(front) if excess >= front.len() => {
                    excess -= front.len();
                    self.tail_bytes = self.tail_bytes.saturating_sub(front.len());
                    self.omitted_bytes = self.omitted_bytes.saturating_add(front.len());
                    self.tail.pop_front();
                }
                Some(front) => {
                    front.drain(..excess);
                    self.tail_bytes = self.tail_bytes.saturating_sub(excess);
                    self.omitted_bytes = self.omitted_bytes.saturating_add(excess);
                    break;
                }
                None => break,
            }
        }
    }

    fn truncation_marker(&self) -> Option<Vec<u8>> {
        if self.omitted_bytes == 0 {
            None
        } else {
            Some(format!("[... {} bytes truncated ...]", self.omitted_bytes).into_bytes())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HeadTailBuffer;

    #[test]
    fn head_tail_buffer_does_not_truncate_below_cap() {
        let mut buffer = HeadTailBuffer::new(16);
        buffer.push(b"short output");

        assert_eq!(buffer.omitted_bytes(), 0);
        assert!(!buffer.is_truncated());
        assert_eq!(buffer.to_bytes(), b"short output");
    }

    #[test]
    fn head_tail_buffer_preserves_head_tail_and_reports_omitted_bytes() {
        let mut buffer = HeadTailBuffer::new(10);
        buffer.push(b"abcdefghijklmnopqrstuvwxyz");

        let out = String::from_utf8(buffer.to_bytes()).expect("utf8");
        assert!(out.starts_with("abcde"), "{out}");
        assert!(out.ends_with("vwxyz"), "{out}");
        assert!(out.contains("[... 16 bytes truncated ...]"), "{out}");
        assert_eq!(buffer.omitted_bytes(), 16);
        assert_eq!(buffer.retained_bytes(), 10);
    }
}
