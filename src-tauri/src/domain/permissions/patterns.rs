//! 危险模式检测数据库

use super::types::{DetectedRisk, RiskCategory, RiskLevel};
use regex::Regex;

pub struct DangerousPatternDB {
    patterns: Vec<DangerousPattern>,
}

pub struct DangerousPattern {
    pub regex: Regex,
    pub severity: RiskLevel,
    pub category: RiskCategory,
    pub description: String,
    pub mitigation: Option<String>,
}

impl DangerousPatternDB {
    pub fn new() -> Self {
        let mut db = Self { patterns: Vec::new() };
        db.load_default_patterns();
        db
    }
    
    fn load_default_patterns(&mut self) {
        // CRITICAL: Fork bomb
        self.add(DangerousPattern {
            regex: Regex::new(r":\(\)\s*\{.*:\s*\|.*:\s*&.*\}").unwrap(),
            severity: RiskLevel::Critical,
            category: RiskCategory::System,
            description: "Fork bomb - 会导致系统资源耗尽".to_string(),
            mitigation: Some("这是一个恶意命令，永远不要执行".to_string()),
        });
        
        // CRITICAL: Direct disk write
        self.add(DangerousPattern {
            regex: Regex::new(r">\s*/dev/sd[a-z]").unwrap(),
            severity: RiskLevel::Critical,
            category: RiskCategory::DataLoss,
            description: "直接写入磁盘设备（会覆盖分区表）".to_string(),
            mitigation: Some("使用 dd 时注意 of= 参数".to_string()),
        });
        
        // HIGH: chmod 777 on root
        self.add(DangerousPattern {
            regex: Regex::new(r"chmod\s+-R\s+777\s+/").unwrap(),
            severity: RiskLevel::High,
            category: RiskCategory::Security,
            description: "递归修改根目录权限为 777".to_string(),
            mitigation: Some("使用更精确的权限设置".to_string()),
        });
        
        // MEDIUM: rm -rf
        self.add(DangerousPattern {
            regex: Regex::new(r"rm\s+-rf").unwrap(),
            severity: RiskLevel::Medium,
            category: RiskCategory::DataLoss,
            description: "递归强制删除".to_string(),
            mitigation: Some("确认目标路径，考虑使用 -i 交互模式".to_string()),
        });
        
        // MEDIUM: Remote script execution
        self.add(DangerousPattern {
            regex: Regex::new(r"curl.*\|\s*sh|wget.*\|\s*sh").unwrap(),
            severity: RiskLevel::Medium,
            category: RiskCategory::Security,
            description: "管道执行远程脚本（可能有恶意代码）".to_string(),
            mitigation: Some("先下载查看脚本内容，确认安全后再执行".to_string()),
        });
        
        // MEDIUM: Privilege escalation
        self.add(DangerousPattern {
            regex: Regex::new(r"\bsudo\b|\bsu\s+-").unwrap(),
            severity: RiskLevel::Medium,
            category: RiskCategory::Security,
            description: "提权操作".to_string(),
            mitigation: Some("确认命令来源可信".to_string()),
        });
        
        // HIGH: System file modification
        self.add(DangerousPattern {
            regex: Regex::new(r"/etc/(passwd|shadow|sudoers)").unwrap(),
            severity: RiskLevel::High,
            category: RiskCategory::System,
            description: "修改系统关键文件".to_string(),
            mitigation: Some("这些文件修改可能导致系统无法登录".to_string()),
        });
    }
    
    pub fn add(&mut self, pattern: DangerousPattern) {
        self.patterns.push(pattern);
    }
    
    pub fn check(&self, command: &str) -> Vec<DetectedRisk> {
        let mut risks = Vec::new();
        
        for pattern in &self.patterns {
            if pattern.regex.is_match(command) {
                risks.push(DetectedRisk {
                    category: pattern.category.clone(),
                    severity: pattern.severity,
                    description: pattern.description.clone(),
                    mitigation: pattern.mitigation.clone(),
                });
            }
        }
        
        risks
    }
}

impl Default for DangerousPatternDB {
    fn default() -> Self {
        Self::new()
    }
}
