//! 风险评估逻辑：工具风险分类、路径越界检测、文件路径提取。
//! 这是 `PermissionManager` 的子模块，可访问父模块的私有字段。

use super::super::types::*;
use super::bash_path_regex;
use crate::domain::connectors;

impl super::PermissionManager {
    /// 风险评估
    pub(super) async fn assess_risk(&self, context: &PermissionContext) -> RiskAssessment {
        let mut detected_risks = Vec::new();
        let mut categories = Vec::new();

        // 1. 工具级别风险
        let tool_risk = self.assess_tool_risk(&context.tool_name);
        detected_risks.extend(tool_risk.detected_risks);
        categories.extend(tool_risk.categories);

        // 1b. Connector 写操作具有外部服务副作用；只有模型显式声明
        // confirm_write=true 时才进入统一 UI 审批。未声明确认的写操作会先由
        // connector 工具自身拦截并记录为 blocked，避免无意义地弹审批框。
        if super::PermissionManager::approval_cache_key(&context.tool_name) == "connector"
            && connectors::connector_permission_write_confirmed(&context.arguments)
            && connectors::connector_permission_write_operation_from_args(&context.arguments)
                .is_some()
        {
            let (connector_id, operation) =
                connectors::connector_permission_identity_from_args(&context.arguments)
                    .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
            detected_risks.push(DetectedRisk {
                category: RiskCategory::Network,
                severity: RiskLevel::Critical,
                description: format!("Connector 写操作将修改外部服务: {connector_id}/{operation}"),
                mitigation: Some(
                    "请确认目标、内容和账号无误；批准后仍会写入 connector 审计日志。".to_string(),
                ),
            });
            detected_risks.push(DetectedRisk {
                category: RiskCategory::Privacy,
                severity: RiskLevel::Medium,
                description: "外部服务可能接收当前对话生成的内容".to_string(),
                mitigation: Some("避免发送 secret、token、未确认的私有信息。".to_string()),
            });
            categories.push(RiskCategory::Network);
            categories.push(RiskCategory::Privacy);
        }

        if super::PermissionManager::approval_cache_key(&context.tool_name) == "computer_type"
            && crate::domain::computer_use::value_contains_probable_secret(&context.arguments)
        {
            detected_risks.push(DetectedRisk {
                category: RiskCategory::Privacy,
                severity: RiskLevel::Critical,
                description: "Computer Use 将输入疑似 secret/token/password".to_string(),
                mitigation: Some(
                    "确认目标窗口可信；该输入只允许单次批准，审计日志会脱敏保存。".to_string(),
                ),
            });
            categories.push(RiskCategory::Privacy);
        }

        // 2. 参数级别风险（bash/shell 危险命令检测）
        if context.tool_name == "bash" || context.tool_name == "shell" {
            if let Some(cmd) = context.arguments.get("command").and_then(|v| v.as_str()) {
                let pattern_risks = self.patterns.check(cmd);
                for risk in &pattern_risks {
                    categories.push(risk.category.clone());
                }
                detected_risks.extend(pattern_risks);
            }
        }

        // 3. 路径风险
        let home_dir = dirs::home_dir();
        if let Some(ref paths) = context.file_paths {
            for path in paths {
                let path_str = path.to_string_lossy();

                // 3a. 系统路径
                if path_str.starts_with("/etc/")
                    || path_str.starts_with("/boot/")
                    || path_str.starts_with("/sys/")
                {
                    detected_risks.push(DetectedRisk {
                        category: RiskCategory::System,
                        severity: RiskLevel::High,
                        description: format!("访问系统路径: {}", path_str),
                        mitigation: Some("确认是否真的需要修改系统文件".to_string()),
                    });
                    categories.push(RiskCategory::System);
                }

                // 3b. 敏感文件
                if path_str.contains(".env")
                    || path_str.contains("secret")
                    || path_str.contains("credential")
                {
                    detected_risks.push(DetectedRisk {
                        category: RiskCategory::Privacy,
                        severity: RiskLevel::Medium,
                        description: format!("可能涉及敏感文件: {}", path_str),
                        mitigation: Some("确认是否需要修改此文件".to_string()),
                    });
                    categories.push(RiskCategory::Privacy);
                }

                // 3c. 项目根目录之外的写操作
                if let Some(ref root) = context.project_root {
                    let abs_path = if path.is_absolute() {
                        path.clone()
                    } else {
                        root.join(path)
                    };
                    let canonical_root = std::fs::canonicalize(root).unwrap_or(root.clone());
                    let canonical_path =
                        std::fs::canonicalize(&abs_path).unwrap_or(abs_path.clone());

                    if !canonical_path.starts_with(&canonical_root) {
                        // 判断是否直接在 home 目录下（更危险）
                        let is_home_level = home_dir
                            .as_ref()
                            .map(|h| {
                                let canonical_home = std::fs::canonicalize(h).unwrap_or(h.clone());
                                // 直接子目录或文件（depth == home + 1）
                                canonical_path.starts_with(&canonical_home)
                                    && canonical_path
                                        .strip_prefix(&canonical_home)
                                        .map(|rel| rel.components().count() <= 1)
                                        .unwrap_or(false)
                            })
                            .unwrap_or(false);

                        let (severity, desc) = if is_home_level {
                            (
                                RiskLevel::High,
                                format!(
                                    "操作路径在 Home 目录根层级 (~/)，超出项目范围: {}",
                                    path_str
                                ),
                            )
                        } else {
                            (
                                RiskLevel::High,
                                format!("操作路径超出项目目录: {}", path_str),
                            )
                        };
                        detected_risks.push(DetectedRisk {
                            category: RiskCategory::FileSystem,
                            severity,
                            description: desc,
                            mitigation: Some(format!(
                                "项目根目录为 {}，请确认是否允许访问外部路径",
                                root.display()
                            )),
                        });
                        categories.push(RiskCategory::FileSystem);
                    }
                }
            }
        }

        // 3d. bash 命令中的路径越界检测（mkdir / touch / cp 等写入外部路径）
        if let (Some(ref root), true) = (
            &context.project_root,
            context.tool_name == "bash" || context.tool_name == "shell",
        ) {
            if let Some(cmd) = context.arguments.get("command").and_then(|v| v.as_str()) {
                for cap in bash_path_regex().captures_iter(cmd) {
                    let raw = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
                    // Expand ~ to home dir
                    let expanded = if let Some(stripped) = raw.strip_prefix("~/") {
                        home_dir
                            .as_ref()
                            .map(|h| h.join(stripped))
                            .unwrap_or_else(|| std::path::PathBuf::from(raw))
                    } else if raw == "~" {
                        home_dir
                            .clone()
                            .unwrap_or_else(|| std::path::PathBuf::from(raw))
                    } else {
                        std::path::PathBuf::from(raw)
                    };

                    if !expanded.is_absolute() {
                        continue;
                    }

                    let canonical_root = std::fs::canonicalize(root).unwrap_or(root.clone());
                    let canonical_path =
                        std::fs::canonicalize(&expanded).unwrap_or(expanded.clone());

                    if !canonical_path.starts_with(&canonical_root) {
                        let is_home_level = home_dir
                            .as_ref()
                            .map(|h| {
                                let ch = std::fs::canonicalize(h).unwrap_or(h.clone());
                                canonical_path.starts_with(&ch)
                                    && canonical_path
                                        .strip_prefix(&ch)
                                        .map(|rel| rel.components().count() <= 1)
                                        .unwrap_or(false)
                            })
                            .unwrap_or(false);

                        if is_home_level {
                            detected_risks.push(DetectedRisk {
                                category: RiskCategory::FileSystem,
                                severity: RiskLevel::High,
                                description: format!(
                                    "命令中包含 Home 根层级路径 (~/)，超出项目范围: {}",
                                    raw
                                ),
                                mitigation: Some(format!(
                                    "项目根目录为 {}，该路径超出允许范围",
                                    root.display()
                                )),
                            });
                            categories.push(RiskCategory::FileSystem);
                        }
                    }
                }
            }
        }

        // 计算总体风险等级
        let max_level = detected_risks
            .iter()
            .map(|r| r.severity)
            .max()
            .unwrap_or(RiskLevel::Safe);

        let description = if detected_risks.is_empty() {
            format!("使用工具: {}", context.tool_name)
        } else {
            format!("检测到 {} 个风险点", detected_risks.len())
        };

        let recommendations: Vec<String> = detected_risks
            .iter()
            .filter_map(|r| r.mitigation.clone())
            .collect();

        categories.sort();
        categories.dedup();

        RiskAssessment {
            level: max_level,
            categories,
            description,
            recommendations,
            detected_risks,
        }
    }

    pub(super) fn assess_tool_risk(&self, tool_name: &str) -> RiskAssessment {
        match tool_name {
            "bash" | "shell" => RiskAssessment {
                level: RiskLevel::High,
                categories: vec![RiskCategory::System],
                description: "执行系统命令".to_string(),
                recommendations: vec![
                    "仔细检查命令内容".to_string(),
                    "避免使用 rm -rf 等危险命令".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::System,
                    severity: RiskLevel::High,
                    description: "允许执行任意系统命令".to_string(),
                    mitigation: Some("使用受限的 file_* 工具替代".to_string()),
                }],
            },
            "file_write" | "file_edit" | "skill_manage" | "skill_config" => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![RiskCategory::FileSystem, RiskCategory::DataLoss],
                description: "修改文件内容".to_string(),
                recommendations: vec![
                    "确认文件路径正确".to_string(),
                    "重要文件建议先备份".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::FileSystem,
                    severity: RiskLevel::Medium,
                    description: "将修改文件内容".to_string(),
                    mitigation: Some("使用 file_read 先查看当前内容".to_string()),
                }],
            },
            "file_read" | "glob" | "grep" | "read_mcp_resource" => RiskAssessment {
                level: RiskLevel::Safe,
                categories: vec![],
                description: "读取操作（安全）".to_string(),
                recommendations: vec![],
                detected_risks: vec![],
            },
            "connector" | "fetch" | "query" | "search" => RiskAssessment {
                level: RiskLevel::Low,
                categories: vec![RiskCategory::Network],
                description: "网络请求".to_string(),
                recommendations: vec!["确认 URL 可信".to_string()],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::Network,
                    severity: RiskLevel::Low,
                    description: "将访问外部网络".to_string(),
                    mitigation: None,
                }],
            },
            "computer_observe" => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![RiskCategory::Privacy],
                description: "观察本机屏幕/前台窗口".to_string(),
                recommendations: vec!["确认当前屏幕未显示不应共享的敏感内容".to_string()],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::Privacy,
                    severity: RiskLevel::Medium,
                    description: "可能读取屏幕截图、窗口标题或页面内容".to_string(),
                    mitigation: Some("只在需要本机 UI 自动化的任务中开启 Computer Use".to_string()),
                }],
            },
            "computer_set_target" => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![RiskCategory::System],
                description: "选择本机目标应用/窗口".to_string(),
                recommendations: vec!["确认目标应用和窗口正确".to_string()],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::System,
                    severity: RiskLevel::Medium,
                    description: "可能切换或聚焦本机应用窗口".to_string(),
                    mitigation: Some("跨 App 操作前应显式确认目标".to_string()),
                }],
            },
            "computer_click" | "computer_click_element" => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![RiskCategory::System],
                description: "在本机目标窗口执行点击".to_string(),
                recommendations: vec!["确认最近 observation 与当前窗口一致".to_string()],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::System,
                    severity: RiskLevel::Medium,
                    description: "将向本机应用发送鼠标点击".to_string(),
                    mitigation: Some(
                        "动作前由 Computer Use facade 执行观察/限速/stop 检查".to_string(),
                    ),
                }],
            },
            "computer_type" => RiskAssessment {
                level: RiskLevel::High,
                categories: vec![RiskCategory::Privacy, RiskCategory::System],
                description: "向本机目标窗口输入文本".to_string(),
                recommendations: vec![
                    "确认目标窗口可信".to_string(),
                    "不要输入 secret/token/password，除非用户明确要求".to_string(),
                ],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::Privacy,
                    severity: RiskLevel::High,
                    description: "将向本机应用输入可能包含隐私的数据".to_string(),
                    mitigation: Some("Computer Use 审计日志会脱敏保存输入".to_string()),
                }],
            },
            "computer_stop" => RiskAssessment {
                level: RiskLevel::Safe,
                categories: vec![],
                description: "停止 Computer Use 运行".to_string(),
                recommendations: vec![],
                detected_risks: vec![],
            },
            // 其他常见安全工具
            "list_skills" | "skills_list" | "skill_view" | "tool_search" | "get_current_time"
            | "get_system_info" => RiskAssessment {
                level: RiskLevel::Safe,
                categories: vec![],
                description: format!("使用工具: {}", tool_name),
                recommendations: vec![],
                detected_risks: vec![],
            },
            // 默认：未知工具视为 Medium 风险（需要确认）
            _ => RiskAssessment {
                level: RiskLevel::Medium,
                categories: vec![],
                description: format!("使用工具: {}", tool_name),
                recommendations: vec!["未知工具，请确认是否允许执行".to_string()],
                detected_risks: vec![DetectedRisk {
                    category: RiskCategory::Security,
                    severity: RiskLevel::Medium,
                    description: format!("未识别的工具: {}", tool_name),
                    mitigation: Some("如果是 Skill 工具，可以在权限设置中添加规则".to_string()),
                }],
            },
        }
    }

    /// 从工具参数中提取文件路径
    pub(crate) fn extract_file_paths(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Option<Vec<std::path::PathBuf>> {
        let mut paths = Vec::new();

        match tool_name {
            "file_read" | "file_write" | "file_edit" => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    paths.push(std::path::PathBuf::from(path));
                }
            }
            "bash" | "shell" => {
                if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                    for cap in bash_path_regex().captures_iter(cmd) {
                        if let Some(m) = cap.get(1) {
                            paths.push(std::path::PathBuf::from(m.as_str()));
                        }
                    }
                }
            }
            _ => {}
        }

        if paths.is_empty() {
            None
        } else {
            Some(paths)
        }
    }
}
