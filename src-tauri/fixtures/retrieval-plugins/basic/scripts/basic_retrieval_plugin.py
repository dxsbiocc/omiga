#!/usr/bin/env python3
"""Runnable fixture for Omiga's local retrieval plugin JSONL protocol."""

import json
import sys
from typing import Any, Dict, List

PROTOCOL_VERSION = 1
SOURCE = {
    "category": "dataset",
    "id": "example_dataset",
    "capabilities": ["search", "query", "fetch"],
}


def write(message: Dict[str, Any]) -> None:
    print(json.dumps(message, separators=(",", ":")), flush=True)


def metadata(request: Dict[str, Any]) -> Dict[str, Any]:
    params = request.get("params") if isinstance(request.get("params"), dict) else {}
    credentials = request.get("credentials") if isinstance(request.get("credentials"), dict) else {}
    return {
        "organism": params.get("organism", "human"),
        "credentialRefs": sorted(credentials.keys()),
        "fixture": "retrieval-protocol-example",
    }


def item(request: Dict[str, Any], index: int = 1) -> Dict[str, Any]:
    query = request.get("query") or request.get("id") or "example"
    return {
        "id": f"example-{index}",
        "accession": f"EXAMPLE:{index}",
        "title": f"Example result {index} for {query}",
        "url": f"https://example.test/datasets/example-{index}",
        "snippet": "Fixture response from the local retrieval plugin protocol.",
        "content": f"Detailed fixture content for {query}.",
        "metadata": metadata(request),
        "raw": {"echo": request},
    }


def result(message_id: str, request: Dict[str, Any]) -> Dict[str, Any]:
    operation = request.get("operation") or "search"
    base = {
        "ok": True,
        "operation": operation,
        "category": request.get("category", SOURCE["category"]),
        "source": request.get("source", SOURCE["id"]),
        "effectiveSource": SOURCE["id"],
        "notes": ["fixture response"],
        "raw": {"protocolFixture": True},
    }
    if operation == "fetch":
        base.update({"items": [], "detail": item(request, 1), "total": 1})
    elif operation == "query":
        base.update({"items": [item(request, 1), item(request, 2)], "total": 2})
    else:
        base.update({"items": [item(request, 1)], "total": 1})
    return {"id": message_id, "type": "result", "response": base}


def error(message_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": message_id, "type": "error", "error": {"code": code, "message": message}}


def handle_execute(message: Dict[str, Any]) -> Dict[str, Any]:
    request = message.get("request") if isinstance(message.get("request"), dict) else {}
    if request.get("source") not in (None, SOURCE["id"]):
        return error(message.get("id", "execute"), "unknown_source", "source is not registered by this fixture")
    if request.get("operation") not in ("search", "query", "fetch"):
        return error(message.get("id", "execute"), "unsupported_operation", "fixture supports search, query, and fetch")
    params = request.get("params") if isinstance(request.get("params"), dict) else {}
    if params.get("forceError"):
        return error(message.get("id", "execute"), "forced_error", "forced fixture error")
    return result(message.get("id", "execute"), request)


def main() -> int:
    for line in sys.stdin:
        if not line.strip():
            continue
        message = json.loads(line)
        message_type = message.get("type")
        message_id = message.get("id", message_type or "unknown")
        if message_type == "initialize":
            write({
                "id": message_id,
                "type": "initialized",
                "protocolVersion": PROTOCOL_VERSION,
                "resources": [SOURCE],
            })
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
