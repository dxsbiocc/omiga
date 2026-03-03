"""System utilities skill for Omiga."""
from __future__ import annotations

import logging
import os
import platform
import shutil
from datetime import datetime, timezone
from typing import Any, Dict, Optional

from omiga.skills.base import Skill, SkillContext, SkillMetadata, SkillError

logger = logging.getLogger("omiga.skills.system")


class SystemSkill(Skill):
    """Skill for system utilities."""

    metadata = SkillMetadata(
        name="system",
        description="系统工具 - 时间、日期、系统信息",
        emoji="⚙️",
        tags=["system", "time", "date", "info"],
    )

    def __init__(self, context: SkillContext):
        super().__init__(context)

    async def execute(
        self,
        action: str,
        **kwargs: Any,
    ) -> Any:  # type: ignore[override]
        """Execute the system skill.

        Args:
            action: Action to perform (time, date, datetime, system_info, env, user, disk)
            **kwargs: Action-specific arguments

        Returns:
            Result of the system operation
        """
        actions = {
            "time": self._get_time,
            "date": self._get_date,
            "datetime": self._get_datetime,
            "system_info": self._get_system_info,
            "env": self._get_env,
            "user": self._get_user,
            "disk": self._get_disk_usage,
            "cwd": self._get_cwd,
        }

        if action not in actions:
            raise SkillError(f"Unknown action: {action}", self.name)

        return await actions[action](**kwargs)

    async def _get_time(
        self,
        timezone_str: Optional[str] = None,
        **kwargs: Any,
    ) -> Dict[str, str]:
        """Get current time."""
        now = datetime.now(timezone.utc)
        return {
            "time": now.strftime("%H:%M:%S"),
            "timezone": "UTC",
            "iso": now.isoformat(),
        }

    async def _get_date(
        self,
        **kwargs: Any,
    ) -> Dict[str, str]:
        """Get current date."""
        now = datetime.now(timezone.utc)
        return {
            "date": now.strftime("%Y-%m-%d"),
            "day_of_week": now.strftime("%A"),
            "iso": now.isoformat(),
        }

    async def _get_datetime(
        self,
        **kwargs: Any,
    ) -> Dict[str, str]:
        """Get current datetime."""
        now = datetime.now(timezone.utc)
        return {
            "datetime": now.strftime("%Y-%m-%d %H:%M:%S"),
            "timezone": "UTC",
            "iso": now.isoformat(),
            "timestamp": str(int(now.timestamp())),
        }

    async def _get_system_info(
        self,
        **kwargs: Any,
    ) -> Dict[str, str]:
        """Get system information."""
        return {
            "system": platform.system(),
            "release": platform.release(),
            "version": platform.version(),
            "machine": platform.machine(),
            "processor": platform.processor() or "Unknown",
            "python_version": platform.python_version(),
            "node": platform.node(),
        }

    async def _get_env(
        self,
        name: Optional[str] = None,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Get environment variable(s)."""
        if name:
            value = os.environ.get(name)
            return {
                "name": name,
                "value": value,
                "exists": value is not None,
            }

        # Return summary of env vars (not values for security)
        return {
            "count": len(os.environ),
            "names": sorted(list(os.environ.keys())),
        }

    async def _get_user(
        self,
        **kwargs: Any,
    ) -> Dict[str, str]:
        """Get current user information."""
        return {
            "username": os.environ.get("USER") or os.environ.get("USERNAME") or "Unknown",
            "home": os.path.expanduser("~"),
            "uid": str(os.getuid()) if hasattr(os, "getuid") else "N/A",
        }

    async def _get_disk_usage(
        self,
        path: Optional[str] = None,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Get disk usage information."""
        target = path or "/"
        try:
            usage = shutil.disk_usage(target)
            total_gb = usage.total / (1024 ** 3)
            used_gb = usage.used / (1024 ** 3)
            free_gb = usage.free / (1024 ** 3)
            percent_used = (usage.used / usage.total) * 100

            return {
                "path": target,
                "total_gb": round(total_gb, 2),
                "used_gb": round(used_gb, 2),
                "free_gb": round(free_gb, 2),
                "percent_used": round(percent_used, 1),
            }
        except Exception as e:
            raise SkillError(f"Failed to get disk usage: {e}", self.name)

    async def _get_cwd(
        self,
        **kwargs: Any,
    ) -> Dict[str, str]:
        """Get current working directory."""
        return {
            "cwd": os.getcwd(),
        }

    def get_usage(self) -> str:
        """Return usage instructions."""
        return """
System Skill - 系统工具

可用操作:
- time [timezone]: 获取当前时间
- date: 获取当前日期
- datetime [timezone]: 获取当前日期时间
- system_info: 获取系统信息
- env [name]: 获取环境变量
- user: 获取当前用户信息
- disk [path]: 获取磁盘使用信息
- cwd: 获取当前工作目录

示例:
- 获取时间：execute(action="time")
- 获取日期：execute(action="date")
- 系统信息：execute(action="system_info")
- 检查环境变量：execute(action="env", name="HOME")
- 磁盘使用：execute(action="disk", path="/")
"""
