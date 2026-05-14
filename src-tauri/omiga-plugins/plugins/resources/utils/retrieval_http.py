#!/usr/bin/env python3
"""Small HTTP helpers shared by Omiga resource retrieval plugins."""

from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Optional, Tuple

DEFAULT_USER_AGENT = "Omiga retrieval plugin/0.1"


def merged_headers(
    headers: Optional[Dict[str, str]] = None,
    *,
    user_agent: str = DEFAULT_USER_AGENT,
    accept: str = "application/json",
) -> Dict[str, str]:
    out = {"User-Agent": user_agent, "Accept": accept}
    if headers:
        out.update(headers)
    return out


def fetch_text_with_headers(
    url: str,
    *,
    timeout: int = 25,
    headers: Optional[Dict[str, str]] = None,
    user_agent: str = DEFAULT_USER_AGENT,
    accept: str = "application/json",
) -> Tuple[str, Dict[str, str]]:
    req = urllib.request.Request(url, headers=merged_headers(headers, user_agent=user_agent, accept=accept))
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            body = response.read().decode("utf-8", errors="replace")
            return body, {key.lower(): value for key, value in response.headers.items()}
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")[:500]
        raise RuntimeError(f"HTTP {exc.code} from {url}: {body}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"request failed for {url}: {exc.reason}") from exc


def fetch_json_with_headers(
    url: str,
    *,
    timeout: int = 25,
    headers: Optional[Dict[str, str]] = None,
    user_agent: str = DEFAULT_USER_AGENT,
    accept: str = "application/json",
) -> Tuple[Any, Dict[str, str]]:
    body, response_headers = fetch_text_with_headers(
        url,
        timeout=timeout,
        headers=headers,
        user_agent=user_agent,
        accept=accept,
    )
    return json.loads(body), response_headers


def fetch_text(
    url: str,
    *,
    timeout: int = 25,
    headers: Optional[Dict[str, str]] = None,
    user_agent: str = DEFAULT_USER_AGENT,
    accept: str = "*/*",
) -> str:
    body, _ = fetch_text_with_headers(
        url,
        timeout=timeout,
        headers=headers,
        user_agent=user_agent,
        accept=accept,
    )
    return body


def fetch_json(
    url: str,
    *,
    timeout: int = 25,
    headers: Optional[Dict[str, str]] = None,
    user_agent: str = DEFAULT_USER_AGENT,
    accept: str = "application/json",
) -> Any:
    body, _ = fetch_json_with_headers(
        url,
        timeout=timeout,
        headers=headers,
        user_agent=user_agent,
        accept=accept,
    )
    return body


def with_query_credentials(
    url: str,
    credentials: Dict[str, str],
    *,
    api_key_key: str,
    api_key_param: str = "api_key",
    email_key: Optional[str] = None,
    email_param: str = "email",
    tool_key: Optional[str] = None,
    tool_param: str = "tool",
    default_email: Optional[str] = None,
    default_tool: Optional[str] = None,
) -> str:
    parts = urllib.parse.urlsplit(url)
    query = dict(urllib.parse.parse_qsl(parts.query, keep_blank_values=True))
    if credentials.get(api_key_key):
        query[api_key_param] = credentials[api_key_key]
    if email_key and credentials.get(email_key):
        query[email_param] = credentials[email_key]
    elif default_email:
        query.setdefault(email_param, default_email)
    if tool_key and credentials.get(tool_key):
        query[tool_param] = credentials[tool_key]
    elif default_tool:
        query.setdefault(tool_param, default_tool)
    encoded = urllib.parse.urlencode(query)
    return urllib.parse.urlunsplit((parts.scheme, parts.netloc, parts.path, encoded, parts.fragment))
