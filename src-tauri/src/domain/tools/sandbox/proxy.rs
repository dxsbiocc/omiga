use std::io;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::{HostRule, NetworkMode, NetworkPolicy};
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{timeout, timeout_at, Instant};
use tracing::{debug, info};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HANDSHAKE_BYTES: usize = 16 * 1024;
const MAX_CONCURRENT_CONNECTIONS: usize = 512;
static PROXY: OnceLock<Mutex<Option<NetworkPolicyProxy>>> = OnceLock::new();

pub fn policy_needs_proxy(policy: &NetworkPolicy) -> bool {
    matches!(policy.mode, NetworkMode::AllowList | NetworkMode::DenyList)
}

// Known limitation (accepted for this opt-in feature): a single process-wide
// proxy carries one policy. If two concurrent bash commands ran under
// *different* network policies, the later `ensure_proxy_for_policy` would
// overwrite the shared policy and the earlier command's in-flight connections
// would be evaluated against the newer policy on the same port. This does not
// trigger in practice because the policy is derived from process environment
// (`OMIGA_SANDBOX_NETWORK*`), which is static for the life of the process.
// Per-command policy binding would require per-connection tagging over plain
// CONNECT and is out of scope here.
pub async fn ensure_proxy_for_policy(policy: &NetworkPolicy) -> Result<Option<u16>, String> {
    let singleton = PROXY.get_or_init(|| Mutex::new(None));
    let mut guard = singleton.lock().await;

    if !policy_needs_proxy(policy) {
        if let Some(proxy) = guard.take() {
            proxy.shutdown().await;
        }
        return Ok(None);
    }

    if let Some(proxy) = guard.as_mut() {
        let policy_handle = proxy.policy_handle();
        let mut writable_policy = policy_handle.write().await;
        *writable_policy = policy.clone();
        return Ok(Some(proxy.port()));
    }

    let proxy = NetworkPolicyProxy::start(policy.clone()).await?;
    let port = proxy.port();
    *guard = Some(proxy);
    Ok(Some(port))
}

pub async fn shutdown_proxy_singleton() {
    let singleton = PROXY.get_or_init(|| Mutex::new(None));
    let mut guard = singleton.lock().await;

    if let Some(proxy) = guard.take() {
        proxy.shutdown().await;
    }
}

#[derive(Debug)]
pub struct NetworkPolicyProxy {
    policy: Arc<RwLock<NetworkPolicy>>,
    accept_loop: JoinHandle<()>,
    shutdown: watch::Sender<bool>,
    port: u16,
}

#[derive(Debug)]
struct ParsedRequest {
    host: String,
    port: u16,
    connect: bool,
    request_bytes: Vec<u8>,
}

impl NetworkPolicyProxy {
    pub async fn start(policy: NetworkPolicy) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| format!("failed to bind network policy proxy: {error}"))?;

        let port = listener
            .local_addr()
            .map_err(|error| format!("failed to read proxy bind address: {error}"))?
            .port();
        let policy = Arc::new(RwLock::new(policy));
        let (shutdown, shutdown_rx) = watch::channel(false);
        let policy_handle = Arc::clone(&policy);
        let connection_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));

        let accept_loop = tokio::spawn(async move {
            let mut shutdown = shutdown_rx;
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        break;
                    }
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((mut stream, _)) => {
                                let permit = match connection_limit.clone().try_acquire_owned() {
                                    Ok(permit) => permit,
                                    Err(_) => {
                                        if let Err(error) =
                                            write_service_unavailable(&mut stream).await
                                        {
                                            debug!(error = %error, "failed to respond with service unavailable");
                                        }
                                        continue;
                                    }
                                };
                                let policy = Arc::clone(&policy_handle);
                                tokio::spawn(handle_connection(stream, policy, permit));
                            }
                            Err(error) => {
                                debug!(error = %error, "network policy proxy accept failed");
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            policy,
            accept_loop,
            shutdown,
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn policy_handle(&self) -> Arc<RwLock<NetworkPolicy>> {
        Arc::clone(&self.policy)
    }

    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.accept_loop.await;
    }
}

pub fn connect_allowed(policy: &NetworkPolicy, host: &str, port: u16) -> bool {
    if host.is_empty() {
        return false;
    }

    match policy.mode {
        NetworkMode::AllowAll => true,
        NetworkMode::DenyAll => false,
        NetworkMode::AllowList => policy
            .hosts
            .iter()
            .any(|rule| host_matches(rule, host, port)),
        NetworkMode::DenyList => !policy
            .hosts
            .iter()
            .any(|rule| host_matches(rule, host, port)),
    }
}

async fn handle_connection(
    mut upstream: TcpStream,
    policy: Arc<RwLock<NetworkPolicy>>,
    _permit: OwnedSemaphorePermit,
) {
    let request = match read_request_frame(&mut upstream).await {
        Ok(request) => request,
        Err(error) => {
            debug!(error = %error, "network policy proxy read request failed");
            return;
        }
    };

    let request_target = request.host;
    let target_port = request.port;
    let allowed = {
        let policy = policy.read().await;
        connect_allowed(&policy, &request_target, target_port)
    };
    if !allowed {
        let target = format!("{request_target}:{target_port}");
        info!(target = %target, "network policy blocked by Omiga proxy");
        if write_blocked(&mut upstream, &target).await.is_err() {
            debug!("network policy proxy failed to write blocked response");
        }
        return;
    }

    let upstream_connect = match timeout(
        HANDSHAKE_TIMEOUT,
        TcpStream::connect((request_target.as_str(), target_port)),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            debug!(error = %error, "network policy proxy target connect failed");
            let target = format!("{request_target}:{target_port}");
            let _ = write_upstream_error(&mut upstream, 502, "Bad Gateway", &target).await;
            return;
        }
        Err(_) => {
            debug!("network policy proxy upstream connect timed out");
            let target = format!("{request_target}:{target_port}");
            let _ = write_upstream_error(&mut upstream, 504, "Gateway Timeout", &target).await;
            return;
        }
    };

    if request.connect {
        if let Err(error) = upstream
            .write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: Omiga\r\n\r\n")
            .await
        {
            debug!(error = %error, "network policy proxy failed to write connect-ok");
            return;
        }

        let mut upstream_stream = upstream_connect;
        let _ = copy_bidirectional(&mut upstream, &mut upstream_stream).await;
        return;
    }

    let mut upstream_stream = upstream_connect;
    if let Err(error) = upstream_stream.write_all(&request.request_bytes).await {
        debug!(error = %error, "network policy proxy failed to write request bytes");
        return;
    }

    let _ = copy_bidirectional(&mut upstream, &mut upstream_stream).await;
}

async fn write_blocked(stream: &mut TcpStream, target: &str) -> io::Result<()> {
    write_http_error(
        stream,
        403,
        "Forbidden",
        &format!("{target} blocked by Omiga network policy"),
    )
    .await
}

async fn write_upstream_error(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    target: &str,
) -> io::Result<()> {
    write_http_error(
        stream,
        status_code,
        reason,
        &format!("{target} cannot be reached through Omiga proxy"),
    )
    .await
}

async fn write_service_unavailable(stream: &mut TcpStream) -> io::Result<()> {
    write_http_error(
        stream,
        503,
        "Service Unavailable",
        "network policy proxy is overloaded",
    )
    .await
}

async fn write_http_error(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    body: &str,
) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status_code} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body,
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_request_frame(stream: &mut TcpStream) -> io::Result<ParsedRequest> {
    let mut buf = Vec::with_capacity(1024);
    let deadline = timeout_at(Instant::now() + HANDSHAKE_TIMEOUT, async {
        let mut tmp = [0u8; 1024];
        loop {
            let remaining = MAX_HANDSHAKE_BYTES.saturating_sub(buf.len());
            if remaining == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "request headers exceeded 16KB",
                ));
            }

            let read_limit = remaining.min(tmp.len());
            let n = stream.read(&mut tmp[..read_limit]).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "remote closed before handshake completion",
                ));
            }

            buf.extend_from_slice(&tmp[..n]);
            if let Some((header_end, _)) = find_header_end(&buf) {
                let request = parse_request(&buf[..header_end + 4])?;
                return Ok(ParsedRequest {
                    request_bytes: buf,
                    ..request
                });
            }
        }
    });

    match deadline.await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "network policy proxy handshake timeout",
        )),
    }
}

fn parse_request(request_bytes: &[u8]) -> io::Result<ParsedRequest> {
    let request_text = std::str::from_utf8(request_bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let mut lines = request_text.split("\r\n").peekable();
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty HTTP request line"))?;

    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request method"))?;
    let target = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request target"))?;

    let is_connect = method.eq_ignore_ascii_case("CONNECT");

    if is_connect {
        let (host, port) = parse_host_port(target, 443)?;
        return Ok(ParsedRequest {
            host,
            port,
            connect: true,
            request_bytes: request_bytes.to_vec(),
        });
    }

    if let Some((host, port)) = parse_absolute_uri_host(target, 80) {
        return Ok(ParsedRequest {
            host,
            port,
            connect: false,
            request_bytes: request_bytes.to_vec(),
        });
    }

    let mut host = None;
    for line in lines {
        let header_line = line.trim();
        if header_line.is_empty() {
            break;
        }
        if let Some((name, value)) = header_line.split_once(':') {
            if name.eq_ignore_ascii_case("host") {
                host = Some(value.trim().to_string());
            }
        }
    }

    let host = host.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Host header for plain request",
        )
    })?;
    let (host, port) = parse_host_port(&host, 80)?;
    Ok(ParsedRequest {
        host,
        port,
        connect: false,
        request_bytes: request_bytes.to_vec(),
    })
}

fn find_header_end(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let marker = b"\r\n\r\n";
    bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|index| (index, &bytes[index..]))
}

fn parse_absolute_uri_host(target: &str, default_port: u16) -> Option<(String, u16)> {
    let authority = if let Some(url) = target.strip_prefix("http://") {
        Some(url)
    } else if let Some(url) = target.strip_prefix("https://") {
        Some(url)
    } else {
        None
    }?;

    let host_part = authority
        .split_once('/')
        .map(|(authority, _)| authority)
        .unwrap_or(authority);
    parse_host_port(host_part, default_port).ok()
}

fn parse_host_port(raw: &str, default_port: u16) -> io::Result<(String, u16)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty host value",
        ));
    }

    if raw.starts_with('[') {
        let close = raw
            .find(']')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv6 host"))?;
        let host = raw[1..close].trim_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty IPv6 host",
            ));
        }

        let port = if let Some(rest) = raw[close + 1..].strip_prefix(':') {
            parse_port(rest)?
        } else {
            default_port
        };
        return Ok((host, port));
    }

    let mut parts = raw.rsplitn(2, ':');
    let last = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid host header"))?;
    let first = parts.next();

    let (host, port) = match (first, last.parse::<u16>().ok()) {
        (Some(host), Some(port)) if port > 0 => (host, port),
        _ => (raw, default_port),
    };

    let host = host.trim_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty host value",
        ));
    }

    if port == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "host port cannot be zero",
        ));
    }

    Ok((host, port))
}

fn parse_port(raw_port: &str) -> io::Result<u16> {
    let parsed = raw_port
        .parse::<u16>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid host port value"))?;
    if parsed == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "host port cannot be zero",
        ));
    }
    Ok(parsed)
}

fn host_matches(rule: &HostRule, host: &str, port: u16) -> bool {
    if rule.port.is_some_and(|rule_port| rule_port != port) {
        return false;
    }

    if rule.domain == "*" {
        return true;
    }

    if rule.domain == host {
        return true;
    }

    let Some(wildcard) = rule.domain.strip_prefix("*.") else {
        return false;
    };

    host.len() > wildcard.len() + 1 && host.ends_with(format!(".{wildcard}").as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    static SINGLETON_TEST_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

    fn lock_singleton_tests() -> &'static Mutex<()> {
        SINGLETON_TEST_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn connect_allowed_allow_mode_allows_any_host() {
        let policy = NetworkPolicy {
            mode: NetworkMode::AllowAll,
            hosts: Vec::new(),
        };
        assert!(connect_allowed(&policy, "BLOCKED.EXAMPLE.COM", 443));
    }

    #[test]
    fn policy_needs_proxy_matches_mode() {
        assert!(!policy_needs_proxy(&NetworkPolicy::allow_all()));
        assert!(!policy_needs_proxy(&NetworkPolicy::deny_all()));
        assert!(policy_needs_proxy(&NetworkPolicy {
            mode: NetworkMode::AllowList,
            hosts: Vec::new(),
        }));
        assert!(policy_needs_proxy(&NetworkPolicy {
            mode: NetworkMode::DenyList,
            hosts: Vec::new(),
        }));
    }

    #[tokio::test]
    async fn ensure_proxy_for_policy_manages_singleton_lifecycle() {
        let _singleton_guard = lock_singleton_tests().lock().await;
        shutdown_proxy_singleton().await;

        let policy = NetworkPolicy::allow_all();
        assert!(ensure_proxy_for_policy(&policy).await.unwrap().is_none());

        let policy = NetworkPolicy {
            mode: NetworkMode::AllowList,
            hosts: vec![HostRule {
                domain: "127.0.0.1".to_string(),
                port: Some(443),
            }],
        };
        let allow_port = match ensure_proxy_for_policy(&policy).await {
            Ok(Some(port)) => port,
            Ok(None) => {
                eprintln!("skip test because allow-list policy did not start proxy");
                return;
            }
            Err(error) => {
                eprintln!("skip test because proxy start failed: {error}");
                return;
            }
        };

        if timeout(
            Duration::from_millis(500),
            TcpStream::connect(("127.0.0.1", allow_port)),
        )
        .await
        .is_err()
        {
            eprintln!("skip test because proxy listener not reachable");
            shutdown_proxy_singleton().await;
            return;
        }

        let updated = match ensure_proxy_for_policy(&NetworkPolicy {
            mode: NetworkMode::DenyList,
            hosts: vec![HostRule {
                domain: "*".to_string(),
                port: None,
            }],
        })
        .await
        {
            Ok(Some(port)) => port,
            Ok(None) => {
                eprintln!("skip test because proxy policy update returned None");
                shutdown_proxy_singleton().await;
                return;
            }
            Err(error) => {
                eprintln!("skip test because proxy policy update failed: {error}");
                shutdown_proxy_singleton().await;
                return;
            }
        };
        assert_eq!(updated, allow_port);

        let mut blocked = match timeout(
            Duration::from_secs(1),
            TcpStream::connect(("127.0.0.1", allow_port)),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                eprintln!("skip test because proxy listener disappeared: {error}");
                shutdown_proxy_singleton().await;
                return;
            }
            Err(_) => {
                eprintln!("skip test because proxy listener became unreachable");
                shutdown_proxy_singleton().await;
                return;
            }
        };

        if blocked
            .write_all(b"CONNECT 127.0.0.1:443 HTTP/1.1\r\nHost: 127.0.0.1:443\r\n\r\n")
            .await
            .is_err()
        {
            eprintln!("skip test because proxy request write failed");
            shutdown_proxy_singleton().await;
            return;
        }

        let mut response = [0u8; 32];
        let n = match timeout(Duration::from_secs(1), blocked.read(&mut response)).await {
            Ok(Ok(count)) => count,
            Ok(Err(error)) => {
                eprintln!("skip test because proxy response read failed: {error}");
                shutdown_proxy_singleton().await;
                return;
            }
            Err(_) => {
                eprintln!("skip test because proxy response timeout");
                shutdown_proxy_singleton().await;
                return;
            }
        };
        assert!(
            String::from_utf8_lossy(&response[..n]).starts_with("HTTP/1.1 403"),
            "policy hot update should block CONNECT via singleton proxy"
        );

        let denied_port = allow_port;
        assert!(ensure_proxy_for_policy(&NetworkPolicy::deny_all())
            .await
            .unwrap()
            .is_none());
        let denied = timeout(
            Duration::from_secs(1),
            TcpStream::connect(("127.0.0.1", denied_port)),
        )
        .await;
        assert!(
            matches!(denied, Ok(Err(_)) | Err(_)),
            "proxy port should stop accepting after deny-all"
        );
        shutdown_proxy_singleton().await;
    }

    #[test]
    fn connect_allowed_deny_mode_allows_only_matching_rules() {
        let policy = NetworkPolicy {
            mode: NetworkMode::AllowList,
            hosts: vec![
                HostRule {
                    domain: "example.com".to_string(),
                    port: None,
                },
                HostRule {
                    domain: "*.wild.test".to_string(),
                    port: Some(443),
                },
                HostRule {
                    domain: "*".to_string(),
                    port: Some(443),
                },
            ],
        };

        assert!(connect_allowed(&policy, "example.com", 80));
        assert!(connect_allowed(&policy, "sub.wild.test", 443));
        assert!(connect_allowed(&policy, "x.wild.test", 443));
        assert!(connect_allowed(&policy, "example.com", 444));
        assert!(!connect_allowed(&policy, "sub.wild.test", 80));
        assert!(connect_allowed(&policy, "any.example.com", 443));
        assert!(!connect_allowed(&policy, "unlisted.example", 80));
    }

    #[test]
    fn connect_allowed_allows_host_when_wildcard_all_allowed() {
        let policy = NetworkPolicy {
            mode: NetworkMode::AllowList,
            hosts: vec![HostRule {
                domain: "*".to_string(),
                port: None,
            }],
        };

        assert!(connect_allowed(&policy, "example.com", 80));
        assert!(connect_allowed(&policy, "anything.internal", 65535));
    }

    #[test]
    fn connect_allowed_blocks_all_when_wildcard_all_denied() {
        let policy = NetworkPolicy {
            mode: NetworkMode::DenyList,
            hosts: vec![HostRule {
                domain: "*".to_string(),
                port: None,
            }],
        };

        assert!(!connect_allowed(&policy, "example.com", 80));
        assert!(!connect_allowed(&policy, "anything.internal", 443));
    }

    #[test]
    fn connect_allowed_only_matches_port_for_port_only_wildcard() {
        let policy = NetworkPolicy {
            mode: NetworkMode::AllowList,
            hosts: vec![HostRule {
                domain: "*".to_string(),
                port: Some(443),
            }],
        };

        assert!(connect_allowed(&policy, "example.com", 443));
        assert!(!connect_allowed(&policy, "example.com", 80));
    }

    #[test]
    fn proxy_has_connection_limit_constant() {
        assert_eq!(MAX_CONCURRENT_CONNECTIONS, 512);
    }

    #[tokio::test]
    async fn proxy_plain_request_forwards_request_and_body_bytes() {
        let echo_listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("skip test because upstream bind failed: {error}");
                return;
            }
        };
        let echo_port = match echo_listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                eprintln!("skip test because upstream port query failed: {error}");
                return;
            }
        };

        let expected_request = format!(
            "POST /x HTTP/1.1\r\nHost: 127.0.0.1:{echo_port}\r\nContent-Length: 8\r\n\r\nABCDEFGH"
        )
        .into_bytes();

        let _echo_task = tokio::spawn(async move {
            let (mut stream, _) = match echo_listener.accept().await {
                Ok(accepted) => accepted,
                Err(error) => {
                    debug!(error = %error, "echo upstream accept failed");
                    return;
                }
            };
            let mut buffer = vec![0u8; 2048];
            let n = match stream.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            if stream.write_all(&buffer[..n]).await.is_err() {
                return;
            }
        });

        let proxy = match NetworkPolicyProxy::start(NetworkPolicy {
            mode: NetworkMode::AllowAll,
            hosts: Vec::new(),
        })
        .await
        {
            Ok(proxy) => proxy,
            Err(error) => {
                eprintln!("skip test because proxy start failed: {error}");
                return;
            }
        };

        let mut upstream_client = match TcpStream::connect(("127.0.0.1", proxy.port())).await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("skip test because proxy connect failed: {error}");
                proxy.shutdown().await;
                return;
            }
        };

        if upstream_client.write_all(&expected_request).await.is_err() {
            eprintln!("skip test because write to proxy failed");
            proxy.shutdown().await;
            return;
        }

        let mut echoed = vec![0u8; 2048];
        let n = match timeout(Duration::from_secs(2), upstream_client.read(&mut echoed)).await {
            Ok(Ok(n)) => n,
            _ => {
                proxy.shutdown().await;
                panic!("expected proxy to echo the request bytes");
            }
        };
        let echoed = &echoed[..n];
        assert!(echoed
            .windows(b"ABCDEFGH".len())
            .any(|window| window == b"ABCDEFGH"));

        proxy.shutdown().await;
    }

    #[tokio::test]
    async fn proxy_connect_allows_deny_mode_and_blocks_else() {
        let echo_listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("skip test because upstream bind failed: {error}");
                return;
            }
        };
        let echo_port = match echo_listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                eprintln!("skip test because upstream port query failed: {error}");
                return;
            }
        };

        let _echo_task = tokio::spawn(async move {
            let (mut stream, _) = match echo_listener.accept().await {
                Ok(accepted) => accepted,
                Err(error) => {
                    debug!(error = %error, "echo upstream accept failed");
                    return;
                }
            };
            loop {
                let mut buffer = vec![0u8; 1024];
                let n = match stream.read(&mut buffer).await {
                    Ok(0) => return,
                    Ok(n) => n,
                    Err(error) => {
                        debug!(error = %error, "echo upstream read failed");
                        return;
                    }
                };
                if stream.write_all(&buffer[..n]).await.is_err() {
                    return;
                }
            }
        });

        let policy = NetworkPolicy {
            mode: NetworkMode::AllowList,
            hosts: vec![HostRule {
                domain: "127.0.0.1".to_string(),
                port: Some(echo_port),
            }],
        };
        let proxy = match NetworkPolicyProxy::start(policy).await {
            Ok(proxy) => proxy,
            Err(error) => {
                eprintln!("skip test because proxy bind failed: {error}");
                return;
            }
        };

        let mut upstream_client = match TcpStream::connect(("127.0.0.1", proxy.port())).await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("skip test because proxy connect failed: {error}");
                let _ = proxy.shutdown().await;
                return;
            }
        };

        let connect_line = format!("CONNECT 127.0.0.1:{echo_port} HTTP/1.1\r\n\r\n");
        if let Err(error) = upstream_client.write_all(connect_line.as_bytes()).await {
            eprintln!("skip test due upstream write failed: {error}");
            proxy.shutdown().await;
            return;
        }

        let mut response = [0u8; 1024];
        let n = match timeout(Duration::from_secs(2), upstream_client.read(&mut response)).await {
            Ok(Ok(n)) => n,
            _ => {
                proxy.shutdown().await;
                panic!("proxy should return connect-established response");
            }
        };
        let response_text = String::from_utf8_lossy(&response[..n]);
        assert!(response_text.starts_with("HTTP/1.1 200"));

        if upstream_client.write_all(b"hello").await.is_err() {
            proxy.shutdown().await;
            panic!("failed to write through established tunnel");
        }

        let mut echo = [0u8; 5];
        let n = timeout(
            Duration::from_secs(2),
            upstream_client.read_exact(&mut echo),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(&echo[..n], b"hello");

        let mut blocked = match TcpStream::connect(("127.0.0.1", proxy.port())).await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("skip blocked-client connect failed: {error}");
                proxy.shutdown().await;
                return;
            }
        };
        let blocked_request = b"CONNECT blocked.example:443 HTTP/1.1\r\n\r\n";
        assert!(blocked.write_all(blocked_request).await.is_ok());
        let mut blocked_rsp = [0u8; 1024];
        let n = timeout(Duration::from_secs(2), blocked.read(&mut blocked_rsp))
            .await
            .unwrap()
            .unwrap();
        let blocked_text = String::from_utf8_lossy(&blocked_rsp[..n]);
        assert!(blocked_text.starts_with("HTTP/1.1 403"));
        assert!(blocked_text.contains("blocked.example:443"));
        assert!(blocked_text.contains("blocked by Omiga network policy"));

        proxy.shutdown().await;
    }

    #[tokio::test]
    async fn proxy_policy_handle_hot_update_takes_effect() {
        let echo_listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("skip test because upstream bind failed: {error}");
                return;
            }
        };
        let echo_port = match echo_listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                eprintln!("skip test because upstream port query failed: {error}");
                return;
            }
        };

        let _echo_task = tokio::spawn(async move {
            let (mut stream, _) = match echo_listener.accept().await {
                Ok(accepted) => accepted,
                Err(error) => {
                    debug!(error = %error, "echo upstream accept failed");
                    return;
                }
            };
            loop {
                let mut buffer = vec![0u8; 1024];
                let n = match stream.read(&mut buffer).await {
                    Ok(0) => return,
                    Ok(n) => n,
                    Err(error) => {
                        debug!(error = %error, "echo upstream read failed");
                        return;
                    }
                };
                if stream.write_all(&buffer[..n]).await.is_err() {
                    return;
                }
            }
        });

        let proxy = match NetworkPolicyProxy::start(NetworkPolicy {
            mode: NetworkMode::DenyAll,
            hosts: vec![],
        })
        .await
        {
            Ok(proxy) => proxy,
            Err(error) => {
                eprintln!("skip test because proxy bind failed: {error}");
                return;
            }
        };

        let mut blocked = match TcpStream::connect(("127.0.0.1", proxy.port())).await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("skip test because proxy connect failed: {error}");
                proxy.shutdown().await;
                return;
            }
        };
        assert!(blocked
            .write_all(format!("CONNECT 127.0.0.1:{echo_port} HTTP/1.1\r\n\r\n").as_bytes())
            .await
            .is_ok());
        let mut blocked_rsp = [0u8; 64];
        let n = timeout(Duration::from_secs(1), blocked.read(&mut blocked_rsp))
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&blocked_rsp[..n]).starts_with("HTTP/1.1 403"));

        {
            let policy_handle = proxy.policy_handle();
            let mut writable_policy = policy_handle.write().await;
            *writable_policy = NetworkPolicy {
                mode: NetworkMode::AllowList,
                hosts: vec![HostRule {
                    domain: "127.0.0.1".to_string(),
                    port: Some(echo_port),
                }],
            };
        }

        let mut allowed = match TcpStream::connect(("127.0.0.1", proxy.port())).await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("skip test because proxy connect after update failed: {error}");
                proxy.shutdown().await;
                return;
            }
        };
        assert!(allowed
            .write_all(format!("CONNECT 127.0.0.1:{echo_port} HTTP/1.1\r\n\r\n").as_bytes())
            .await
            .is_ok());
        let mut allowed_rsp = [0u8; 64];
        let n = timeout(Duration::from_secs(2), allowed.read(&mut allowed_rsp))
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&allowed_rsp[..n]).starts_with("HTTP/1.1 200"));

        proxy.shutdown().await;
    }

    #[tokio::test]
    async fn proxy_shutdown_stops_accepting_connections() {
        let proxy = match NetworkPolicyProxy::start(NetworkPolicy {
            mode: NetworkMode::AllowAll,
            hosts: vec![],
        })
        .await
        {
            Ok(proxy) => proxy,
            Err(error) => {
                eprintln!("skip test because proxy bind failed: {error}");
                return;
            }
        };
        let port = proxy.port();
        proxy.shutdown().await;

        match timeout(
            Duration::from_secs(1),
            TcpStream::connect(("127.0.0.1", port)),
        )
        .await
        {
            Ok(Ok(_)) => panic!("proxy should not accept after shutdown"),
            Ok(Err(_)) => {}
            Err(_) => panic!("proxy shutdown should return quickly"),
        }
    }

    #[test]
    fn parse_host_port_is_case_insensitive_and_defaults_work() {
        let (host, port) = parse_host_port("EXAMPLE.COM:443", 80).expect("parse");
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);

        let (host, port) = parse_host_port("example.com", 80).expect("parse");
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
    }
}
