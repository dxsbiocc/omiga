"""Cron skill for managing scheduled tasks."""
from __future__ import annotations

import logging
from typing import Any, Optional

from omiga.skills.base import Skill, SkillContext, SkillMetadata, SkillError

logger = logging.getLogger("omiga.skills.cron")


class CronSkill(Skill):
    """Skill for managing scheduled tasks."""

    metadata = SkillMetadata(
        name="cron",
        description="管理定时任务 - 创建、查询、暂停、恢复、删除任务",
        emoji="⏰",
        tags=["schedule", "task", "automation"],
    )

    def __init__(self, context: SkillContext):
        super().__init__(context)
        self._tasks_cache: Optional[list[dict]] = None

    async def execute(self, action: str, **kwargs: Any) -> Any:  # type: ignore[override]
        """Execute the cron skill.

        Args:
            action: The action to perform (list, get, create, delete, pause, resume)
            **kwargs: Action-specific arguments

        Returns:
            Result of the action
        """
        actions = {
            "list": self._list_tasks,
            "get": self._get_task,
            "create": self._create_task,
            "delete": self._delete_task,
            "pause": self._pause_task,
            "resume": self._resume_task,
        }

        if action not in actions:
            raise SkillError(f"Unknown action: {action}", self.name)

        return await actions[action](**kwargs)

    async def _list_tasks(self, **kwargs: Any) -> list[dict]:
        """List all scheduled tasks."""
        # Import here to avoid circular imports
        from omiga.database import get_all_tasks

        try:
            tasks = await get_all_tasks()
            self._tasks_cache = [
                {
                    "id": t.id,
                    "group_folder": t.group_folder,
                    "prompt": t.prompt,
                    "schedule_type": t.schedule_type,
                    "schedule_value": t.schedule_value,
                    "status": t.status,
                    "next_run": t.next_run,
                }
                for t in tasks
            ]
            return self._tasks_cache
        except Exception as e:
            raise SkillError(f"Failed to list tasks: {e}", self.name)

    async def _get_task(self, task_id: str, **kwargs: Any) -> Optional[dict]:
        """Get task details by ID."""
        from omiga.database import get_task_by_id

        try:
            task = await get_task_by_id(task_id)
            if not task:
                raise SkillError(f"Task '{task_id}' not found", self.name)
            return {
                "id": task.id,
                "group_folder": task.group_folder,
                "prompt": task.prompt,
                "schedule_type": task.schedule_type,
                "schedule_value": task.schedule_value,
                "status": task.status,
                "next_run": task.next_run,
                "last_run": task.last_run,
                "last_result": task.last_result,
            }
        except SkillError:
            raise
        except Exception as e:
            raise SkillError(f"Failed to get task: {e}", self.name)

    async def _create_task(
        self,
        prompt: str,
        schedule_type: str = "cron",
        schedule_value: str = "* * * * *",
        group_folder: Optional[str] = None,
        chat_jid: Optional[str] = None,
        **kwargs: Any,
    ) -> dict:
        """Create a new scheduled task."""
        import uuid
        from datetime import datetime, timezone

        from omiga.database import create_task
        from omiga.models import ScheduledTask

        try:
            # Validate schedule_type
            valid_types = ["cron", "interval", "date"]
            if schedule_type not in valid_types:
                raise SkillError(
                    f"Invalid schedule_type. Must be one of: {valid_types}", self.name
                )

            task_id = str(uuid.uuid4())
            task = ScheduledTask(
                id=task_id,
                group_folder=group_folder or "main",
                chat_jid=chat_jid or "",
                prompt=prompt,
                schedule_type=schedule_type,
                schedule_value=schedule_value,
                status="active",
                created_at=datetime.now(timezone.utc).isoformat(),
                next_run=None,  # Will be calculated by scheduler
            )

            await create_task(task)
            self._tasks_cache = None  # Invalidate cache

            return {
                "id": task_id,
                "status": "created",
                "message": f"Task created: {prompt[:50]}...",
            }
        except SkillError:
            raise
        except Exception as e:
            raise SkillError(f"Failed to create task: {e}", self.name)

    async def _delete_task(self, task_id: str, **kwargs: Any) -> dict:
        """Delete a task by ID."""
        from omiga.database import delete_task, get_task_by_id

        try:
            task = await get_task_by_id(task_id)
            if not task:
                raise SkillError(f"Task '{task_id}' not found", self.name)

            await delete_task(task_id)
            self._tasks_cache = None  # Invalidate cache

            return {"id": task_id, "status": "deleted"}
        except SkillError:
            raise
        except Exception as e:
            raise SkillError(f"Failed to delete task: {e}", self.name)

    async def _pause_task(self, task_id: str, **kwargs: Any) -> dict:
        """Pause a task."""
        from omiga.database import get_task_by_id, update_task

        try:
            task = await get_task_by_id(task_id)
            if not task:
                raise SkillError(f"Task '{task_id}' not found", self.name)

            await update_task(task_id, status="paused")
            return {"id": task_id, "status": "paused"}
        except SkillError:
            raise
        except Exception as e:
            raise SkillError(f"Failed to pause task: {e}", self.name)

    async def _resume_task(self, task_id: str, **kwargs: Any) -> dict:
        """Resume a paused task."""
        from omiga.database import get_task_by_id, update_task

        try:
            task = await get_task_by_id(task_id)
            if not task:
                raise SkillError(f"Task '{task_id}' not found", self.name)

            await update_task(task_id, status="active")
            return {"id": task_id, "status": "resumed"}
        except SkillError:
            raise
        except Exception as e:
            raise SkillError(f"Failed to resume task: {e}", self.name)

    def get_usage(self) -> str:
        """Return usage instructions."""
        return """
Cron Skill - 定时任务管理

可用操作:
- list: 列出所有任务
- get <task_id>: 查看任务详情
- create <prompt> [schedule_type] [schedule_value]: 创建任务
- delete <task_id>: 删除任务
- pause <task_id>: 暂停任务
- resume <task_id>: 恢复任务

示例:
- 列出所有任务：execute(action="list")
- 创建每天 9 点任务：execute(action="create", prompt="发送早安消息", schedule_type="cron", schedule_value="0 9 * * *")
"""
