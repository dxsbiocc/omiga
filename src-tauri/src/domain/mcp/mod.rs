//! MCP (Model Context Protocol) module
//!
//! This module provides comprehensive MCP support for Omiga:
//! - **client**: Low-level MCP connection handling via rmcp
//! - **config**: MCP server configuration parsing and merging
//! - **connection_manager**: Session-aware connection pool with lifecycle management
//! - **discovery**: MCP tool discovery and schema generation
//! - **names**: MCP tool name normalization and parsing
//! - **resource_output**: MCP resource output formatting
//! - **tool_dispatch**: Tool call dispatch with connection pooling
//! - **tool_pool**: Tool execution pool management

pub mod client;
pub mod config;
pub mod connection_manager;
pub mod discovery;
pub mod names;
pub mod resource_output;
pub mod tool_dispatch;
pub mod tool_pool;

// Re-export commonly used types for convenience
pub use client::{McpConnectionType, McpLiveConnection, connect_mcp_server, connect_mcp_server_legacy};
pub use config::{McpServerConfig, merged_mcp_servers};
pub use connection_manager::{McpConnectionManager, GlobalMcpManager, ConnectionStats};
pub use discovery::collect_mcp_server_names;
pub use names::{normalize_name_for_mcp, parse_mcp_tool_name};
pub use tool_dispatch::execute_mcp_tool_call;
pub use tool_pool::discover_mcp_tool_schemas;
