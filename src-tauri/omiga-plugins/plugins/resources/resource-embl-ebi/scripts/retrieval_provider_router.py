#!/usr/bin/env python3
"""Plugin-local router for an aggregated Omiga retrieval resource plugin."""

from __future__ import annotations

from pathlib import Path
import importlib.util
import json
import sys
from typing import Any, Dict, List

PROTOCOL_VERSION = 1
RUNNER_FILES = ['embl_dataset_sources.py', 'embl_knowledge_sources.py']
SCRIPT_DIR = Path(__file__).resolve().parent


def write(message: Dict[str, Any]) -> None:
    print(json.dumps(message, separators=(",", ":"), ensure_ascii=False), flush=True)


def error(message_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": message_id, "type": "error", "error": {"code": code, "message": message}}


def load_runner(filename: str):
    path = SCRIPT_DIR / filename
    if not path.is_file():
        raise FileNotFoundError(f"plugin-local resource runner {filename} not found under {SCRIPT_DIR}")
    module_name = f"omiga_{path.stem}_{abs(hash(str(path)))}"
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load resource runner {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNERS = [load_runner(filename) for filename in RUNNER_FILES]


def configured_sources() -> List[Dict[str, Any]]:
    sources: List[Dict[str, Any]] = []
    seen: set[tuple[str, str]] = set()
    for runner in RUNNERS:
        for source in runner.configured_sources():
            key = (str(source.get("category", "")), str(source.get("id", "")))
            if key in seen:
                continue
            seen.add(key)
            sources.append(source)
    return sources


def runner_by_source() -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    for runner in RUNNERS:
        for source in runner.configured_sources():
            source_id = str(source.get("id", "")).strip().lower().replace("-", "_")
            if source_id:
                out[source_id] = runner
    return out


def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    message_id = str(message.get("id", "execute"))
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    source = str(request.get("source", "")).strip().lower().replace("-", "_")
    runner = runner_by_source().get(source)
    if runner is None:
        return error(message_id, "unknown_source", f"source is not served by this provider plugin: {source}")
    return runner.handle_execute(message)


def main() -> int:
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as exc:
            write(error("unknown", "bad_json", f"invalid JSON input: {exc}"))
            continue
        message_type = message.get("type")
        message_id = str(message.get("id", message_type or "unknown"))
        if message_type == "initialize":
            write({"id": message_id, "type": "initialized", "protocolVersion": PROTOCOL_VERSION, "resources": configured_sources()})
        elif message_type == "execute":
            write(handle_execute(message))
        elif message_type == "shutdown":
            write({"id": message_id, "type": "shutdown"})
            return 0
        else:
            write(error(message_id, "unknown_message_type", f"unsupported message type: {message_type}"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
