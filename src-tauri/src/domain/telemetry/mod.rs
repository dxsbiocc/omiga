//! Lightweight turn-level telemetry aggregation.

use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnMetrics {
    pub connect_ms: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub stream_total_ms: Option<u64>,
    pub tool_calls: u32,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub retries: u32,
}

#[derive(Debug, Clone)]
pub struct TurnMetricsRecorder {
    start: Instant,
    connected_at: Option<Instant>,
    first_visible_at: Option<Instant>,
    stream_end_at: Option<Instant>,
    tool_calls: u32,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    cache_read_tokens: Option<u32>,
    retries: u32,
}

impl TurnMetricsRecorder {
    pub fn new(start: Instant) -> Self {
        Self {
            start,
            connected_at: None,
            first_visible_at: None,
            stream_end_at: None,
            tool_calls: 0,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            retries: 0,
        }
    }

    pub fn on_connected(&mut self, now: Instant) {
        if self.connected_at.is_none() {
            self.connected_at = Some(now);
        }
    }

    pub fn on_first_visible(&mut self, now: Instant) {
        if self.first_visible_at.is_none() {
            self.first_visible_at = Some(now);
        }
    }

    pub fn on_stream_end(&mut self, now: Instant) {
        self.stream_end_at = Some(now);
    }

    pub fn set_usage(
        &mut self,
        input_tokens: u32,
        output_tokens: u32,
        cache_read_tokens: Option<u32>,
    ) {
        self.input_tokens = Some(input_tokens);
        self.output_tokens = Some(output_tokens);
        self.cache_read_tokens = cache_read_tokens;
    }

    pub fn inc_tool(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }

    pub fn inc_retry(&mut self) {
        self.retries = self.retries.saturating_add(1);
    }

    pub fn metrics(&self) -> TurnMetrics {
        TurnMetrics {
            connect_ms: self
                .connected_at
                .map(|connected_at| elapsed_ms(self.start, connected_at)),
            ttft_ms: self
                .first_visible_at
                .map(|first_visible_at| elapsed_ms(self.start, first_visible_at)),
            stream_total_ms: self
                .stream_end_at
                .map(|stream_end_at| elapsed_ms(self.start, stream_end_at)),
            tool_calls: self.tool_calls,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            retries: self.retries,
        }
    }
}

pub fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub fn elapsed_ms(start: Instant, end: Instant) -> u64 {
    if end >= start {
        duration_ms(end.duration_since(start))
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn recorder_calculates_ordered_phase_durations() {
        let start = Instant::now();
        let mut recorder = TurnMetricsRecorder::new(start);

        recorder.on_connected(start + ms(25));
        recorder.on_first_visible(start + ms(80));
        recorder.on_stream_end(start + ms(240));

        let metrics = recorder.metrics();
        assert_eq!(metrics.connect_ms, Some(25));
        assert_eq!(metrics.ttft_ms, Some(80));
        assert_eq!(metrics.stream_total_ms, Some(240));
        assert!(metrics.connect_ms <= metrics.ttft_ms);
        assert!(metrics.ttft_ms <= metrics.stream_total_ms);
    }

    #[test]
    fn recorder_leaves_ttft_empty_without_first_visible_chunk() {
        let start = Instant::now();
        let mut recorder = TurnMetricsRecorder::new(start);

        recorder.on_connected(start + ms(10));
        recorder.on_stream_end(start + ms(40));

        let metrics = recorder.metrics();
        assert_eq!(metrics.connect_ms, Some(10));
        assert_eq!(metrics.ttft_ms, None);
        assert_eq!(metrics.stream_total_ms, Some(40));
    }

    #[test]
    fn recorder_counts_tools_retries_and_copies_usage() {
        let start = Instant::now();
        let mut recorder = TurnMetricsRecorder::new(start);

        recorder.inc_tool();
        recorder.inc_tool();
        recorder.inc_retry();
        recorder.inc_retry();
        recorder.inc_retry();
        recorder.set_usage(1_000, 250, Some(8_000));

        let metrics = recorder.metrics();
        assert_eq!(metrics.tool_calls, 2);
        assert_eq!(metrics.retries, 3);
        assert_eq!(metrics.input_tokens, Some(1_000));
        assert_eq!(metrics.output_tokens, Some(250));
        assert_eq!(metrics.cache_read_tokens, Some(8_000));
    }
}
