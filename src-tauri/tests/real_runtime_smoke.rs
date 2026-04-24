//! Real-provider smoke test.
//!
//! Ignored by default because it requires a valid provider config on the host machine.
//! Intended for manual validation before running full `/schedule` / `/team` / `/autopilot`
//! scenarios in the GUI.

use futures::StreamExt;
use omiga_lib::llm::{create_client, load_config, LlmMessage, LlmStreamChunk};

#[tokio::test]
#[ignore = "requires real provider config"]
async fn provider_chat_smoke() {
    let config = load_config().expect("load provider config");
    let provider = config.provider.to_string();
    let model = config.model.clone();
    eprintln!("real-runtime smoke using provider={provider} model={model}");

    let client = create_client(config).expect("create client");
    let messages = vec![LlmMessage::user("Reply with exactly: ok")];
    let mut stream = client
        .send_message_streaming(messages, vec![])
        .await
        .expect("start streaming");

    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk.expect("stream chunk") {
            LlmStreamChunk::Text(t) | LlmStreamChunk::ReasoningContent(t) => out.push_str(&t),
            LlmStreamChunk::Stop { .. } => break,
            _ => {}
        }
    }

    let normalized = out.trim().to_lowercase();
    assert!(
        normalized.contains("ok"),
        "expected provider smoke reply to contain 'ok', got: {out}"
    );
}
