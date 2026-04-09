//! Agent 选择器
//!
//! 根据任务描述自动选择最合适的 Agent。



/// Agent 匹配分数
#[derive(Debug, Clone)]
pub struct AgentMatch {
    /// Agent 类型
    pub agent_type: String,
    /// 匹配分数 (0-100)
    pub score: u8,
    /// 匹配原因
    pub reason: String,
    /// 预估执行时间
    pub estimated_duration_secs: u64,
}

/// Agent 选择器
pub struct AgentSelector {
    // 可以添加缓存等
}

impl AgentSelector {
    pub fn new() -> Self {
        Self {}
    }

    /// 为任务选择最佳 Agent
    pub fn select(&self, task_description: &str, _project_root: &str) -> String {
        // 1. 基于关键词的选择
        let keywords = self.extract_keywords(task_description);
        
        // 2. 根据关键词匹配 Agent
        let matches = self.match_agents(&keywords, task_description);
        
        // 3. 返回最佳匹配
        if let Some(best) = matches.first() {
            best.agent_type.clone()
        } else {
            // 默认使用 general-purpose
            "general-purpose".to_string()
        }
    }

    /// 获取所有匹配的 Agent（按分数排序）
    pub fn select_all(&self, task_description: &str) -> Vec<AgentMatch> {
        let keywords = self.extract_keywords(task_description);
        self.match_agents(&keywords, task_description)
    }

    /// 提取任务关键词
    fn extract_keywords(&self, description: &str) -> Vec<String> {
        let lower = description.to_lowercase();
        let mut keywords = Vec::new();

        // 代码探索相关
        if self.matches_any(&lower, &["find", "search", "locate", "where", "look for"]) {
            keywords.push("explore".to_string());
        }
        if self.matches_any(&lower, &[
            "codebase", "files", "structure", "module", "directory", "folder",
            "代码库", "文件", "结构", "模块", "目录"
        ]) {
            keywords.push("explore".to_string());
        }

        // 规划设计相关
        if self.matches_any(&lower, &[
            "design", "plan", "architecture", "structure", "organize",
            "设计", "规划", "架构", "方案", "重构"
        ]) {
            keywords.push("plan".to_string());
        }
        if self.matches_any(&lower, &[
            "implement", "create", "build", "add feature", "new feature",
            "实现", "创建", "构建", "添加功能", "新功能"
        ]) {
            keywords.push("plan".to_string());
        }

        // 验证测试相关
        if self.matches_any(&lower, &[
            "verify", "test", "check", "validate", "review", "audit",
            "验证", "测试", "检查", "审查", "审计", "确保"
        ]) {
            keywords.push("verification".to_string());
        }
        if self.matches_any(&lower, &[
            "bug", "issue", "problem", "error", "fix", "correct",
            "bug", "问题", "错误", "修复", "修正"
        ]) {
            keywords.push("verification".to_string());
        }

        // 代码修改相关（需要 General-Purpose）
        if self.matches_any(&lower, &[
            "edit", "modify", "change", "update", "refactor", "rewrite",
            "编辑", "修改", "更改", "更新", "重构", "重写"
        ]) {
            keywords.push("general-purpose".to_string());
        }

        // 复杂分析相关
        if self.matches_any(&lower, &[
            "analyze", "investigate", "research", "understand", "complex",
            "分析", "调查", "研究", "理解", "复杂"
        ]) {
            keywords.push("general-purpose".to_string());
        }

        // 内容生成类任务（需要详细输出）
        if self.matches_any(&lower, &[
            "travel", "itinerary", "plan", "guide", "recommendation",
            "旅行", "行程", "攻略", "推荐", "计划",
            "write", "document", "draft", "create content",
            "写", "文档", "起草", "内容"
        ]) {
            keywords.push("content-generation".to_string());
        }

        keywords
    }

    /// 匹配 Agent
    fn match_agents(&self, keywords: &[String], description: &str) -> Vec<AgentMatch> {
        let mut matches = Vec::new();
        let desc_lower = description.to_lowercase();

        // Explore Agent 匹配
        if keywords.iter().any(|k| k == "explore")
            || self.is_explore_task(&desc_lower) {
            let score = self.calculate_explore_score(&desc_lower);
            matches.push(AgentMatch {
                agent_type: "Explore".to_string(),
                score,
                reason: "任务涉及代码搜索和探索".to_string(),
                estimated_duration_secs: 30 + (description.len() / 50) as u64,
            });
        }

        // Plan Agent 匹配
        if keywords.iter().any(|k| k == "plan")
            || self.is_plan_task(&desc_lower) {
            let score = self.calculate_plan_score(&desc_lower);
            matches.push(AgentMatch {
                agent_type: "Plan".to_string(),
                score,
                reason: "任务需要架构设计和规划".to_string(),
                estimated_duration_secs: 60 + (description.len() / 30) as u64,
            });
        }

        // Verification Agent 匹配
        if keywords.iter().any(|k| k == "verification")
            || self.is_verification_task(&desc_lower) {
            let score = self.calculate_verification_score(&desc_lower);
            matches.push(AgentMatch {
                agent_type: "verification".to_string(),
                score,
                reason: "任务需要验证和测试".to_string(),
                estimated_duration_secs: 120,
            });
        }

        // General-Purpose 作为默认选择
        let gp_score = self.calculate_general_score(&desc_lower);
        matches.push(AgentMatch {
            agent_type: "general-purpose".to_string(),
            score: gp_score,
            reason: "通用任务执行".to_string(),
            estimated_duration_secs: 60 + (description.len() / 40) as u64,
        });

        // 按分数排序（降序）
        matches.sort_by(|a, b| b.score.cmp(&a.score));
        matches
    }

    /// 判断是否为 Explore 任务
    fn is_explore_task(&self, desc: &str) -> bool {
        self.matches_any(desc, &[
            "find all", "search for", "look for", "where is", "locate",
            "list all", "show me", "what files", "which module",
            "找到", "搜索", "查找", "在哪里", "列出"
        ])
    }

    /// 判断是否为 Plan 任务
    fn is_plan_task(&self, desc: &str) -> bool {
        self.matches_any(desc, &[
            "how to", "design a", "plan for", "architecture for",
            "best way to", "approach to", "strategy for",
            "如何", "设计一个", "规划", "架构"
        ])
    }

    /// 判断是否为 Verification 任务
    fn is_verification_task(&self, desc: &str) -> bool {
        self.matches_any(desc, &[
            "verify", "validate", "check if", "test the", "ensure",
            "make sure", "confirm", "audit", "review",
            "验证", "确认", "检查", "确保", "测试"
        ])
    }

    /// 计算 Explore 匹配分数
    fn calculate_explore_score(&self, desc: &str) -> u8 {
        let mut score = 50;
        
        // 强烈的探索信号
        if desc.contains("search") || desc.contains("搜索") { score += 20; }
        if desc.contains("find all") || desc.contains("找到所有") { score += 20; }
        if desc.contains("codebase") || desc.contains("代码库") { score += 15; }
        if desc.contains("structure") || desc.contains("结构") { score += 10; }
        
        // 排除其他类型信号
        if desc.contains("implement") || desc.contains("实现") { score -= 20; }
        if desc.contains("edit") || desc.contains("修改") { score -= 20; }
        
        score.clamp(0, 100)
    }

    /// 计算 Plan 匹配分数
    fn calculate_plan_score(&self, desc: &str) -> u8 {
        let mut score = 50;
        
        if desc.contains("design") || desc.contains("设计") { score += 25; }
        if desc.contains("architecture") || desc.contains("架构") { score += 20; }
        if desc.contains("implement") || desc.contains("实现") { score += 15; }
        if desc.contains("plan") || desc.contains("规划") { score += 20; }
        
        score.clamp(0, 100)
    }

    /// 计算 Verification 匹配分数
    fn calculate_verification_score(&self, desc: &str) -> u8 {
        let mut score = 50;
        
        if desc.contains("verify") || desc.contains("验证") { score += 25; }
        if desc.contains("test") || desc.contains("测试") { score += 20; }
        if desc.contains("check") || desc.contains("检查") { score += 15; }
        if desc.contains("bug") || desc.contains("错误") { score += 15; }
        
        score.clamp(0, 100)
    }

    /// 计算 General-Purpose 匹配分数
    fn calculate_general_score(&self, desc: &str) -> u8 {
        let mut score = 40; // 基础分数较低
        
        // 复杂任务信号
        if desc.contains("complex") || desc.contains("复杂") { score += 15; }
        if desc.contains("multiple") || desc.contains("多个") { score += 10; }
        if desc.contains("analyze") || desc.contains("分析") { score += 15; }
        
        // 修改信号（必须是 General-Purpose）
        if desc.contains("edit") || desc.contains("修改") { score += 25; }
        if desc.contains("change") || desc.contains("更改") { score += 25; }
        if desc.contains("update") || desc.contains("更新") { score += 20; }
        
        score.clamp(0, 100)
    }

    /// 辅助函数：检查是否匹配任意关键词
    fn matches_any(&self, text: &str, patterns: &[&str]) -> bool {
        patterns.iter().any(|p| text.contains(p))
    }
}

impl Default for AgentSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// 快速选择函数
pub fn select_agent_for_task(task_description: &str) -> String {
    let selector = AgentSelector::new();
    selector.select(task_description, ".")
}

/// 获取所有候选 Agent（带分数）
pub fn select_agents_with_scores(task_description: &str) -> Vec<AgentMatch> {
    let selector = AgentSelector::new();
    selector.select_all(task_description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_explore_for_search() {
        let selector = AgentSelector::new();
        let agent = selector.select("Find all files that use User model", ".");
        assert_eq!(agent, "Explore");
    }

    #[test]
    fn test_select_plan_for_design() {
        let selector = AgentSelector::new();
        let agent = selector.select("Design an authentication system", ".");
        assert_eq!(agent, "Plan");
    }

    #[test]
    fn test_select_verification_for_testing() {
        let selector = AgentSelector::new();
        let agent = selector.select("Verify the implementation is correct", ".");
        assert_eq!(agent, "verification");
    }

    #[test]
    fn test_select_general_for_editing() {
        let selector = AgentSelector::new();
        let agent = selector.select("Edit the main.rs file to add logging", ".");
        assert_eq!(agent, "general-purpose");
    }

    #[test]
    fn test_chinese_keywords() {
        let selector = AgentSelector::new();
        let agent = selector.select("搜索所有使用 User 模型的文件", ".");
        assert_eq!(agent, "Explore");
    }
}
