//! Session codec tests

use chrono::Utc;
use omiga_lib::api::Role;
use omiga_lib::domain::persistence::MessageRecord;
use omiga_lib::domain::session::{Message, SessionCodec, ToolCall};

#[test]
fn test_user_message_roundtrip() {
    let msg = Message::User {
        content: "Hello".to_string(),
    };
    let (id, session_id, role, content, tool_calls, tool_call_id, tok, reasoning, _follow_up) =
        SessionCodec::message_to_record(&msg, "msg-1", "sess-1");

    assert_eq!(id, "msg-1");
    assert_eq!(session_id, "sess-1");
    assert_eq!(role, "user");
    assert_eq!(content, "Hello");
    assert!(tool_calls.is_none());
    assert!(tool_call_id.is_none());
    assert!(tok.is_none());
    assert!(reasoning.is_none());
}

#[test]
fn test_assistant_message_roundtrip() {
    let msg = Message::Assistant {
        content: "Let me help".to_string(),
        tool_calls: None,
        token_usage: None,
        reasoning_content: None,
        follow_up_suggestions: None,
        turn_summary: None,
    };

    let (_, _, role, content, tool_calls, tool_call_id, tok, reasoning, _) =
        SessionCodec::message_to_record(&msg, "msg-2", "sess-1");

    assert_eq!(role, "assistant");
    assert_eq!(content, "Let me help");
    assert!(tool_calls.is_none());
    assert!(tool_call_id.is_none());
    assert!(tok.is_none());
    assert!(reasoning.is_none());
}

#[test]
fn test_assistant_message_with_tool_calls_roundtrip() {
    let tool_calls = vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path": "test.txt"}"#.to_string(),
    }];

    let msg = Message::Assistant {
        content: "Let me read that file".to_string(),
        tool_calls: Some(tool_calls),
        token_usage: None,
        reasoning_content: None,
        follow_up_suggestions: None,
        turn_summary: None,
    };

    let (_, _, role, content, tool_calls_json, _, tok, reasoning, _) =
        SessionCodec::message_to_record(&msg, "msg-3", "sess-1");

    assert_eq!(role, "assistant");
    assert_eq!(content, "Let me read that file");
    assert!(tool_calls_json.is_some());
    assert!(tok.is_none());
    assert!(reasoning.is_none());

    // Verify we can parse it back
    let parsed = SessionCodec::parse_tool_calls(&tool_calls_json.unwrap());
    assert!(parsed.is_some());
    let parsed_calls = parsed.unwrap();
    assert_eq!(parsed_calls.len(), 1);
    assert_eq!(parsed_calls[0].id, "call-1");
    assert_eq!(parsed_calls[0].name, "read_file");
}

#[test]
fn test_tool_message_roundtrip() {
    let msg = Message::Tool {
        tool_call_id: "call-1".to_string(),
        output: "File contents here".to_string(),
    };

    let (_, _, role, content, tool_calls, tool_call_id, tok, reasoning, _) =
        SessionCodec::message_to_record(&msg, "msg-4", "sess-1");

    assert_eq!(role, "tool");
    assert_eq!(content, "File contents here");
    assert!(tool_calls.is_none());
    assert_eq!(tool_call_id, Some("call-1".to_string()));
    assert!(tok.is_none());
    assert!(reasoning.is_none());
}

#[test]
fn test_record_to_message_user() {
    let record = MessageRecord {
        id: "msg-1".to_string(),
        session_id: "sess-1".to_string(),
        role: "user".to_string(),
        content: "Hello world".to_string(),
        tool_calls: None,
        tool_call_id: None,
        token_usage_json: None,
        reasoning_content: None,
        follow_up_suggestions_json: None,
        turn_summary: None,
        created_at: Utc::now().to_rfc3339(),
    };

    let msg = SessionCodec::record_to_message(record);
    match msg {
        Message::User { content } => assert_eq!(content, "Hello world"),
        _ => panic!("Expected User message"),
    }
}

#[test]
fn test_record_to_message_assistant_with_tool_calls() {
    let tool_calls_json = r#"[{"id":"call-1","name":"bash","arguments":"{\"command\": \"ls\"}"}]"#;

    let record = MessageRecord {
        id: "msg-2".to_string(),
        session_id: "sess-1".to_string(),
        role: "assistant".to_string(),
        content: "Running command".to_string(),
        tool_calls: Some(tool_calls_json.to_string()),
        tool_call_id: None,
        token_usage_json: None,
        reasoning_content: None,
        follow_up_suggestions_json: None,
        turn_summary: None,
        created_at: Utc::now().to_rfc3339(),
    };

    let msg = SessionCodec::record_to_message(record);
    match msg {
        Message::Assistant {
            content,
            tool_calls,
            reasoning_content,
            ..
        } => {
            assert_eq!(content, "Running command");
            assert!(tool_calls.is_some());
            assert_eq!(tool_calls.as_ref().unwrap().len(), 1);
            assert_eq!(tool_calls.unwrap()[0].id, "call-1");
            assert!(reasoning_content.is_none());
        }
        _ => panic!("Expected Assistant message"),
    }
}

#[test]
fn test_record_to_message_tool() {
    let record = MessageRecord {
        id: "msg-3".to_string(),
        session_id: "sess-1".to_string(),
        role: "tool".to_string(),
        content: "total 32\n-rw-r--r-- 1 user user 1234 Jan 1 00:00 file.txt".to_string(),
        tool_calls: None,
        tool_call_id: Some("call-1".to_string()),
        token_usage_json: None,
        reasoning_content: None,
        follow_up_suggestions_json: None,
        turn_summary: None,
        created_at: Utc::now().to_rfc3339(),
    };

    let msg = SessionCodec::record_to_message(record);
    match msg {
        Message::Tool {
            tool_call_id,
            output,
        } => {
            assert_eq!(tool_call_id, "call-1");
            assert!(output.contains("total 32"));
        }
        _ => panic!("Expected Tool message"),
    }
}

#[test]
fn test_to_api_messages_user_only() {
    let messages = vec![Message::User {
        content: "Hello".to_string(),
    }];

    let api_messages = SessionCodec::to_api_messages(&messages);
    assert_eq!(api_messages.len(), 1);
    assert_eq!(api_messages[0].role, Role::User);
}

#[test]
fn test_to_api_messages_conversation() {
    let messages = vec![
        Message::User {
            content: "List files".to_string(),
        },
        Message::Assistant {
            content: "I'll list the files for you".to_string(),
            tool_calls: Some(vec![ToolCall {
                id: "call-1".to_string(),
                name: "bash".to_string(),
                arguments: r#"{"command": "ls -la"}"#.to_string(),
            }]),
            token_usage: None,
            reasoning_content: None,
            follow_up_suggestions: None,
            turn_summary: None,
        },
        Message::Tool {
            tool_call_id: "call-1".to_string(),
            output: "total 10\ndrwxr-xr-x 5 user user 160 Jan 1 00:00 .".to_string(),
        },
    ];

    let api_messages = SessionCodec::to_api_messages(&messages);
    assert_eq!(api_messages.len(), 3);

    // Check user message
    assert_eq!(api_messages[0].role, Role::User);

    // Check assistant message with tool use
    assert_eq!(api_messages[1].role, Role::Assistant);
    assert_eq!(api_messages[1].content.len(), 2); // Text + ToolUse

    // Check tool result
    assert_eq!(api_messages[2].role, Role::User);
}

#[test]
fn test_extract_tool_calls() {
    let msg_with_tools = Message::Assistant {
        content: "Using tool".to_string(),
        tool_calls: Some(vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path": "test.txt"}"#.to_string(),
        }]),
        token_usage: None,
        reasoning_content: None,
        follow_up_suggestions: None,
        turn_summary: None,
    };

    let extracted = SessionCodec::extract_tool_calls(&msg_with_tools);
    assert!(extracted.is_some());
    let json = extracted.unwrap();
    assert!(json.contains("read_file"));
}

#[test]
fn test_build_assistant_message() {
    let tool_calls_json = r#"[{"id":"call-1","name":"bash","arguments":"{}"}]"#;

    let msg = SessionCodec::build_assistant_message("Output", Some(tool_calls_json));
    match msg {
        Message::Assistant {
            content,
            tool_calls,
            reasoning_content,
            ..
        } => {
            assert_eq!(content, "Output");
            assert!(tool_calls.is_some());
            assert_eq!(tool_calls.unwrap().len(), 1);
            assert!(reasoning_content.is_none());
        }
        _ => panic!("Expected Assistant message"),
    }
}

#[test]
fn test_build_assistant_message_without_tools() {
    let msg = SessionCodec::build_assistant_message("Just text", None);
    match msg {
        Message::Assistant {
            content,
            tool_calls,
            reasoning_content,
            ..
        } => {
            assert_eq!(content, "Just text");
            assert!(tool_calls.is_none());
            assert!(reasoning_content.is_none());
        }
        _ => panic!("Expected Assistant message"),
    }
}
