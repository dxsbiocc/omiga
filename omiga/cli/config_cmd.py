"""Config command - initialize and manage Omiga configuration."""
from __future__ import annotations

import click
from pathlib import Path


@click.group("config")
def config_cmd() -> None:
    """Manage Omiga configuration."""
    pass


@config_cmd.command("init")
@click.option(
    "--force",
    is_flag=True,
    help="Overwrite existing .env file",
)
@click.option(
    "--defaults",
    "use_defaults",
    is_flag=True,
    help="Use defaults only, no interactive prompts",
)
def init_cmd(force: bool, use_defaults: bool) -> None:
    """Initialize Omiga configuration.

    Creates a .env file with default configuration values.
    """
    from omiga.config import PROJECT_ROOT

    env_path = PROJECT_ROOT / ".env"

    # Check if .env already exists
    if env_path.exists() and not force:
        if use_defaults:
            click.echo(".env already exists, skipping.")
            return

        if not click.confirm(".env already exists. Overwrite?"):
            click.echo("Configuration initialization skipped.")
            return

    # Default configuration template
    config_template = """# Omiga Configuration
# Copy to .env and fill in values

# Assistant name (used for trigger pattern @NAME)
ASSISTANT_NAME=Omiga

# Whether the assistant has its own WhatsApp number
ASSISTANT_HAS_OWN_NUMBER=false

# Container configuration
CONTAINER_IMAGE=omiga-agent:latest
CONTAINER_TIMEOUT=1800000
CONTAINER_MAX_OUTPUT_SIZE=10485760
MAX_CONCURRENT_CONTAINERS=5

# Timing
IDLE_TIMEOUT=1800000

# Timezone (defaults to system timezone)
# TZ=America/New_York

# Logging
# LOG_LEVEL=info

# ── HTTP API Server ───────────────────────────────────────────────────────────
# REST API for remote management. Disabled if HTTP_API_PORT=0.
# Endpoints: GET /status /groups /chats /tasks, POST /groups, DELETE /groups/{jid}
#            POST /tasks/{id}/run, GET /workspace/backup
#
HTTP_API_PORT=7891
HTTP_API_HOST=127.0.0.1
# HTTP_API_TOKEN=your-secret-token   # optional Bearer token for auth

# ── Main Group (Personal Chat) ────────────────────────────────────────────────
# Optional: JID of your personal chat or private group.
# When set, omiga auto-registers it as the "main" group at startup.
# The main group NEVER needs a trigger word — every message goes to the agent.
#
# MAIN_GROUP_JID=tg:123456789
# MAIN_GROUP_NAME=My Personal Chat

# ── Telegram Channel ──────────────────────────────────────────────────────────
# Get a token from @BotFather: /newbot
# TELEGRAM_BOT_TOKEN=

# ── Voice Transcription (OpenAI Whisper) ─────────────────────────────────────
# WHISPER_ENABLED=true
# OPENAI_API_KEY=sk-...

# ── AI Provider ───────────────────────────────────────────────────────────────
# Works with any OpenAI-compatible provider.
#
# DeepSeek:
#   AI_API_KEY=sk-...
#   AI_BASE_URL=https://api.deepseek.com/v1
#   AI_MODEL=deepseek-chat
#
# Qwen (Alibaba DashScope):
#   AI_API_KEY=sk-...
#   AI_BASE_URL=https://dashscope.aliyuncs.com/compatible-mode/v1
#   AI_MODEL=qwen-plus
"""

    if use_defaults:
        # Write defaults silently
        env_path.parent.mkdir(parents=True, exist_ok=True)
        env_path.write_text(config_template, encoding="utf-8")
        click.echo(f"✓ Configuration initialized at {env_path}")
        return

    # Interactive configuration
    click.echo("\n=== Omiga Configuration ===\n")

    # Assistant name
    assistant_name = click.prompt(
        "Assistant name",
        default="Omiga",
        type=str,
    )

    # API configuration
    click.echo("\n--- AI Provider Configuration ---")
    ai_provider = click.prompt(
        "AI Provider",
        type=click.Choice(["deepseek", "qwen", "custom"], case_sensitive=False),
        default="deepseek",
    )

    ai_api_key = click.prompt("AI API Key", type=str, hide_input=True)

    if ai_provider == "deepseek":
        ai_base_url = "https://api.deepseek.com/v1"
        ai_model = "deepseek-chat"
    elif ai_provider == "qwen":
        ai_base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
        ai_model = "qwen-plus"
    else:
        ai_base_url = click.prompt("AI Base URL", type=str)
        ai_model = click.prompt("AI Model", type=str)

    # HTTP API
    click.echo("\n--- HTTP API Configuration ---")
    http_port = click.prompt("HTTP API Port", type=int, default=7891)

    # Build .env content
    env_content = f"""# Omiga Configuration

# Assistant name
ASSISTANT_NAME={assistant_name}
ASSISTANT_HAS_OWN_NUMBER=false

# Container configuration
CONTAINER_IMAGE=omiga-agent:latest
CONTAINER_TIMEOUT=1800000
CONTAINER_MAX_OUTPUT_SIZE=10485760
MAX_CONCURRENT_CONTAINERS=5

# Timing
IDLE_TIMEOUT=1800000

# HTTP API Server
HTTP_API_PORT={http_port}
HTTP_API_HOST=127.0.0.1

# AI Provider
AI_API_KEY={ai_api_key}
AI_BASE_URL={ai_base_url}
AI_MODEL={ai_model}

# Optional: Main group JID for personal chat
# MAIN_GROUP_JID=
# MAIN_GROUP_NAME=

# Optional: Telegram bot token
# TELEGRAM_BOT_TOKEN=

# Optional: Voice transcription
# WHISPER_ENABLED=true
# OPENAI_API_KEY=
"""

    # Write configuration
    env_path.parent.mkdir(parents=True, exist_ok=True)
    env_path.write_text(env_content, encoding="utf-8")

    click.echo(f"\n✓ Configuration saved to {env_path}")
    click.echo("\nNext steps:")
    click.echo("  1. Review and edit .env if needed")
    click.echo("  2. Run 'omiga app' to start the server")
    click.echo("  3. Open http://127.0.0.1:{port} in your browser".format(port=http_port))


@config_cmd.command("show")
def show_cmd() -> None:
    """Show current configuration."""
    from omiga.config import (
        ASSISTANT_NAME,
        HTTP_API_HOST,
        HTTP_API_PORT,
        TIMEZONE,
        PROJECT_ROOT,
    )

    click.echo("\n=== Omiga Configuration ===\n")
    click.echo(f"Project root:    {PROJECT_ROOT}")
    click.echo(f"Assistant name:  {ASSISTANT_NAME}")
    click.echo(f"HTTP API:        {HTTP_API_HOST}:{HTTP_API_PORT}")
    click.echo(f"Timezone:        {TIMEZONE}")
    click.echo("")
