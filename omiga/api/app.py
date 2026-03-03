"""
Lightweight HTTP REST API for Omiga.

Runs as a FastAPI app inside the main asyncio event loop (via uvicorn with
`loop="none"` so it shares the running loop).

Endpoints
---------
GET  /                           redirect to Web Console
GET  /console                    Web Console UI
GET  /static/*                   Static files (JS, CSS)
GET  /api                        health + uptime
GET  /api/status                 channel status
GET  /api/groups                 list registered groups
POST /api/groups                 register a group
DELETE /api/groups/{jid}         unregister a group
GET  /api/chats                  list known chats
GET  /api/tasks                  list scheduled tasks
POST /api/tasks/{id}/run         trigger a task immediately
GET  /api/workspace/backup       download workspace as zip

Configuration
-------------
HTTP_API_PORT   — port to bind (default 7891, 0 = disabled)
HTTP_API_TOKEN  — optional Bearer token for auth (no auth if unset)
"""
from __future__ import annotations

import asyncio
import io
import logging
import time
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Optional

logger = logging.getLogger(__name__)

_START_TIME = time.monotonic()

# ---------------------------------------------------------------------------
# Pydantic request models (module-level — required for FastAPI type resolution)
# ---------------------------------------------------------------------------
try:
    from pydantic import BaseModel as _BaseModel

    class GroupIn(_BaseModel):
        jid: str
        name: str
        requires_trigger: bool = True

except ImportError:
    GroupIn = None  # type: ignore[assignment,misc]


def _uptime_str() -> str:
    elapsed = int(time.monotonic() - _START_TIME)
    h, m, s = elapsed // 3600, (elapsed % 3600) // 60, elapsed % 60
    return f"{h:02d}:{m:02d}:{s:02d}"


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


# ---------------------------------------------------------------------------
# App factory
# ---------------------------------------------------------------------------

def create_app(
    *,
    channels_fn: Callable,
    registered_groups_fn: Callable,
    all_chats_fn: Callable,
    get_tasks_fn: Callable,
    run_task_fn: Callable,
    register_group_fn: Callable,
    unregister_group_fn: Callable,
    groups_dir: Path,
    api_token: str = "",
    serve_static: bool = True,
) -> Any:
    """Build and return the FastAPI app.

    All state is accessed through callables so the API always sees the latest
    in-memory state without coupling to global variables.

    Args:
        serve_static: If True, serve the Web Console static files
    """
    try:
        from fastapi import Depends, FastAPI, HTTPException, status
        from fastapi.responses import StreamingResponse, FileResponse, HTMLResponse, RedirectResponse
        from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
        from fastapi.staticfiles import StaticFiles
    except ImportError as exc:
        raise RuntimeError("fastapi is required: pip install fastapi uvicorn") from exc

    app = FastAPI(
        title="Omiga API",
        description="Remote management API for Omiga",
        version="1.0.0",
    )

    # ------------------------------------------------------------------
    # Optional Bearer-token auth — applied via route decorator, not signature
    # ------------------------------------------------------------------
    _bearer = HTTPBearer(auto_error=False)

    def _check_auth(credentials: Optional[HTTPAuthorizationCredentials] = Depends(_bearer)):
        if not api_token:
            return  # auth disabled
        if credentials is None or credentials.credentials != api_token:
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Invalid or missing Bearer token",
                headers={"WWW-Authenticate": "Bearer"},
            )

    # Applied at route level so it doesn't conflict with body parsing
    _auth_dep = [Depends(_check_auth)]

    # ------------------------------------------------------------------
    # Static files and Web Console
    # ------------------------------------------------------------------
    if serve_static:
        # Find the static files directory
        import omiga.web
        static_dir = Path(omiga.web.__file__).parent / "static"

        if static_dir.exists():
            # Mount static files at /static
            app.mount("/static", StaticFiles(directory=str(static_dir)), name="static")

            # Serve index.html at /console
            @app.get("/console", include_in_schema=False)
            async def serve_console():
                index_path = static_dir / "index.html"
                if index_path.exists():
                    return HTMLResponse(index_path.read_text(encoding="utf-8"))
                raise HTTPException(404, "Console not found")

            # Redirect / to /console for the web UI
            @app.get("/", include_in_schema=False)
            async def root_redirect():
                return RedirectResponse(url="/console")
        else:
            logger.warning(f"Static files directory not found: {static_dir}")

    # ------------------------------------------------------------------
    # API Routes
    # ------------------------------------------------------------------

    @app.get("/api", include_in_schema=False)
    async def root():
        return {
            "service": "omiga",
            "uptime": _uptime_str(),
            "time": _now_iso(),
        }

    @app.get("/api/status", dependencies=_auth_dep)
    async def get_status():
        channels = channels_fn()
        return {
            "time": _now_iso(),
            "uptime": _uptime_str(),
            "registered_groups": len(registered_groups_fn()),
            "channels": [
                {"name": ch.name, "connected": ch.is_connected()}
                for ch in channels
            ],
        }

    @app.get("/api/groups", dependencies=_auth_dep)
    async def list_groups():
        groups = registered_groups_fn()
        return [
            {
                "jid": jid,
                "name": g.name,
                "folder": g.folder,
                "requires_trigger": g.requires_trigger,
                "added_at": g.added_at,
            }
            for jid, g in groups.items()
        ]

    @app.post("/api/groups", status_code=201, dependencies=_auth_dep)
    async def register_group(body: GroupIn):
        groups = registered_groups_fn()
        if body.jid in groups:
            raise HTTPException(409, f"Already registered: {body.jid}")
        try:
            await register_group_fn(body.jid, body.name, body.requires_trigger)
        except Exception as exc:
            raise HTTPException(400, str(exc)) from exc
        return {"jid": body.jid, "name": body.name, "registered": True}

    @app.delete("/api/groups/{jid}", status_code=200, dependencies=_auth_dep)
    async def unregister_group(jid: str):
        groups = registered_groups_fn()
        # URL-decode colon-like separators encoded by some HTTP clients
        jid = jid.replace("%3A", ":").replace("%3a", ":")
        if jid not in groups:
            raise HTTPException(404, f"Not registered: {jid}")
        await unregister_group_fn(jid)
        return {"jid": jid, "unregistered": True}

    @app.get("/api/chats", dependencies=_auth_dep)
    async def list_chats():
        chats = await all_chats_fn()
        registered = set(registered_groups_fn().keys())
        return [
            {
                "jid": c.jid,
                "name": c.name,
                "last_message_time": c.last_message_time,
                "channel": c.channel,
                "is_group": c.is_group,
                "is_registered": c.jid in registered,
            }
            for c in chats
        ]

    @app.get("/api/tasks", dependencies=_auth_dep)
    async def list_tasks():
        tasks = await get_tasks_fn()
        return [
            {
                "id": t.id,
                "group_folder": t.group_folder,
                "chat_jid": t.chat_jid,
                "schedule_type": t.schedule_type,
                "schedule_value": t.schedule_value,
                "status": t.status,
                "next_run": t.next_run,
                "last_run": t.last_run,
                "last_result": t.last_result,
            }
            for t in tasks
        ]

    @app.post("/api/tasks/{task_id}/run", status_code=202, dependencies=_auth_dep)
    async def run_task(task_id: str):
        try:
            await run_task_fn(task_id)
        except KeyError:
            raise HTTPException(404, f"Task not found: {task_id}")
        except Exception as exc:
            raise HTTPException(400, str(exc)) from exc
        return {"task_id": task_id, "queued": True}

    @app.get("/api/workspace/backup", dependencies=_auth_dep)
    async def workspace_backup():
        """Stream all group folders as a zip archive."""
        if not groups_dir.exists():
            raise HTTPException(404, "groups/ directory not found")

        def _build_zip() -> bytes:
            buf = io.BytesIO()
            with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
                for path in sorted(groups_dir.rglob("*")):
                    if path.is_file():
                        arcname = path.relative_to(groups_dir.parent)
                        try:
                            zf.write(path, arcname)
                        except (PermissionError, OSError) as exc:
                            logger.warning("Backup: skipping %s — %s", path, exc)
            buf.seek(0)
            return buf.read()

        loop = asyncio.get_event_loop()
        data = await loop.run_in_executor(None, _build_zip)
        ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S")
        filename = f"omiga-backup-{ts}.zip"

        return StreamingResponse(
            io.BytesIO(data),
            media_type="application/zip",
            headers={"Content-Disposition": f'attachment; filename="{filename}"'},
        )

    return app


# ---------------------------------------------------------------------------
# Server lifecycle
# ---------------------------------------------------------------------------

async def start_api_server(
    app: Any,
    port: int,
    host: str = "127.0.0.1",
) -> None:
    """Start uvicorn inside the running asyncio loop (non-blocking)."""
    try:
        import uvicorn
    except ImportError:
        logger.error("uvicorn not installed — HTTP API disabled. pip install uvicorn")
        return

    config = uvicorn.Config(
        app=app,
        host=host,
        port=port,
        loop="none",          # reuse the existing asyncio loop
        log_config=None,      # use Omiga's own logging
        access_log=False,
    )
    server = uvicorn.Server(config)
    # Run server in background task
    asyncio.create_task(server.serve(), name="http-api-server")
    logger.info("HTTP API (with Web Console) listening on http://%s:%d", host, port)
