"""
Database module for Omiga Python port.

Uses aiosqlite for async access.  All public functions are async coroutines.
Schema mirrors src/db.ts exactly (6 tables).

Note: SQLite is used with WAL mode for better concurrent read/write performance.
"""
from __future__ import annotations

import asyncio
import json
import logging
from contextlib import asynccontextmanager
from pathlib import Path
from typing import AsyncGenerator, Optional

import aiosqlite

from omiga.config import ASSISTANT_NAME, DATA_DIR, STORE_DIR
from omiga.group_folder import is_valid_group_folder
from omiga.models import (
    ChatInfo,
    MediaAttachment,
    NewMessage,
    ReplyContext,
    RegisteredGroup,
    ScheduledTask,
    TaskRunLog,
    ContainerConfig,
    AdditionalMount,
)

logger = logging.getLogger(__name__)

_DB_PATH: Optional[Path] = None

# ---------------------------------------------------------------------------
# Shared connection pool (single long-lived connection + lock)
# ---------------------------------------------------------------------------

_db_connection: Optional[aiosqlite.Connection] = None
_db_lock: Optional[asyncio.Lock] = None


def _get_db_lock() -> asyncio.Lock:
    global _db_lock
    if _db_lock is None:
        _db_lock = asyncio.Lock()
    return _db_lock


async def close_database() -> None:
    """Close the shared DB connection and reset pool state.

    Must be called at process shutdown and between test cases that use
    different database paths.
    """
    global _db_connection, _db_lock
    if _db_connection is not None:
        try:
            await _db_connection.close()
        except Exception:
            pass
        _db_connection = None
    _db_lock = None


def _db_path() -> Path:
    if _DB_PATH is not None:
        return _DB_PATH
    return STORE_DIR / "messages.db"


def _set_test_db_path(path: Path) -> None:
    """Override DB path for tests (use ':memory:' equivalent via path)."""
    global _DB_PATH
    _DB_PATH = path


_DDL = """
CREATE TABLE IF NOT EXISTS chats (
    jid TEXT PRIMARY KEY,
    name TEXT,
    last_message_time TEXT,
    channel TEXT,
    is_group INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT,
    chat_jid TEXT,
    sender TEXT,
    sender_name TEXT,
    content TEXT,
    timestamp TEXT,
    is_from_me INTEGER,
    is_bot_message INTEGER DEFAULT 0,
    PRIMARY KEY (id, chat_jid),
    FOREIGN KEY (chat_jid) REFERENCES chats(jid)
);
CREATE INDEX IF NOT EXISTS idx_timestamp ON messages(timestamp);

CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id TEXT PRIMARY KEY,
    group_folder TEXT NOT NULL,
    chat_jid TEXT NOT NULL,
    prompt TEXT NOT NULL,
    schedule_type TEXT NOT NULL,
    schedule_value TEXT NOT NULL,
    context_mode TEXT DEFAULT 'isolated',
    next_run TEXT,
    last_run TEXT,
    last_result TEXT,
    status TEXT DEFAULT 'active',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_next_run ON scheduled_tasks(next_run);
CREATE INDEX IF NOT EXISTS idx_status ON scheduled_tasks(status);

CREATE TABLE IF NOT EXISTS task_run_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    run_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    status TEXT NOT NULL,
    result TEXT,
    error TEXT,
    FOREIGN KEY (task_id) REFERENCES scheduled_tasks(id)
);
CREATE INDEX IF NOT EXISTS idx_task_run_logs ON task_run_logs(task_id, run_at);

CREATE TABLE IF NOT EXISTS router_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    group_folder TEXT PRIMARY KEY,
    session_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS registered_groups (
    jid TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    folder TEXT NOT NULL UNIQUE,
    trigger_pattern TEXT NOT NULL,
    added_at TEXT NOT NULL,
    container_config TEXT,
    requires_trigger INTEGER DEFAULT 1
);
"""

_MIGRATIONS = [
    "ALTER TABLE scheduled_tasks ADD COLUMN context_mode TEXT DEFAULT 'isolated'",
    "ALTER TABLE messages ADD COLUMN is_bot_message INTEGER DEFAULT 0",
    "ALTER TABLE chats ADD COLUMN channel TEXT",
    "ALTER TABLE chats ADD COLUMN is_group INTEGER DEFAULT 0",
    "ALTER TABLE messages ADD COLUMN attachments TEXT",
    "ALTER TABLE messages ADD COLUMN reply_to TEXT",
]


async def init_database() -> None:
    """Create schema and run migrations. Must be called once at startup.

    Resets the shared connection pool so the pool always points at the
    current _DB_PATH.  This makes the test fixture (which changes
    _DB_PATH between tests) safe to call multiple times.
    """
    await close_database()          # flush any stale connection first
    path = _db_path()
    path.parent.mkdir(parents=True, exist_ok=True)

    async with aiosqlite.connect(str(path)) as db:
        await db.execute("PRAGMA journal_mode=WAL")
        await db.executescript(_DDL)

        # Run migrations (idempotent — ignore errors for existing columns)
        for migration in _MIGRATIONS:
            try:
                await db.execute(migration)
            except Exception:
                pass  # column already exists

        # Backfill bot messages
        try:
            await db.execute(
                "UPDATE messages SET is_bot_message = 1 WHERE content LIKE ?",
                (f"{ASSISTANT_NAME}:%",),
            )
        except Exception:
            pass

        # Backfill channel / is_group from JID patterns
        try:
            await db.executescript("""
                UPDATE chats SET channel = 'whatsapp', is_group = 1 WHERE jid LIKE '%@g.us';
                UPDATE chats SET channel = 'whatsapp', is_group = 0 WHERE jid LIKE '%@s.whatsapp.net';
                UPDATE chats SET channel = 'discord',  is_group = 1 WHERE jid LIKE 'dc:%';
                UPDATE chats SET channel = 'telegram', is_group = 1 WHERE jid LIKE 'tg:%';
            """)
        except Exception:
            pass

        await db.commit()

    logger.info("Database initialized at %s", path)


@asynccontextmanager
async def _connect() -> AsyncGenerator[aiosqlite.Connection, None]:
    """Yield the shared DB connection, opening it lazily on first use.

    A module-level asyncio.Lock serialises access so concurrent coroutines
    do not race on the same sqlite3 connection handle.
    """
    global _db_connection
    lock = _get_db_lock()
    async with lock:
        if _db_connection is None:
            path = _db_path()
            conn = await aiosqlite.connect(str(path))
            conn.row_factory = aiosqlite.Row
            await conn.execute("PRAGMA journal_mode=WAL")
            _db_connection = conn
        yield _db_connection


# ---------------------------------------------------------------------------
# Chat metadata
# ---------------------------------------------------------------------------

async def store_chat_metadata(
    chat_jid: str,
    timestamp: str,
    name: Optional[str] = None,
    channel: Optional[str] = None,
    is_group: Optional[bool] = None,
) -> None:
    is_group_int = None if is_group is None else (1 if is_group else 0)
    async with _connect() as db:
        if name:
            await db.execute(
                """
                INSERT INTO chats (jid, name, last_message_time, channel, is_group)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(jid) DO UPDATE SET
                    name = excluded.name,
                    last_message_time = MAX(last_message_time, excluded.last_message_time),
                    channel = COALESCE(excluded.channel, channel),
                    is_group = COALESCE(excluded.is_group, is_group)
                """,
                (chat_jid, name, timestamp, channel, is_group_int),
            )
        else:
            await db.execute(
                """
                INSERT INTO chats (jid, name, last_message_time, channel, is_group)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(jid) DO UPDATE SET
                    last_message_time = MAX(last_message_time, excluded.last_message_time),
                    channel = COALESCE(excluded.channel, channel),
                    is_group = COALESCE(excluded.is_group, is_group)
                """,
                (chat_jid, chat_jid, timestamp, channel, is_group_int),
            )
        await db.commit()


async def update_chat_name(chat_jid: str, name: str) -> None:
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc).isoformat()
    async with _connect() as db:
        await db.execute(
            """
            INSERT INTO chats (jid, name, last_message_time) VALUES (?, ?, ?)
            ON CONFLICT(jid) DO UPDATE SET name = excluded.name
            """,
            (chat_jid, name, now),
        )
        await db.commit()


async def get_all_chats() -> list[ChatInfo]:
    async with _connect() as db:
        async with db.execute(
            "SELECT jid, name, last_message_time, channel, is_group FROM chats ORDER BY last_message_time DESC"
        ) as cursor:
            rows = await cursor.fetchall()
    return [
        ChatInfo(
            jid=r["jid"],
            name=r["name"] or "",
            last_message_time=r["last_message_time"] or "",
            channel=r["channel"],
            is_group=bool(r["is_group"]),
        )
        for r in rows
    ]


async def get_last_group_sync() -> Optional[str]:
    async with _connect() as db:
        async with db.execute(
            "SELECT last_message_time FROM chats WHERE jid = '__group_sync__'"
        ) as cursor:
            row = await cursor.fetchone()
    return row["last_message_time"] if row else None


async def set_last_group_sync() -> None:
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc).isoformat()
    async with _connect() as db:
        await db.execute(
            "INSERT OR REPLACE INTO chats (jid, name, last_message_time) VALUES ('__group_sync__', '__group_sync__', ?)",
            (now,),
        )
        await db.commit()


# ---------------------------------------------------------------------------
# Messages
# ---------------------------------------------------------------------------

def _serialize_reply_to(reply_to) -> Optional[str]:
    """Serialize a ReplyContext to JSON, or None if absent."""
    if reply_to is None:
        return None
    return json.dumps({
        "message_id": reply_to.message_id,
        "sender_name": reply_to.sender_name,
        "content": reply_to.content,
    })


def _parse_reply_to(raw: Optional[str]) -> Optional[ReplyContext]:
    """Deserialize a JSON string back into a ReplyContext, or None."""
    if not raw:
        return None
    try:
        d = json.loads(raw)
        return ReplyContext(
            message_id=d["message_id"],
            sender_name=d["sender_name"],
            content=d["content"],
        )
    except Exception:
        return None


def _serialize_attachments(attachments: list) -> Optional[str]:
    """Serialize a list of MediaAttachment to a JSON string, or None if empty."""
    if not attachments:
        return None
    return json.dumps([
        {
            "type": a.type,
            "filename": a.filename,
            "mime_type": a.mime_type,
            "local_path": a.local_path,
            "url": a.url,
        }
        for a in attachments
    ])


def _parse_attachments(raw: Optional[str]) -> list[MediaAttachment]:
    """Deserialize a JSON string back into a list of MediaAttachment objects."""
    if not raw:
        return []
    try:
        items = json.loads(raw)
        return [
            MediaAttachment(
                type=a["type"],
                filename=a["filename"],
                mime_type=a["mime_type"],
                local_path=a["local_path"],
                url=a.get("url", ""),
            )
            for a in items
        ]
    except Exception:
        return []


async def store_message(msg: NewMessage) -> None:
    async with _connect() as db:
        await db.execute(
            """
            INSERT OR REPLACE INTO messages
            (id, chat_jid, sender, sender_name, content, timestamp,
             is_from_me, is_bot_message, attachments, reply_to)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                msg.id,
                msg.chat_jid,
                msg.sender,
                msg.sender_name,
                msg.content,
                msg.timestamp,
                1 if msg.is_from_me else 0,
                1 if msg.is_bot_message else 0,
                _serialize_attachments(msg.attachments),
                _serialize_reply_to(msg.reply_to),
            ),
        )
        await db.commit()


async def get_new_messages(
    jids: list[str],
    last_timestamp: str,
    bot_prefix: str,
) -> tuple[list[NewMessage], str]:
    """Return (messages, new_timestamp) for all jids with timestamp > last_timestamp."""
    if not jids:
        return [], last_timestamp

    placeholders = ",".join("?" * len(jids))
    sql = f"""
        SELECT id, chat_jid, sender, sender_name, content, timestamp, attachments, reply_to
        FROM messages
        WHERE timestamp > ? AND chat_jid IN ({placeholders})
          AND is_bot_message = 0 AND content NOT LIKE ?
          AND content != '' AND content IS NOT NULL
        ORDER BY timestamp
    """
    async with _connect() as db:
        async with db.execute(sql, (last_timestamp, *jids, f"{bot_prefix}:%")) as cursor:
            rows = await cursor.fetchall()

    messages = [
        NewMessage(
            id=r["id"],
            chat_jid=r["chat_jid"],
            sender=r["sender"],
            sender_name=r["sender_name"],
            content=r["content"],
            timestamp=r["timestamp"],
            attachments=_parse_attachments(r["attachments"]),
            reply_to=_parse_reply_to(r["reply_to"]),
        )
        for r in rows
    ]

    new_timestamp = last_timestamp
    for m in messages:
        if m.timestamp > new_timestamp:
            new_timestamp = m.timestamp

    return messages, new_timestamp


async def get_messages_since(
    chat_jid: str,
    since_timestamp: str,
    bot_prefix: str,
) -> list[NewMessage]:
    sql = """
        SELECT id, chat_jid, sender, sender_name, content, timestamp, attachments, reply_to
        FROM messages
        WHERE chat_jid = ? AND timestamp > ?
          AND is_bot_message = 0 AND content NOT LIKE ?
          AND content != '' AND content IS NOT NULL
        ORDER BY timestamp
    """
    async with _connect() as db:
        async with db.execute(sql, (chat_jid, since_timestamp, f"{bot_prefix}:%")) as cursor:
            rows = await cursor.fetchall()

    return [
        NewMessage(
            id=r["id"],
            chat_jid=r["chat_jid"],
            sender=r["sender"],
            sender_name=r["sender_name"],
            content=r["content"],
            timestamp=r["timestamp"],
            attachments=_parse_attachments(r["attachments"]),
            reply_to=_parse_reply_to(r["reply_to"]),
        )
        for r in rows
    ]


# ---------------------------------------------------------------------------
# Scheduled tasks
# ---------------------------------------------------------------------------

def _row_to_task(r: aiosqlite.Row) -> ScheduledTask:
    return ScheduledTask(
        id=r["id"],
        group_folder=r["group_folder"],
        chat_jid=r["chat_jid"],
        prompt=r["prompt"],
        schedule_type=r["schedule_type"],
        schedule_value=r["schedule_value"],
        context_mode=r["context_mode"] or "isolated",
        next_run=r["next_run"],
        last_run=r["last_run"],
        last_result=r["last_result"],
        status=r["status"],
        created_at=r["created_at"],
    )


async def create_task(task: ScheduledTask) -> None:
    async with _connect() as db:
        await db.execute(
            """
            INSERT INTO scheduled_tasks
            (id, group_folder, chat_jid, prompt, schedule_type, schedule_value,
             context_mode, next_run, status, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                task.id,
                task.group_folder,
                task.chat_jid,
                task.prompt,
                task.schedule_type,
                task.schedule_value,
                task.context_mode or "isolated",
                task.next_run,
                task.status,
                task.created_at,
            ),
        )
        await db.commit()


async def get_task_by_id(task_id: str) -> Optional[ScheduledTask]:
    async with _connect() as db:
        async with db.execute(
            "SELECT * FROM scheduled_tasks WHERE id = ?", (task_id,)
        ) as cursor:
            row = await cursor.fetchone()
    return _row_to_task(row) if row else None


async def get_tasks_for_group(group_folder: str) -> list[ScheduledTask]:
    async with _connect() as db:
        async with db.execute(
            "SELECT * FROM scheduled_tasks WHERE group_folder = ? ORDER BY created_at DESC",
            (group_folder,),
        ) as cursor:
            rows = await cursor.fetchall()
    return [_row_to_task(r) for r in rows]


async def get_all_tasks() -> list[ScheduledTask]:
    async with _connect() as db:
        async with db.execute(
            "SELECT * FROM scheduled_tasks ORDER BY created_at DESC"
        ) as cursor:
            rows = await cursor.fetchall()
    return [_row_to_task(r) for r in rows]


async def update_task(
    task_id: str,
    *,
    prompt: Optional[str] = None,
    schedule_type: Optional[str] = None,
    schedule_value: Optional[str] = None,
    next_run: Optional[str] = None,
    status: Optional[str] = None,
) -> None:
    fields, values = [], []
    if prompt is not None:
        fields.append("prompt = ?"); values.append(prompt)
    if schedule_type is not None:
        fields.append("schedule_type = ?"); values.append(schedule_type)
    if schedule_value is not None:
        fields.append("schedule_value = ?"); values.append(schedule_value)
    if next_run is not None:
        fields.append("next_run = ?"); values.append(next_run)
    if status is not None:
        fields.append("status = ?"); values.append(status)
    if not fields:
        return
    values.append(task_id)
    async with _connect() as db:
        await db.execute(
            f"UPDATE scheduled_tasks SET {', '.join(fields)} WHERE id = ?", values
        )
        await db.commit()


async def delete_task(task_id: str) -> None:
    async with _connect() as db:
        await db.execute("DELETE FROM task_run_logs WHERE task_id = ?", (task_id,))
        await db.execute("DELETE FROM scheduled_tasks WHERE id = ?", (task_id,))
        await db.commit()


async def get_due_tasks() -> list[ScheduledTask]:
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc).isoformat()
    async with _connect() as db:
        async with db.execute(
            """
            SELECT * FROM scheduled_tasks
            WHERE status = 'active' AND next_run IS NOT NULL AND next_run <= ?
            ORDER BY next_run
            """,
            (now,),
        ) as cursor:
            rows = await cursor.fetchall()
    return [_row_to_task(r) for r in rows]


async def update_task_after_run(
    task_id: str, next_run: Optional[str], last_result: str
) -> None:
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc).isoformat()
    async with _connect() as db:
        await db.execute(
            """
            UPDATE scheduled_tasks
            SET next_run = ?,
                last_run = ?,
                last_result = ?,
                status = CASE WHEN ? IS NULL THEN 'completed' ELSE status END
            WHERE id = ?
            """,
            (next_run, now, last_result, next_run, task_id),
        )
        await db.commit()


async def log_task_run(log: TaskRunLog) -> None:
    async with _connect() as db:
        await db.execute(
            """
            INSERT INTO task_run_logs (task_id, run_at, duration_ms, status, result, error)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (log.task_id, log.run_at, log.duration_ms, log.status, log.result, log.error),
        )
        await db.commit()


# ---------------------------------------------------------------------------
# Router state
# ---------------------------------------------------------------------------

async def get_router_state(key: str) -> Optional[str]:
    async with _connect() as db:
        async with db.execute(
            "SELECT value FROM router_state WHERE key = ?", (key,)
        ) as cursor:
            row = await cursor.fetchone()
    return row["value"] if row else None


async def set_router_state(key: str, value: str) -> None:
    async with _connect() as db:
        await db.execute(
            "INSERT OR REPLACE INTO router_state (key, value) VALUES (?, ?)",
            (key, value),
        )
        await db.commit()


# ---------------------------------------------------------------------------
# Sessions
# ---------------------------------------------------------------------------

async def get_session(group_folder: str) -> Optional[str]:
    async with _connect() as db:
        async with db.execute(
            "SELECT session_id FROM sessions WHERE group_folder = ?", (group_folder,)
        ) as cursor:
            row = await cursor.fetchone()
    return row["session_id"] if row else None


async def set_session(group_folder: str, session_id: str) -> None:
    async with _connect() as db:
        await db.execute(
            "INSERT OR REPLACE INTO sessions (group_folder, session_id) VALUES (?, ?)",
            (group_folder, session_id),
        )
        await db.commit()


async def get_all_sessions() -> dict[str, str]:
    async with _connect() as db:
        async with db.execute("SELECT group_folder, session_id FROM sessions") as cursor:
            rows = await cursor.fetchall()
    return {r["group_folder"]: r["session_id"] for r in rows}


# ---------------------------------------------------------------------------
# Registered groups
# ---------------------------------------------------------------------------

def _parse_registered_group(row: aiosqlite.Row) -> Optional[RegisteredGroup]:
    if not is_valid_group_folder(row["folder"]):
        logger.warning(
            "Skipping registered group with invalid folder jid=%s folder=%s",
            row["jid"], row["folder"],
        )
        return None

    cc_data = row["container_config"]
    container_config: Optional[ContainerConfig] = None
    if cc_data:
        try:
            raw = json.loads(cc_data)
            mounts = None
            if raw.get("additionalMounts"):
                mounts = [
                    AdditionalMount(
                        host_path=m["hostPath"],
                        container_path=m.get("containerPath"),
                        readonly=m.get("readonly", True),
                    )
                    for m in raw["additionalMounts"]
                ]
            container_config = ContainerConfig(
                additional_mounts=mounts,
                timeout=raw.get("timeout"),
            )
        except Exception:
            pass

    rt = row["requires_trigger"]
    requires_trigger: Optional[bool] = None if rt is None else bool(rt)

    return RegisteredGroup(
        name=row["name"],
        folder=row["folder"],
        trigger=row["trigger_pattern"],
        added_at=row["added_at"],
        container_config=container_config,
        requires_trigger=requires_trigger,
    )


async def get_registered_group(jid: str) -> Optional[RegisteredGroup]:
    async with _connect() as db:
        async with db.execute(
            "SELECT * FROM registered_groups WHERE jid = ?", (jid,)
        ) as cursor:
            row = await cursor.fetchone()
    if not row:
        return None
    return _parse_registered_group(row)


async def set_registered_group(jid: str, group: RegisteredGroup) -> None:
    if not is_valid_group_folder(group.folder):
        raise ValueError(f'Invalid group folder "{group.folder}" for JID {jid}')

    cc_json = None
    if group.container_config:
        raw: dict = {}
        if group.container_config.timeout is not None:
            raw["timeout"] = group.container_config.timeout
        if group.container_config.additional_mounts:
            raw["additionalMounts"] = [
                {
                    "hostPath": m.host_path,
                    **({"containerPath": m.container_path} if m.container_path else {}),
                    "readonly": m.readonly,
                }
                for m in group.container_config.additional_mounts
            ]
        cc_json = json.dumps(raw)

    rt = None if group.requires_trigger is None else (1 if group.requires_trigger else 0)

    async with _connect() as db:
        await db.execute(
            """
            INSERT OR REPLACE INTO registered_groups
            (jid, name, folder, trigger_pattern, added_at, container_config, requires_trigger)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (jid, group.name, group.folder, group.trigger, group.added_at, cc_json, rt),
        )
        await db.commit()


async def get_all_registered_groups() -> dict[str, RegisteredGroup]:
    async with _connect() as db:
        async with db.execute("SELECT * FROM registered_groups") as cursor:
            rows = await cursor.fetchall()

    result: dict[str, RegisteredGroup] = {}
    for row in rows:
        group = _parse_registered_group(row)
        if group:
            result[row["jid"]] = group
    return result


async def delete_registered_group(jid: str) -> None:
    """Remove a group from the registered_groups table.

    The group's folder on disk is intentionally left intact so that history
    and CLAUDE.md are preserved for potential re-registration.
    """
    async with _connect() as db:
        await db.execute("DELETE FROM registered_groups WHERE jid = ?", (jid,))
        await db.commit()
