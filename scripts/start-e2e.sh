#!/bin/bash
# E2E Test Launch Script for Omiga
#
# Supports multiple LLM providers:
# International:
#   - Anthropic (Claude): ANTHROPIC_API_KEY
#   - OpenAI: OPENAI_API_KEY
#   - Azure: AZURE_OPENAI_KEY
#   - Google: GOOGLE_API_KEY or GEMINI_API_KEY
#
# Domestic (Chinese):
#   - DeepSeek: DEEPSEEK_API_KEY
#   - 阿里通义千问: DASHSCOPE_API_KEY
#   - 智谱 ChatGLM: ZHIPU_API_KEY
#   - 百度文心一言: BAIDU_API_KEY + BAIDU_SECRET_KEY
#   - 讯飞星火: XUNFEI_APP_ID + XUNFEI_API_KEY + XUNFEI_SECRET_KEY
#   - 月之暗面: MOONSHOT_API_KEY
#   - MiniMax: MINIMAX_API_KEY
#
# Custom/Local:
#   - LLM_BASE_URL + LLM_API_KEY
#
# Or simply set: LLM_PROVIDER + LLM_API_KEY
#
# Config File:
#   - Copy config.example.yaml to omiga.yaml and configure your keys

set -e

echo "🚀 Omiga E2E Test Launcher"
echo "=========================="

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Function to check if any API key is configured
check_api_key() {
    # International providers
    if [ -n "$ANTHROPIC_API_KEY" ]; then
        echo -e "${GREEN}✓ ANTHROPIC_API_KEY configured${NC}"
        return 0
    fi
    if [ -n "$OPENAI_API_KEY" ]; then
        echo -e "${GREEN}✓ OPENAI_API_KEY configured${NC}"
        return 0
    fi
    if [ -n "$AZURE_OPENAI_KEY" ]; then
        echo -e "${GREEN}✓ AZURE_OPENAI_KEY configured${NC}"
        return 0
    fi
    if [ -n "$GOOGLE_API_KEY" ] || [ -n "$GEMINI_API_KEY" ]; then
        echo -e "${GREEN}✓ Google/Gemini API Key configured${NC}"
        return 0
    fi

    # Domestic Chinese providers
    if [ -n "$DEEPSEEK_API_KEY" ]; then
        echo -e "${GREEN}✓ DeepSeek API Key configured${NC}"
        return 0
    fi
    if [ -n "$DASHSCOPE_API_KEY" ] || [ -n "$ALIBABA_API_KEY" ]; then
        echo -e "${GREEN}✓ 阿里通义千问 API Key configured${NC}"
        return 0
    fi
    if [ -n "$ZHIPU_API_KEY" ]; then
        echo -e "${GREEN}✓ 智谱 ChatGLM API Key configured${NC}"
        return 0
    fi
    if [ -n "$BAIDU_API_KEY" ]; then
        echo -e "${GREEN}✓ 百度文心 API Key configured${NC}"
        return 0
    fi
    if [ -n "$XUNFEI_API_KEY" ] || [ -n "$XUNFEI_APP_ID" ]; then
        echo -e "${GREEN}✓ 讯飞星火 API configured${NC}"
        return 0
    fi
    if [ -n "$MOONSHOT_API_KEY" ]; then
        echo -e "${GREEN}✓ 月之暗面 API Key configured${NC}"
        return 0
    fi
    if [ -n "$MINIMAX_API_KEY" ]; then
        echo -e "${GREEN}✓ MiniMax API Key configured${NC}"
        return 0
    fi

    # Generic
    if [ -n "$LLM_API_KEY" ]; then
        echo -e "${GREEN}✓ LLM_API_KEY configured${NC}"
        return 0
    fi

    # Check for config file
    if [ -f "omiga.yaml" ] || [ -f "omiga.yml" ] || [ -f "omiga.json" ] || [ -f "omiga.toml" ]; then
        echo -e "${GREEN}✓ Configuration file found${NC}"
        return 0
    fi
    if [ -f "$HOME/.config/omiga/omiga.yaml" ] || [ -f "$HOME/.config/omiga/omiga.yml" ] || [ -f "$HOME/.config/omiga/omiga.json" ] || [ -f "$HOME/.config/omiga/omiga.toml" ]; then
        echo -e "${GREEN}✓ Config file found at ~/.config/omiga/${NC}"
        return 0
    fi
    if [ -f "$HOME/.omiga/omiga.yaml" ] || [ -f "$HOME/.omiga/omiga.yml" ] || [ -f "$HOME/.omiga/omiga.json" ] || [ -f "$HOME/.omiga/omiga.toml" ]; then
        echo -e "${GREEN}✓ Config file found at ~/.omiga/${NC}"
        return 0
    fi

    return 1
}

require_bun() {
    if ! command -v bun >/dev/null 2>&1; then
        echo -e "${RED}✗ Bun 1.x is required for Omiga JavaScript commands${NC}" >&2
        echo "Install Bun and rerun this script. This repository intentionally does not use npm install." >&2
        exit 1
    fi
}

# Check API Key
echo -e "${BLUE}Checking LLM API configuration...${NC}"
echo ""

if ! check_api_key; then
    echo -e "${YELLOW}⚠️  No API key configured${NC}"
    echo ""
    echo -e "${CYAN}International Providers (国际厂商):${NC}"
    echo ""
    echo "  Anthropic (Claude):"
    echo "    export ANTHROPIC_API_KEY='your-key-here'"
    echo ""
    echo "  OpenAI:"
    echo "    export OPENAI_API_KEY='your-key-here'"
    echo ""
    echo "  Azure OpenAI:"
    echo "    export AZURE_OPENAI_KEY='your-key-here'"
    echo "    export LLM_BASE_URL='https://your-resource.openai.azure.com/...'"
    echo ""
    echo "  Google Gemini:"
    echo "    export GOOGLE_API_KEY='your-key-here'"
    echo ""
    echo -e "${CYAN}Domestic Chinese Providers (国产模型):${NC}"
    echo ""
    echo "  DeepSeek (https://platform.deepseek.com):"
    echo "    export DEEPSEEK_API_KEY='your-key-here'"
    echo "    export LLM_PROVIDER='deepseek'"
    echo ""
    echo "  阿里通义千问 (https://dashscope.aliyun.com):"
    echo "    export DASHSCOPE_API_KEY='your-key-here'"
    echo "    export LLM_PROVIDER='alibaba'"
    echo ""
    echo "  智谱 ChatGLM (https://open.bigmodel.cn):"
    echo "    export ZHIPU_API_KEY='your-key-here'"
    echo "    export LLM_PROVIDER='zhipu'"
    echo ""
    echo "  百度文心一言 (https://ai.baidu.com):"
    echo "    export BAIDU_API_KEY='your-key-here'"
    echo "    export BAIDU_SECRET_KEY='your-secret-here'"
    echo "    export LLM_PROVIDER='baidu'"
    echo ""
    echo "  讯飞星火 (https://xinghuo.xfyun.cn):"
    echo "    export XUNFEI_APP_ID='your-app-id'"
    echo "    export XUNFEI_API_KEY='your-key-here'"
    echo "    export XUNFEI_SECRET_KEY='your-secret-here'"
    echo "    export LLM_PROVIDER='xunfei'"
    echo ""
    echo "  月之暗面 Moonshot (https://platform.moonshot.cn):"
    echo "    export MOONSHOT_API_KEY='your-key-here'"
    echo "    export LLM_PROVIDER='moonshot'"
    echo ""
    echo "  MiniMax:"
    echo "    export MINIMAX_API_KEY='your-key-here'"
    echo "    export LLM_PROVIDER='minimax'"
    echo ""
    echo -e "${CYAN}Local/Custom Providers (本地模型):${NC}"
    echo ""
    echo "  Ollama (本地运行):"
    echo "    export LLM_PROVIDER='custom'"
    echo "    export LLM_BASE_URL='http://localhost:11434/v1/chat/completions'"
    echo "    export LLM_MODEL='llama3.1'"
    echo "    export LLM_API_KEY='ollama'"
    echo ""
    echo -e "${CYAN}Configuration File (配置文件方式):${NC}"
    echo ""
    echo "  Copy and edit the example config:"
    echo "    cp config.example.yaml omiga.yaml"
    echo "    # Edit omiga.yaml with your keys, then run:"
    echo "    ./scripts/start-e2e.sh"
    echo ""
    echo "  Or use the generic LLM_API_KEY with provider selection:"
    echo "    export LLM_API_KEY='your-key-here'"
    echo "    export LLM_PROVIDER='deepseek'  # or anthropic, openai, alibaba, zhipu, etc."
    echo ""
    echo "You can also create a .env file in src-tauri/ directory:"
    echo "    echo 'LLM_API_KEY=your-key-here' > src-tauri/.env"
    exit 1
fi

# Show provider info if explicitly set
if [ -n "$LLM_PROVIDER" ]; then
    echo -e "${BLUE}LLM Provider: $LLM_PROVIDER${NC}"
fi
if [ -n "$LLM_MODEL" ]; then
    echo -e "${BLUE}LLM Model: $LLM_MODEL${NC}"
fi
if [ -n "$LLM_BASE_URL" ]; then
    echo -e "${BLUE}LLM Base URL: $LLM_BASE_URL${NC}"
fi

# Check if we're in the right directory
if [ ! -f "package.json" ]; then
    echo -e "${RED}✗ Not in omiga project directory${NC}"
    echo "Please run from omiga/ directory"
    exit 1
fi

echo -e "${GREEN}✓ In correct directory${NC}"
require_bun

# Install dependencies if needed
if [ ! -d "node_modules" ]; then
    echo "📦 Installing frontend dependencies..."
    if [ -f "bun.lock" ]; then
        bun install --frozen-lockfile
    else
        bun install
    fi
fi

if [ ! -d "src-tauri/target" ]; then
    echo "📦 Rust dependencies will be fetched on first build..."
fi

echo ""
echo "🎯 Starting E2E Test Environment..."
echo ""
echo "This will:"
echo "  1. Start the Vite dev server (frontend)"
echo "  2. Launch Tauri with hot-reload"
echo ""
echo "Test URLs:"
echo "  - App: http://localhost:5173"
echo "  - Test Tool: http://localhost:5173/session-flow-test.html"
echo ""
echo "Press Ctrl+C to stop"
echo "=========================="
echo ""

# Start Tauri in dev mode
bun run tauri dev
