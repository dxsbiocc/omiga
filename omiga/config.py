"""
Configuration module for Omiga Python port.

Uses Pydantic Settings for type-safe configuration management.
Reads from .env file (falls back to environment variables).
Secrets are NOT loaded here — they are read only where needed
(container_runner.py) to avoid leaking to child processes.
"""

import os
import re
import time
from datetime import datetime
from pathlib import Path
from typing import Optional

from dotenv import dotenv_values
from pydantic import Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class OmigaSettings(BaseSettings):
    """Omiga 配置管理。

    使用 Pydantic Settings 进行类型安全的配置管理。
    支持 .env 文件和环境变量两种方式。
    """

    model_config = SettingsConfigDict(
        env_file=".env",
        env_file_encoding="utf-8",
        case_sensitive=False,
        extra="ignore",
    )

    # ========== 基础配置 ==========
    assistant_name: str = Field(default="Omiga", description="助手名称")
    assistant_has_own_number: bool = Field(
        default=False,
        description="助手是否有自己的手机号（用于 WhatsApp 等）",
    )

    # ========== 时间间隔配置（秒） ==========
    poll_interval: float = Field(default=2.0, description="消息轮询间隔（秒）")
    scheduler_poll_interval: float = Field(default=60.0, description="调度器轮询间隔（秒）")
    ipc_poll_interval: float = Field(default=1.0, description="IPC 轮询间隔（秒）")
    message_debounce_seconds: float = Field(default=2.0, description="消息防抖间隔（秒）")

    # ========== 路径配置 ==========
    mount_allowlist_path: Optional[Path] = None
    store_dir: Optional[Path] = None
    groups_dir: Optional[Path] = None
    data_dir: Optional[Path] = None
    media_dir: Optional[Path] = None

    # ========== 主组配置 ==========
    main_group_folder: str = Field(default="main", description="主组文件夹名称")
    main_group_jid: str = Field(default="", description="主组 JID（如 tg:123456789）")
    main_group_name: str = Field(default="Main", description="主组名称")

    # ========== 容器配置 ==========
    container_image: str = Field(default="omiga-agent:latest", description="容器镜像名称")
    container_timeout: int = Field(default=1800000, description="容器超时（毫秒）")  # 30 分钟
    container_max_output_size: int = Field(default=10485760, description="容器最大输出（字节）")  # 10MB
    idle_timeout: int = Field(default=1800000, description="空闲超时（毫秒）")
    max_concurrent_containers: int = Field(default=5, ge=1, le=20, description="最大并发容器数")

    # ========== 时区配置 ==========
    timezone: Optional[str] = Field(default=None, description="时区设置")

    # ========== HTTP API 配置 ==========
    http_api_port: int = Field(default=0, description="HTTP API 端口（0=禁用）")
    http_api_host: str = Field(default="127.0.0.1", description="HTTP API 监听地址")
    http_api_token: str = Field(default="", description="HTTP API 认证令牌")

    # ========== 语音转录配置 ==========
    whisper_enabled: bool = Field(default=False, description="是否启用 Whisper 语音转录")
    whisper_language: str = Field(default="", description="Whisper 语言提示（BCP-47 格式）")

    # ========== 触发模式 ==========
    trigger_pattern_raw: str = Field(default="", description="自定义触发模式（留空=使用默认）")

    # ========== 直接模式配置（开发用） ==========
    direct_mode: bool = Field(default=False, description="是否启用直接模式（无 Docker）")

    @field_validator("container_timeout", "idle_timeout")
    @classmethod
    def validate_timeout(cls, v: int) -> int:
        """验证超时值必须在合理范围内。"""
        if v < 10000:  # 最小 10 秒
            raise ValueError("Timeout must be at least 10000ms (10 seconds)")
        if v > 3600000:  # 最大 1 小时
            raise ValueError("Timeout must be at most 3600000ms (1 hour)")
        return v

    @field_validator("timezone")
    @classmethod
    def validate_timezone(cls, v: Optional[str]) -> str:
        """验证时区设置。"""
        if not v:
            # 自动检测时区
            try:
                # 使用 tzname 获取系统时区缩写，然后映射到 IANA 时区
                import time
                tzname = time.tzname[0] if time.daylight == 0 else time.tzname[1]

                # CST 有多种可能，在中国通常指 Asia/Shanghai
                if tzname == "CST":
                    return "Asia/Shanghai"

                # 尝试使用 zoneinfo 获取标准时区名
                try:
                    import zoneinfo
                    # 从系统获取本地时区
                    local = datetime.now().astimezone()
                    # 使用 offset 来查找匹配的时区
                    offset = local.utcoffset().total_seconds() / 3600
                    tz = zoneinfo.ZoneInfo(local.tzname() if not tzname.startswith(("GMT", "UTC")) else f"Etc/GMT{ '+' if offset < 0 else '-' }{abs(offset):.0f}")
                    return str(tz)
                except Exception:
                    pass

                # 回退到 UTC
                return "UTC"
            except Exception:
                pass
            return "UTC"
        return v

    @property
    def project_root(self) -> Path:
        """项目根目录。"""
        return Path(__file__).parent.parent.resolve()

    @property
    def home_dir(self) -> Path:
        """用户主目录。"""
        return Path.home()

    @property
    def mount_allowlist_path_resolved(self) -> Path:
        """挂载白名单路径。"""
        return self.mount_allowlist_path or (self.home_dir / ".config" / "omiga" / "mount-allowlist.json")

    @property
    def store_dir_resolved(self) -> Path:
        """存储目录。"""
        return self.store_dir or (self.project_root / "store")

    @property
    def groups_dir_resolved(self) -> Path:
        """群组目录。"""
        return self.groups_dir or (self.project_root / "groups")

    @property
    def data_dir_resolved(self) -> Path:
        """数据目录。"""
        return self.data_dir or (self.project_root / "data")

    @property
    def media_dir_resolved(self) -> Path:
        """媒体目录。"""
        return self.media_dir or (self.data_dir_resolved / "media")

    def get_trigger_pattern(self) -> re.Pattern:
        """获取触发模式正则表达式。"""
        pattern = self.trigger_pattern_raw or f"^@{re.escape(self.assistant_name)}\\b"
        return re.compile(pattern, re.IGNORECASE)


# ========== 全局配置实例 ==========
_settings: Optional[OmigaSettings] = None


def get_settings() -> OmigaSettings:
    """获取配置单例实例。"""
    global _settings
    if _settings is None:
        _settings = OmigaSettings()
    return _settings


# ========== 向后兼容的快捷访问（全局常量） ==========
# 这些常量从单例配置中派生，保持现有代码兼容

_settings_instance = get_settings()

# Assistant identity
ASSISTANT_NAME: str = _settings_instance.assistant_name
ASSISTANT_HAS_OWN_NUMBER: bool = _settings_instance.assistant_has_own_number

# Polling intervals (秒)
POLL_INTERVAL: float = _settings_instance.poll_interval
SCHEDULER_POLL_INTERVAL: float = _settings_instance.scheduler_poll_interval
IPC_POLL_INTERVAL: float = _settings_instance.ipc_poll_interval
MESSAGE_DEBOUNCE_SECONDS: float = _settings_instance.message_debounce_seconds

# Absolute paths
PROJECT_ROOT: Path = _settings_instance.project_root
HOME_DIR: Path = _settings_instance.home_dir
MOUNT_ALLOWLIST_PATH: Path = _settings_instance.mount_allowlist_path_resolved
STORE_DIR: Path = _settings_instance.store_dir_resolved
GROUPS_DIR: Path = _settings_instance.groups_dir_resolved
DATA_DIR: Path = _settings_instance.data_dir_resolved
MEDIA_DIR: Path = _settings_instance.media_dir_resolved
MAIN_GROUP_FOLDER: str = _settings_instance.main_group_folder
MAIN_GROUP_JID: str = _settings_instance.main_group_jid
MAIN_GROUP_NAME: str = _settings_instance.main_group_name

# Container settings
CONTAINER_IMAGE: str = _settings_instance.container_image
CONTAINER_TIMEOUT: int = _settings_instance.container_timeout
CONTAINER_MAX_OUTPUT_SIZE: int = _settings_instance.container_max_output_size
IDLE_TIMEOUT: int = _settings_instance.idle_timeout
MAX_CONCURRENT_CONTAINERS: int = _settings_instance.max_concurrent_containers

# Timezone
TIMEZONE: str = _settings_instance.timezone

# HTTP API
HTTP_API_PORT: int = _settings_instance.http_api_port
HTTP_API_HOST: str = _settings_instance.http_api_host
HTTP_API_TOKEN: str = _settings_instance.http_api_token

# Voice transcription
WHISPER_ENABLED: bool = _settings_instance.whisper_enabled
WHISPER_LANGUAGE: str = _settings_instance.whisper_language

# Trigger pattern
TRIGGER_PATTERN: re.Pattern = _settings_instance.get_trigger_pattern()


# ========== 保留原有的 get_secret 函数 ==========
def get_secret(key: str) -> str:
    """Read a secret value from the environment or .env file.

    Unlike the module-level _get() helper, this is intentionally public so
    that startup code can load secrets (e.g. TELEGRAM_BOT_TOKEN) without
    caching them in module-level constants.
    """
    _env = dotenv_values(_settings_instance.project_root / ".env") if (_settings_instance.project_root / ".env").exists() else {}
    return os.environ.get(key) or _env.get(key) or ""
