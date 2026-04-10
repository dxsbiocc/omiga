//! Regression tests for background agent persistence (tasks + sidechain transcript).
//! Uses a temporary SQLite file with full migrations (`init_db`).

use crate::domain::agents::background::{BackgroundAgentStatus, BackgroundAgentTask};
use crate::domain::persistence::{init_db, SessionRepository};
use crate::domain::session::{Message, ToolCall};

async fn setup_session_repo() -> (SessionRepository, tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("omiga_persistence_test.sqlite");
    let pool = init_db(&db_path).await.expect("init_db");
    let repo = SessionRepository::new(pool);
    let session_id = "sess-persistence-test".to_string();
    repo.create_session(&session_id, "Persistence test", "/tmp/ws")
        .await
        .expect("create_session");
    (repo, dir, session_id)
}

fn sample_task(session_id: &str, task_id: &str) -> BackgroundAgentTask {
    BackgroundAgentTask {
        task_id: task_id.to_string(),
        agent_type: "TestAgent".to_string(),
        description: "do something".to_string(),
        status: BackgroundAgentStatus::Pending,
        created_at: 100,
        started_at: None,
        completed_at: None,
        result_summary: None,
        error_message: None,
        output_path: None,
        session_id: session_id.to_string(),
        message_id: "msg-invoke".to_string(),
    }
}

#[tokio::test]
async fn background_agent_task_upsert_list_get() {
    let (repo, _dir, session_id) = setup_session_repo().await;
    let mut task = sample_task(&session_id, "bg-task-1");
    repo.upsert_background_agent_task(&task)
        .await
        .expect("upsert");
    let list = repo
        .list_background_agent_tasks_for_session(&session_id)
        .await
        .expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].task_id, "bg-task-1");
    assert_eq!(list[0].status, BackgroundAgentStatus::Pending);

    let got = repo
        .get_background_agent_task_by_id("bg-task-1")
        .await
        .expect("get");
    assert!(got.is_some());
    assert_eq!(got.unwrap().description, "do something");

    task.status = BackgroundAgentStatus::Cancelled;
    task.completed_at = Some(200);
    repo.upsert_background_agent_task(&task)
        .await
        .expect("upsert2");
    let got2 = repo
        .get_background_agent_task_by_id("bg-task-1")
        .await
        .expect("get2")
        .unwrap();
    assert_eq!(got2.status, BackgroundAgentStatus::Cancelled);
    assert_eq!(got2.completed_at, Some(200));
}

#[tokio::test]
async fn background_agent_messages_append_order() {
    let (repo, _dir, session_id) = setup_session_repo().await;
    let task = sample_task(&session_id, "bg-task-msg");
    repo.upsert_background_agent_task(&task)
        .await
        .expect("upsert");

    let u1 = Message::User {
        content: "hello".to_string(),
    };
    let a1 = Message::Assistant {
        content: "hi".to_string(),
        tool_calls: None,
        token_usage: None,
        reasoning_content: None,
    };
    let t1 = Message::Tool {
        tool_call_id: "tu1".to_string(),
        output: "out".to_string(),
    };
    repo.append_background_agent_message("bg-task-msg", &session_id, &u1)
        .await
        .expect("append u1");
    repo.append_background_agent_message("bg-task-msg", &session_id, &a1)
        .await
        .expect("append a1");
    repo.append_background_agent_message("bg-task-msg", &session_id, &t1)
        .await
        .expect("append t1");

    let rows = repo
        .list_background_agent_messages_for_task("bg-task-msg")
        .await
        .expect("list messages");
    assert_eq!(rows.len(), 3);
    assert!(matches!(&rows[0], Message::User { content } if content == "hello"));
    assert!(matches!(&rows[1], Message::Assistant { content, .. } if content == "hi"));
    assert!(
        matches!(&rows[2], Message::Tool { tool_call_id, .. } if tool_call_id == "tu1")
    );
}

#[tokio::test]
async fn background_agent_messages_append_seq_monotonic() {
    let (repo, _dir, session_id) = setup_session_repo().await;
    let task = sample_task(&session_id, "bg-task-seq");
    repo.upsert_background_agent_task(&task)
        .await
        .expect("upsert");

    for i in 0..5 {
        let m = Message::User {
            content: format!("line-{i}"),
        };
        repo.append_background_agent_message("bg-task-seq", &session_id, &m)
            .await
            .expect("append");
    }
    let rows = repo
        .list_background_agent_messages_for_task("bg-task-seq")
        .await
        .expect("list");
    assert_eq!(rows.len(), 5);
    for (i, row) in rows.iter().enumerate() {
        let Message::User { content } = row else {
            panic!("expected user");
        };
        assert_eq!(content, &format!("line-{i}"));
    }
}

#[tokio::test]
async fn background_agent_messages_tool_calls_roundtrip() {
    let (repo, _dir, session_id) = setup_session_repo().await;
    let task = sample_task(&session_id, "bg-task-tools");
    repo.upsert_background_agent_task(&task)
        .await
        .expect("upsert");

    let asst = Message::Assistant {
        content: "call tool".to_string(),
        tool_calls: Some(vec![ToolCall {
            id: "c1".to_string(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        }]),
        token_usage: None,
        reasoning_content: None,
    };
    repo.append_background_agent_message("bg-task-tools", &session_id, &asst)
        .await
        .expect("append");
    let rows = repo
        .list_background_agent_messages_for_task("bg-task-tools")
        .await
        .expect("list");
    assert_eq!(rows.len(), 1);
    match &rows[0] {
        Message::Assistant { tool_calls, .. } => {
            assert!(tool_calls.as_ref().is_some_and(|c| {
                c.len() == 1 && c[0].name == "read" && c[0].id == "c1"
            }));
        }
        _ => panic!("expected assistant"),
    }
}
