"""
Data models for Omiga Python port.
All types are plain dataclasses — no external validation library required.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Literal, Optional


@dataclass
class AdditionalMount:
    host_path: str
    container_path: Optional[str] = None
    readonly: bool = True


@dataclass
class AllowedRoot:
    path: str
    allow_read_write: bool = False
    description: Optional[str] = None


@dataclass
class ReplyContext:
    """The message being replied to, extracted from platform reply metadata."""
    message_id: str    # platform message ID of the original
    sender_name: str   # display name of the original sender
    content: str       # preview of the original content (≤200 chars)


@dataclass
class MediaAttachment:
    """A media file attached to a message.

    ``local_path`` is relative to the group workspace root on the host
    (e.g. ``attachments/photo.jpg``), which maps to
    ``/workspace/attachments/photo.jpg`` inside the agent container.
    For unregistered groups it is an absolute host path.
    """

    type: str       # "image", "audio", "video", "document", "voice"
    filename: str   # filename on disk (unique per message)
    mime_type: str  # e.g. "image/jpeg"
    local_path: str # path used in prompts (relative to workspace or absolute)
    url: str = ""   # original URL or platform file_id (for reference)


@dataclass
class MountAllowlist:
    allowed_roots: list[AllowedRoot]
    blocked_patterns: list[str]
    non_main_read_only: bool


@dataclass
class ContainerConfig:
    additional_mounts: Optional[list[AdditionalMount]] = None
    timeout: Optional[int] = None  # ms


@dataclass
class RegisteredGroup:
    name: str
    folder: str
    trigger: str
    added_at: str
    container_config: Optional[ContainerConfig] = None
    requires_trigger: Optional[bool] = None  # None → default (True for groups)


@dataclass
class NewMessage:
    id: str
    chat_jid: str
    sender: str
    sender_name: str
    content: str
    timestamp: str
    is_from_me: bool = False
    is_bot_message: bool = False
    attachments: list[Any] = field(default_factory=list)  # list[MediaAttachment]
    reply_to: Optional[Any] = None  # ReplyContext | None


@dataclass
class ScheduledTask:
    id: str
    group_folder: str
    chat_jid: str
    prompt: str
    schedule_type: Literal["cron", "interval", "once"]
    schedule_value: str
    context_mode: Literal["group", "isolated"]
    next_run: Optional[str]
    last_run: Optional[str]
    last_result: Optional[str]
    status: Literal["active", "paused", "completed"]
    created_at: str


@dataclass
class TaskRunLog:
    task_id: str
    run_at: str
    duration_ms: int
    status: Literal["success", "error"]
    result: Optional[str]
    error: Optional[str]


@dataclass
class ChatInfo:
    jid: str
    name: str
    last_message_time: str
    channel: Optional[str]
    is_group: bool


@dataclass
class ContainerInput:
    prompt: str
    group_folder: str
    chat_jid: str
    is_main: bool
    session_id: Optional[str] = None
    is_scheduled_task: bool = False
    assistant_name: Optional[str] = None
    secrets: Optional[dict[str, str]] = None


@dataclass
class ContainerOutput:
    status: Literal["success", "error"]
    result: Optional[str]
    new_session_id: Optional[str] = None
    error: Optional[str] = None
    # 新增：详细执行日志和工具调用记录（用于 SOP 生成）
    execution_log: Optional[str] = None
    tool_calls: Optional[list[dict]] = None


@dataclass
class VolumeMount:
    host_path: str
    container_path: str
    readonly: bool


@dataclass
class AvailableGroup:
    jid: str
    name: str
    last_activity: str
    is_registered: bool
