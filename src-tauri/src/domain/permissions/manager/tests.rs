    use super::*;

    // -------------------------------------------------------------------------
    // 基础决策测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_safe_read_is_allowed() {
        let mgr = PermissionManager::new();
        let dec = mgr
            .check_tool("s1", "file_read", &serde_json::json!({"path": "README.md"}))
            .await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "file_read should be Allow"
        );
    }

    #[tokio::test]
    async fn test_dangerous_command_requires_approval() {
        let mgr = PermissionManager::new();
        let dec = mgr
            .check_tool("s1", "bash", &serde_json::json!({"command": "rm -rf /"}))
            .await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "rm -rf / should require approval"
        );
    }

    #[tokio::test]
    async fn connector_confirmed_write_requires_ui_approval() {
        let mgr = PermissionManager::new();
        let args = serde_json::json!({
            "connector": "slack",
            "operation": "post_message",
            "channel": "C123",
            "text": "Ship it",
            "confirm_write": true
        });

        let dec = mgr.check_tool("s_connector", "connector", &args).await;
        let req = match dec {
            PermissionDecision::RequireApproval(req) => req,
            other => panic!("expected connector write approval, got {other:?}"),
        };
        assert_eq!(req.risk.level, RiskLevel::Critical);
        assert!(req
            .risk
            .detected_risks
            .iter()
            .any(|risk| risk.description.contains("slack/post_message")));

        mgr.approve_request("s_connector", PermissionMode::AskEveryTime, &req.context)
            .await
            .unwrap();

        let allowed_once = mgr.check_tool("s_connector", "connector", &args).await;
        assert!(matches!(allowed_once, PermissionDecision::Allow));

        let requires_again = mgr.check_tool("s_connector", "connector", &args).await;
        assert!(matches!(
            requires_again,
            PermissionDecision::RequireApproval(_)
        ));
    }

    #[tokio::test]
    async fn computer_use_tool_risks_are_classified() {
        let mgr = PermissionManager::new();

        let observe = mgr
            .check_tool("s_computer", "computer_observe", &serde_json::json!({}))
            .await;
        let req = match observe {
            PermissionDecision::RequireApproval(req) => req,
            other => panic!("expected observe approval, got {other:?}"),
        };
        assert_eq!(req.risk.level, RiskLevel::Medium);

        let type_text = mgr
            .check_tool(
                "s_computer_type",
                "computer_type",
                &serde_json::json!({"text": "hello"}),
            )
            .await;
        let req = match type_text {
            PermissionDecision::RequireApproval(req) => req,
            other => panic!("expected type approval, got {other:?}"),
        };
        assert_eq!(req.risk.level, RiskLevel::High);

        let stop = mgr
            .check_tool("s_computer_stop", "computer_stop", &serde_json::json!({}))
            .await;
        assert!(matches!(stop, PermissionDecision::Allow));
    }

    #[tokio::test]
    async fn computer_type_probable_secret_forces_critical_single_use() {
        let mgr = PermissionManager::new();
        let args = serde_json::json!({"text": "password=hunter2"});

        let first = mgr
            .check_tool("s_computer_secret", "computer_type", &args)
            .await;
        let req = match first {
            PermissionDecision::RequireApproval(req) => req,
            other => panic!("expected secret approval, got {other:?}"),
        };
        assert_eq!(req.risk.level, RiskLevel::Critical);

        mgr.approve_request("s_computer_secret", PermissionMode::Session, &req.context)
            .await
            .unwrap();

        let allowed_once = mgr
            .check_tool("s_computer_secret", "computer_type", &args)
            .await;
        assert!(matches!(allowed_once, PermissionDecision::Allow));

        let second = mgr
            .check_tool("s_computer_secret", "computer_type", &args)
            .await;
        assert!(matches!(second, PermissionDecision::RequireApproval(_)));
    }

    #[tokio::test]
    async fn connector_write_approval_is_scoped_to_connector_operation() {
        let mgr = PermissionManager::new();
        let slack_post = serde_json::json!({
            "connector": "slack",
            "operation": "post_message",
            "channel": "C123",
            "text": "Ship it",
            "confirmWrite": true
        });
        let req = match mgr
            .check_tool("s_connector_scope", "Connector", &slack_post)
            .await
        {
            PermissionDecision::RequireApproval(req) => req,
            other => panic!("expected slack write approval, got {other:?}"),
        };
        mgr.approve_request("s_connector_scope", PermissionMode::Session, &req.context)
            .await
            .unwrap();
        assert!(matches!(
            mgr.check_tool("s_connector_scope", "connector", &slack_post)
                .await,
            PermissionDecision::Allow
        ));

        let linear_write = serde_json::json!({
            "connector": "linear",
            "operation": "update_issue_status",
            "id": "ENG-1",
            "confirm_write": true
        });
        assert!(matches!(
            mgr.check_tool("s_connector_scope", "connector", &linear_write)
                .await,
            PermissionDecision::RequireApproval(_)
        ));
    }

    #[tokio::test]
    async fn unconfirmed_connector_write_reaches_tool_level_guard_without_ui_prompt() {
        let mgr = PermissionManager::new();
        let dec = mgr
            .check_tool(
                "s_connector_unconfirmed",
                "connector",
                &serde_json::json!({
                    "connector": "slack",
                    "operation": "post_message",
                    "channel": "C123",
                    "text": "Ship it"
                }),
            )
            .await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "missing confirm_write should be blocked by connector tool guard, not a UI prompt"
        );
    }

    // -------------------------------------------------------------------------
    // 会话拒绝测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_deny_blocks_subsequent_calls() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "git status --short"}),
            session_id: "session_deny_test".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        // 第一次拒绝
        mgr.deny_request(&ctx, "用户拒绝").await.unwrap();

        // 后续同类命令应直接返回 Deny
        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Deny(_)),
            "后续调用应返回 Deny，实际: {:?}",
            dec
        );
        let ctx_same_class = PermissionContext {
            arguments: serde_json::json!({"command": "git status --porcelain"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx_same_class).await;
        assert!(
            matches!(dec2, PermissionDecision::Deny(_)),
            "同类 git status 仍应 Deny，实际: {:?}",
            dec2
        );

        let ctx_other_cmd = PermissionContext {
            arguments: serde_json::json!({"command": "git push origin main"}),
            ..ctx.clone()
        };
        let dec3 = mgr.check_permission(&ctx_other_cmd).await;
        assert!(
            !matches!(dec3, PermissionDecision::Deny(_)),
            "不同命令类别不应被 git status 的拒绝覆盖，实际: {:?}",
            dec3
        );
    }

    #[tokio::test]
    async fn test_deny_isolated_to_session() {
        let mgr = PermissionManager::new();
        let ctx_a = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "ls /tmp"}),
            session_id: "session_a".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let ctx_b = PermissionContext {
            session_id: "session_b".to_string(),
            ..ctx_a.clone()
        };

        mgr.deny_request(&ctx_a, "用户拒绝").await.unwrap();

        // session_b 不应受影响
        let dec = mgr.check_permission(&ctx_b).await;
        assert!(
            !matches!(dec, PermissionDecision::Deny(_)),
            "不同 session 不应受拒绝影响"
        );
    }

    // -------------------------------------------------------------------------
    // 会话批准测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_session_approve_bash_is_scoped_to_command_class() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "git status --short"}),
            session_id: "s_approve".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_approve", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "批准后应 Allow，实际: {:?}",
            dec
        );
        let ctx_same_class = PermissionContext {
            arguments: serde_json::json!({"command": "git status --porcelain"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx_same_class).await;
        assert!(
            matches!(dec2, PermissionDecision::Allow),
            "同类 git status 应共享本会话批准，实际: {:?}",
            dec2
        );

        let ctx_other = PermissionContext {
            arguments: serde_json::json!({"command": "git push origin main"}),
            ..ctx.clone()
        };
        let dec3 = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(dec3, PermissionDecision::RequireApproval(_)),
            "不同命令类别不应被 git status 的本会话批准覆盖，实际: {:?}",
            dec3
        );
    }

    #[tokio::test]
    async fn test_session_approve_bash_inline_code_does_not_allow_other_code() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({
                "command": "python3 - <<'PY'\nprint('safe')\nPY"
            }),
            session_id: "s_inline".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_inline", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        assert!(matches!(
            mgr.check_permission(&ctx).await,
            PermissionDecision::Allow
        ));

        let destructive = PermissionContext {
            arguments: serde_json::json!({"command": "rm -rf /tmp/omiga-inline-test"}),
            ..ctx.clone()
        };
        let dec = mgr.check_permission(&destructive).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "批准一段 inline Python 不应放行其它 bash 代码，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_session_approve_install_command_is_single_use() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "npm install left-pad"}),
            session_id: "s_install_once".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_install_once", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        assert!(matches!(
            mgr.check_permission(&ctx).await,
            PermissionDecision::Allow
        ));
        let second = mgr.check_permission(&ctx).await;
        assert!(
            matches!(second, PermissionDecision::RequireApproval(_)),
            "软件安装即使选择本会话也必须下次重新询问，实际: {:?}",
            second
        );
    }

    #[tokio::test]
    async fn test_session_approve_file_deletion_is_single_use() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "rm -rf /tmp/omiga-delete-test"}),
            session_id: "s_delete_once".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_delete_once", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        assert!(matches!(
            mgr.check_permission(&ctx).await,
            PermissionDecision::Allow
        ));
        let second = mgr.check_permission(&ctx).await;
        assert!(
            matches!(second, PermissionDecision::RequireApproval(_)),
            "文件删除即使选择本会话也必须下次重新询问，实际: {:?}",
            second
        );
    }

    #[tokio::test]
    async fn test_session_approve_wire_name_aliases_merge() {
        let mgr = PermissionManager::new();
        let ctx_read = PermissionContext {
            tool_name: "Read".to_string(),
            arguments: serde_json::json!({"path": "/a"}),
            session_id: "s_alias".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from("/a")]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        mgr.approve_request("s_alias", PermissionMode::Session, &ctx_read)
            .await
            .unwrap();
        let ctx_file_read = PermissionContext {
            tool_name: "file_read".to_string(),
            arguments: serde_json::json!({"path": "/b"}),
            file_paths: Some(vec![std::path::PathBuf::from("/b")]),
            ..ctx_read.clone()
        };
        let dec = mgr.check_permission(&ctx_file_read).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "Read 与 file_read 应共享会话批准，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_session_approve_file_write_different_paths() {
        let mgr = PermissionManager::new();
        let ctx_a = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": "/tmp/a.txt", "content": "x"}),
            session_id: "s_fw".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from("/tmp/a.txt")]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        mgr.approve_request("s_fw", PermissionMode::Session, &ctx_a)
            .await
            .unwrap();
        let ctx_b = PermissionContext {
            arguments: serde_json::json!({"path": "/tmp/b.txt", "content": "y"}),
            file_paths: Some(vec![std::path::PathBuf::from("/tmp/b.txt")]),
            ..ctx_a.clone()
        };
        let dec = mgr.check_permission(&ctx_b).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "file_write 不同路径应共享会话批准，实际: {:?}",
            dec
        );
    }

    /// Critical 风险（patterns 中如直接写磁盘设备）在未批准前走 Critical 分支；
    /// 本会话批准后仅相同命令类别命中缓存，不应覆盖其它 bash 命令。
    #[tokio::test]
    async fn test_session_approve_allows_critical_bash_same_args() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo x > /dev/sda"}),
            session_id: "s_crit".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        let before = mgr.check_permission(&ctx).await;
        assert!(
            matches!(before, PermissionDecision::RequireApproval(_)),
            "未批准时应 RequireApproval"
        );

        mgr.approve_request("s_crit", PermissionMode::Session, &ctx)
            .await
            .unwrap();

        let after = mgr.check_permission(&ctx).await;
        assert!(
            matches!(after, PermissionDecision::Allow),
            "本会话记住后应 Allow，实际: {:?}",
            after
        );
        let ctx_other = PermissionContext {
            arguments: serde_json::json!({"command": "dd if=/dev/zero of=/dev/sda"}),
            ..ctx.clone()
        };
        let after_other = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(after_other, PermissionDecision::RequireApproval(_)),
            "Critical bash 批准不应覆盖其它命令类别，实际: {:?}",
            after_other
        );
    }

    #[tokio::test]
    async fn test_time_window_approve() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hello"}),
            session_id: "s_tw".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.approve_request("s_tw", PermissionMode::TimeWindow { minutes: 60 }, &ctx)
            .await
            .unwrap();

        let dec = mgr.check_permission(&ctx).await;
        assert!(matches!(dec, PermissionDecision::Allow));
        let ctx2 = PermissionContext {
            arguments: serde_json::json!({"command": "echo different"}),
            ..ctx.clone()
        };
        let dec2 = mgr.check_permission(&ctx2).await;
        assert!(
            matches!(dec2, PermissionDecision::Allow),
            "时间窗口内同类命令应 Allow，实际: {:?}",
            dec2
        );
        let ctx3 = PermissionContext {
            arguments: serde_json::json!({"command": "rm -rf /tmp/omiga-time-window-test"}),
            ..ctx.clone()
        };
        let dec3 = mgr.check_permission(&ctx3).await;
        assert!(
            matches!(dec3, PermissionDecision::RequireApproval(_)),
            "时间窗口批准不应覆盖不同 bash 命令类别，实际: {:?}",
            dec3
        );
        let ctx4 = PermissionContext {
            arguments: serde_json::json!({"command": "echo $(rm -rf /tmp/omiga-substitution-test)"}),
            ..ctx.clone()
        };
        let dec4 = mgr.check_permission(&ctx4).await;
        assert!(
            matches!(dec4, PermissionDecision::RequireApproval(_)),
            "普通 echo 批准不应覆盖带命令替换的 shell 代码，实际: {:?}",
            dec4
        );
    }

    #[tokio::test]
    async fn test_approve_bypass_mode_rejected() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({}),
            session_id: "s".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let result = mgr.approve_request("s", PermissionMode::Bypass, &ctx).await;
        assert!(result.is_err(), "Bypass 模式应被拒绝");
    }

    #[tokio::test]
    async fn test_outside_project_path_requires_approval_even_in_auto_mode() {
        let mgr = PermissionManager::new();
        mgr.set_session_composer_stance("s_outside", Some("auto"))
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project");
        std::fs::create_dir_all(&project_root).expect("project dir");
        let outside_path = dir.path().join("outside.txt");

        let ctx = PermissionContext {
            tool_name: "file_read".to_string(),
            arguments: serde_json::json!({"path": outside_path.to_string_lossy()}),
            session_id: "s_outside".to_string(),
            file_paths: Some(vec![outside_path]),
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "工作区外路径即使在 auto 模式下也必须请求确认，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_check_tool_with_root_extracts_outside_project_paths() {
        let mgr = PermissionManager::new();
        mgr.set_session_composer_stance("s_outside_tool", Some("auto"))
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project");
        std::fs::create_dir_all(&project_root).expect("project dir");
        let outside_path = dir.path().join("outside.txt");

        let dec = mgr
            .check_tool_with_root(
                "s_outside_tool",
                "file_read",
                &serde_json::json!({"path": outside_path.to_string_lossy()}),
                Some(&project_root),
            )
            .await;

        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "check_tool_with_root 必须提取路径并拦截工作区外访问，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_time_window_overflow_rejected() {
        let mode = PermissionMode::TimeWindow { minutes: u32::MAX };
        assert!(
            mode.validate_user_mode().is_err(),
            "超大 TimeWindow 应被拒绝"
        );
    }

    // -------------------------------------------------------------------------
    // 规则有效期测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_use_limit_rule_expires() {
        let mgr = PermissionManager::new();

        // 添加一条 UseLimit(1) 的规则：允许 file_write，但只能用1次
        let rule = PermissionRule {
            id: "rule_ul".to_string(),
            name: "限制使用次数".to_string(),
            description: None,
            tool_matcher: ToolMatcher::Exact("file_write".to_string()),
            path_matcher: None,
            argument_conditions: vec![],
            mode: PermissionMode::Auto,
            validity: RuleValidity::UseLimit(1),
            priority: 0,
            created_at: chrono::Utc::now(),
            last_used_at: None,
            use_count: 0,
        };
        mgr.add_rule(rule).await.unwrap();

        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            session_id: "s_ul".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from("/tmp/test.txt")]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        // 第一次：规则有效（use_count=0 < limit=1），Auto + Medium risk → Allow
        let dec1 = mgr.check_permission(&ctx).await;
        assert!(matches!(dec1, PermissionDecision::Allow), "第一次应 Allow");

        // 第二次：规则已失效（use_count=1 >= limit=1），走 default_decision
        let dec2 = mgr.check_permission(&ctx).await;
        // file_write 在 assess_tool_risk 中为 Medium，default_decision 对 Medium 为 RequireApproval
        assert!(
            matches!(dec2, PermissionDecision::RequireApproval(_)),
            "规则失效后应按默认策略：Medium 风险 RequireApproval，实际: {:?}",
            dec2
        );
    }

    #[tokio::test]
    async fn test_current_session_rule_isolated() {
        let mgr = PermissionManager::new();

        let rule = PermissionRule {
            id: "rule_cs".to_string(),
            name: "当前会话规则".to_string(),
            description: None,
            tool_matcher: ToolMatcher::Exact("file_write".to_string()),
            path_matcher: None,
            argument_conditions: vec![],
            mode: PermissionMode::Auto,
            validity: RuleValidity::CurrentSession {
                session_id: "session_owner".to_string(),
            },
            priority: 0,
            created_at: chrono::Utc::now(),
            last_used_at: None,
            use_count: 0,
        };
        mgr.add_rule(rule).await.unwrap();

        let write_args = serde_json::json!({"path": "/tmp/current-session-test.txt"});

        // 规则创建者的会话：file_write 是 Medium 风险，Auto 规则 → Allow（Low/Medium 允许）
        let ctx_owner = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: write_args.clone(),
            session_id: "session_owner".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from(
                "/tmp/current-session-test.txt",
            )]),
            timestamp: chrono::Utc::now(),
            project_root: None,
        };
        let dec_owner = mgr.check_permission(&ctx_owner).await;
        assert!(
            matches!(dec_owner, PermissionDecision::Allow),
            "规则所有者 session 应 Allow"
        );

        // 其他会话：规则无效，走 default_decision → RequireApproval
        let ctx_other = PermissionContext {
            session_id: "session_other".to_string(),
            ..ctx_owner.clone()
        };
        let dec_other = mgr.check_permission(&ctx_other).await;
        assert!(
            matches!(dec_other, PermissionDecision::RequireApproval(_)),
            "其他 session 不应受 CurrentSession 规则影响"
        );
    }

    // -------------------------------------------------------------------------
    // 规则匹配测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_wildcard_matcher() {
        let mgr = PermissionManager::new();
        assert!(mgr.matches_tool_matcher(&ToolMatcher::Wildcard("file_*".to_string()), "file_read"));
        assert!(
            mgr.matches_tool_matcher(&ToolMatcher::Wildcard("file_*".to_string()), "file_write")
        );
        assert!(!mgr.matches_tool_matcher(&ToolMatcher::Wildcard("file_*".to_string()), "bash"));
    }

    #[tokio::test]
    async fn test_path_matcher_prefix() {
        let mgr = PermissionManager::new();
        let path = std::path::Path::new("/tmp/test.txt");
        assert!(mgr.matches_path_matcher(&PathMatcher::Prefix("/tmp/".to_string()), path));
        assert!(!mgr.matches_path_matcher(&PathMatcher::Prefix("/etc/".to_string()), path));
    }

    #[tokio::test]
    async fn test_path_matcher_extension() {
        let mgr = PermissionManager::new();
        let path = std::path::Path::new("/tmp/test.rs");
        assert!(mgr.matches_path_matcher(
            &PathMatcher::FileExtension(vec!["rs".to_string(), "toml".to_string()]),
            path
        ));
        assert!(
            !mgr.matches_path_matcher(&PathMatcher::FileExtension(vec!["py".to_string()]), path)
        );
    }

    #[tokio::test]
    async fn test_condition_ne() {
        let mgr = PermissionManager::new();
        let cond = ArgumentCondition {
            key: "cmd".to_string(),
            operator: ConditionOperator::Ne,
            value: serde_json::json!("rm -rf /"),
        };
        assert!(mgr.matches_condition(&cond, &serde_json::json!({"cmd": "ls"})));
        assert!(!mgr.matches_condition(&cond, &serde_json::json!({"cmd": "rm -rf /"})));
    }

    #[tokio::test]
    async fn test_condition_contains() {
        let mgr = PermissionManager::new();
        let cond = ArgumentCondition {
            key: "command".to_string(),
            operator: ConditionOperator::Contains,
            value: serde_json::json!("--force"),
        };
        assert!(mgr.matches_condition(&cond, &serde_json::json!({"command": "git push --force"})));
        assert!(!mgr.matches_condition(&cond, &serde_json::json!({"command": "git push"})));
    }

    #[tokio::test]
    async fn test_hash_no_prefix_collision() {
        // "a" + "bc" 与 "ab" + "c" 应产生不同 hash
        let h1 = PermissionManager::compute_tool_hash("a", &serde_json::json!("bc"));
        let h2 = PermissionManager::compute_tool_hash("ab", &serde_json::json!("c"));
        assert_ne!(h1, h2);
    }

    #[tokio::test]
    async fn test_hash_canonical_json_key_order() {
        // 键顺序不同但内容相同的 JSON 应产生相同 hash
        let args1 = serde_json::json!({"a": 1, "b": 2, "c": 3});
        let args2 = serde_json::json!({"c": 3, "a": 1, "b": 2});
        let h1 = PermissionManager::compute_tool_hash("test", &args1);
        let h2 = PermissionManager::compute_tool_hash("test", &args2);
        assert_eq!(
            h1, h2,
            "Canonical JSON should produce same hash regardless of key order"
        );
    }

    #[tokio::test]
    async fn test_hash_canonical_json_nested() {
        // 嵌套对象也应正确处理
        let args1 = serde_json::json!({"outer": {"a": 1, "b": 2}});
        let args2 = serde_json::json!({"outer": {"b": 2, "a": 1}});
        let h1 = PermissionManager::compute_tool_hash("test", &args1);
        let h2 = PermissionManager::compute_tool_hash("test", &args2);
        assert_eq!(h1, h2, "Nested objects should be canonicalized");
    }

    // -------------------------------------------------------------------------
    // 审计日志测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_denial_audit_log() {
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "rm -rf /"}),
            session_id: "s_audit".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: None,
        };

        mgr.deny_request(&ctx, "用户明确拒绝").await.unwrap();

        let denials = mgr.get_recent_denials(10).await;
        assert_eq!(denials.len(), 1);
        assert_eq!(denials[0].tool_name, "bash");
        assert_eq!(denials[0].reason, "用户明确拒绝");
    }

    // -------------------------------------------------------------------------
    // 工作区智能放行测试
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_workspace_file_write_auto_approved() {
        // file_write 在项目根目录内 → 自动放行，不弹窗
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().to_path_buf();
        let file_path = project_root.join("src/main.rs");

        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": file_path.to_str().unwrap(), "content": "fn main() {}"}),
            session_id: "s_ws".to_string(),
            file_paths: Some(vec![file_path.clone()]),
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "工作区内 file_write 应自动放行，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_workspace_file_edit_auto_approved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().to_path_buf();
        let file_path = project_root.join("Cargo.toml");

        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "file_edit".to_string(),
            arguments: serde_json::json!({"path": file_path.to_str().unwrap()}),
            session_id: "s_ws2".to_string(),
            file_paths: Some(vec![file_path.clone()]),
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "工作区内 file_edit 应自动放行，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_workspace_file_write_outside_requires_approval() {
        // file_write 超出项目根目录 → 必须弹窗
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().join("project");
        std::fs::create_dir_all(&project_root).expect("mkdir");
        let outside_path = dir.path().join("outside.txt");

        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": outside_path.to_str().unwrap()}),
            session_id: "s_ws3".to_string(),
            file_paths: Some(vec![outside_path.clone()]),
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "工作区外 file_write 必须弹窗，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_workspace_bash_safe_command_auto_approved() {
        // bash cargo build 在工作区内 → 自动放行
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().to_path_buf();

        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": "cargo build --release"}),
            session_id: "s_ws4".to_string(),
            file_paths: None, // 无绝对路径引用
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "工作区内安全 bash 命令应自动放行，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_workspace_bash_rm_rf_requires_approval() {
        // bash rm -rf 在工作区内也需要确认（DataLoss 类危险命令）
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().to_path_buf();

        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "bash".to_string(),
            arguments: serde_json::json!({"command": format!("rm -rf {}", project_root.join("target").display())}),
            session_id: "s_ws5".to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "工作区内 rm -rf 必须弹窗，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_workspace_no_root_configured_still_requires_approval() {
        // 未配置项目根目录时，Medium 风险仍需确认
        let mgr = PermissionManager::new();
        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            session_id: "s_ws6".to_string(),
            file_paths: Some(vec![std::path::PathBuf::from("/tmp/test.txt")]),
            timestamp: chrono::Utc::now(),
            project_root: None, // 未配置工作路径
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "未配置工作区时 file_write 必须弹窗，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_ask_stance_overrides_workspace_safe_bypass() {
        // ComposerPermissionStance::Ask 时，即使路径在工作区内也必须弹窗
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().to_path_buf();
        let file_path = project_root.join("src/lib.rs");

        let mgr = PermissionManager::new();
        let session_id = "s_ask_stance";

        // 设置 Ask 立场（raw string "ask" → ComposerPermissionStance::Ask）
        mgr.set_session_composer_stance(session_id, Some("ask")).await;

        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": file_path.to_str().unwrap()}),
            session_id: session_id.to_string(),
            file_paths: Some(vec![file_path.clone()]),
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "Ask 模式下工作区内写操作也必须弹窗，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_workspace_exclusion_blocks_auto_approve() {
        // 排除路径 "secrets/" 下的文件即使在工作区内也必须弹窗
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().to_path_buf();
        let secrets_dir = project_root.join("secrets");
        std::fs::create_dir_all(&secrets_dir).expect("mkdir secrets");
        let secret_file = secrets_dir.join("api_key.txt");

        let mgr = PermissionManager::new();
        mgr.set_workspace_exclusions(vec!["secrets/".to_string()]);

        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": secret_file.to_str().unwrap()}),
            session_id: "s_excl".to_string(),
            file_paths: Some(vec![secret_file.clone()]),
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::RequireApproval(_)),
            "排除路径内的文件必须弹窗，实际: {:?}",
            dec
        );
    }

    #[tokio::test]
    async fn test_workspace_non_excluded_path_still_auto_approved() {
        // 未被排除的路径仍然自动放行
        let dir = tempfile::tempdir().expect("tempdir");
        let project_root = dir.path().to_path_buf();
        let src_file = project_root.join("src").join("main.rs");
        std::fs::create_dir_all(src_file.parent().unwrap()).expect("mkdir src");

        let mgr = PermissionManager::new();
        mgr.set_workspace_exclusions(vec!["secrets/".to_string(), "dist/".to_string()]);

        let ctx = PermissionContext {
            tool_name: "file_write".to_string(),
            arguments: serde_json::json!({"path": src_file.to_str().unwrap()}),
            session_id: "s_not_excl".to_string(),
            file_paths: Some(vec![src_file.clone()]),
            timestamp: chrono::Utc::now(),
            project_root: Some(project_root.clone()),
        };

        let dec = mgr.check_permission(&ctx).await;
        assert!(
            matches!(dec, PermissionDecision::Allow),
            "排除路径之外的工作区文件应自动放行，实际: {:?}",
            dec
        );
    }
