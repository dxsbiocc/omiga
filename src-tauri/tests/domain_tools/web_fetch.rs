//! Tests for `web_fetch`.

use futures::StreamExt;
use omiga_lib::domain::tools::web_fetch::{WebFetchArgs, WebFetchTool};
use omiga_lib::domain::tools::{ToolContext, ToolImpl};
use omiga_lib::infrastructure::streaming::StreamOutputItem;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn web_fetch_returns_html_as_text() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body: &[u8] = b"<html><body><p>Hello</p></body></html>";
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = socket.read(&mut buf).await;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let mut out = response.into_bytes();
        out.extend_from_slice(body);
        let _ = socket.write_all(&out).await;
    });

    let url = format!("http://127.0.0.1:{}/page", addr.port());
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = WebFetchArgs {
        url,
        prompt: "What is the greeting?".to_string(),
    };

    let mut stream = WebFetchTool::execute(&ctx, args).await.unwrap();
    let mut combined = String::new();
    while let Some(item) = stream.next().await {
        if let StreamOutputItem::Content(s) = item {
            combined.push_str(&s);
        }
    }

    assert!(combined.contains("HTTP 200"));
    assert!(combined.contains("Hello"));
    assert!(combined.contains("What is the greeting?"));
}

#[tokio::test]
async fn web_fetch_rejects_non_http_scheme() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = WebFetchArgs {
        url: "ftp://example.com/x".to_string(),
        prompt: "x".to_string(),
    };
    assert!(WebFetchTool::execute(&ctx, args).await.is_err());
}
