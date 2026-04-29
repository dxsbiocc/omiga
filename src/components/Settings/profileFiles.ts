import {
  AutoAwesome,
  Person,
  Psychology,
} from "@mui/icons-material";
import type { SvgIconComponent } from "@mui/icons-material";

export type ProfileFilename = "SOUL.md" | "USER.md" | "MEMORY.md";

export interface ProfileFileDefinition {
  filename: ProfileFilename;
  label: string;
  role: string;
  description: string;
  icon: SvgIconComponent;
  prompts: string[];
}

export const PROFILE_FILE_DEFINITIONS: ProfileFileDefinition[] = [
  {
    filename: "SOUL.md",
    label: "Agent 身份",
    role: "定义助手是谁、语气、边界与核心准则",
    description:
      "保存 Agent 的长期身份与沟通气质。每次对话都会编译进 Permanent Profile，让智能体保持一致的风格和边界。",
    icon: AutoAwesome,
    prompts: [
      "名字、定位、核心专长",
      "语气、回答长度、语言偏好",
      "必须坚持的准则和需要确认的边界",
    ],
  },
  {
    filename: "USER.md",
    label: "用户偏好",
    role: "描述用户画像、沟通偏好与长期偏好",
    description:
      "保存关于你的稳定信息：称呼、角色、技术栈、沟通习惯，以及不希望 Agent 出现的行为。",
    icon: Person,
    prompts: [
      "你希望被如何称呼、当前角色和技术栈",
      "回复风格：先结论、步骤化、直接给代码等",
      "不要过度解释、不要反复确认等禁忌",
    ],
  },
  {
    filename: "MEMORY.md",
    label: "习惯与笔记",
    role: "记录工具链、工作习惯、踩坑与环境约束",
    description:
      "保存长期手写记忆：常用命令、工作区习惯、环境限制、反复踩坑。适合写稳定事实，不适合写密钥。",
    icon: Psychology,
    prompts: [
      "常用工具、包管理器、远程/本地环境约束",
      "仓库和工作区的反复约定",
      "已经踩过的坑，以及下次需要避免的操作",
    ],
  },
];

export interface ProfileMarkdownSummary {
  charCount: number;
  meaningfulLineCount: number;
  placeholderCount: number;
}

export function summarizeProfileMarkdown(content: string): ProfileMarkdownSummary {
  const lines = content.split(/\r?\n/u);
  const meaningfulLineCount = lines.filter((line) => {
    const trimmed = line.trim();
    return (
      trimmed.length > 0 &&
      !trimmed.startsWith("#") &&
      trimmed !== "---" &&
      !trimmed.startsWith(">") &&
      !/^<!--.*-->$/u.test(trimmed)
    );
  }).length;

  return {
    charCount: content.length,
    meaningfulLineCount,
    placeholderCount: (content.match(/<!--/gu) ?? []).length,
  };
}

export function getProfileFileDefinition(filename: ProfileFilename): ProfileFileDefinition {
  return (
    PROFILE_FILE_DEFINITIONS.find((definition) => definition.filename === filename) ??
    PROFILE_FILE_DEFINITIONS[0]
  );
}

