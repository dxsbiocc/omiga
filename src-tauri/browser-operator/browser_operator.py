#!/usr/bin/env python3
from __future__ import annotations

import argparse
import asyncio
import base64
import inspect
import json
import os
import sys
import tempfile
import traceback
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional


SNAPSHOT_JS = r"""
(maxElements, maxTextChars) => {
  const normalizeText = (value, limit = 200) => {
    const text = String(value || '').replace(/\s+/g, ' ').trim();
    return text.length > limit ? text.slice(0, limit) + '…' : text;
  };

  const cssEscape = (value) => {
    if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') {
      return CSS.escape(value);
    }
    return String(value).replace(/([ #;?%&,.+*~':"!^$\[\]()=>|\/\\@])/g, '\\$1');
  };

  const selectorFor = (el) => {
    if (!el || el.nodeType !== 1) return null;
    if (el.id) return `#${cssEscape(el.id)}`;
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== document.body) {
      let part = node.tagName.toLowerCase();
      if (node.classList && node.classList.length) {
        const stableClasses = Array.from(node.classList)
          .filter((name) => !/^((css|jsx)-|active|focus|hover|selected|disabled)/i.test(name))
          .slice(0, 2)
          .map((name) => `.${cssEscape(name)}`)
          .join('');
        part += stableClasses;
      }
      const parent = node.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter((child) => child.tagName === node.tagName);
        if (siblings.length > 1) {
          const nth = siblings.indexOf(node) + 1;
          part += `:nth-of-type(${nth})`;
        }
      }
      parts.unshift(part);
      const selector = parts.join(' > ');
      try {
        if (document.querySelector(selector) === el) return selector;
      } catch (_err) {
        // keep walking up
      }
      node = parent;
    }
    const selector = parts.join(' > ');
    return selector || null;
  };

  const isVisible = (el) => {
    const style = window.getComputedStyle(el);
    if (!style) return true;
    if (style.visibility === 'hidden' || style.display === 'none') return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };

  const selectors = [
    'a[href]',
    'button',
    'input',
    'select',
    'textarea',
    '[role="button"]',
    '[role="link"]',
    '[contenteditable="true"]',
    '[tabindex]'
  ].join(',');

  const nodes = Array.from(document.querySelectorAll(selectors));
  const seen = new Set();
  const elements = [];
  for (const el of nodes) {
    if (!(el instanceof HTMLElement)) continue;
    if (seen.has(el)) continue;
    seen.add(el);
    if (!isVisible(el)) continue;

    const selector = selectorFor(el);
    if (!selector) continue;

    const rect = el.getBoundingClientRect();
    const role = el.getAttribute('role');
    const type = el.getAttribute('type');
    const text = normalizeText(el.innerText || el.textContent || '');
    const ariaLabel = normalizeText(el.getAttribute('aria-label') || '');
    const placeholder = normalizeText(el.getAttribute('placeholder') || '');
    const name = normalizeText(el.getAttribute('name') || '');
    const title = normalizeText(el.getAttribute('title') || '');
    const label = normalizeText(
      el.labels && el.labels.length ? Array.from(el.labels).map((labelEl) => labelEl.innerText || labelEl.textContent || '').join(' ') : ''
    );
    const disabled = !!(el.matches(':disabled') || el.getAttribute('aria-disabled') === 'true');

    elements.push({
      index: elements.length,
      selector,
      tag: el.tagName.toLowerCase(),
      role,
      type,
      text,
      ariaLabel,
      placeholder,
      name,
      title,
      label,
      disabled,
      visible: true,
      bbox: {
        x: Math.round(rect.x),
        y: Math.round(rect.y),
        width: Math.round(rect.width),
        height: Math.round(rect.height),
      },
    });

    if (elements.length >= maxElements) break;
  }

  const pageText = normalizeText((document.body && document.body.innerText) || '', maxTextChars);
  return {
    text: pageText,
    elements,
  };
}
"""


SELECTOR_BY_INDEX_JS = r"""
(index) => {
  const cssEscape = (value) => {
    if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') {
      return CSS.escape(value);
    }
    return String(value).replace(/([ #;?%&,.+*~':"!^$\[\]()=>|\/\\@])/g, '\\$1');
  };

  const selectorFor = (el) => {
    if (!el || el.nodeType !== 1) return null;
    if (el.id) return `#${cssEscape(el.id)}`;
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== document.body) {
      let part = node.tagName.toLowerCase();
      const parent = node.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter((child) => child.tagName === node.tagName);
        if (siblings.length > 1) {
          part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
        }
      }
      parts.unshift(part);
      const selector = parts.join(' > ');
      try {
        if (document.querySelector(selector) === el) return selector;
      } catch (_err) {
      }
      node = parent;
    }
    return parts.join(' > ') || null;
  };

  const isVisible = (el) => {
    const style = window.getComputedStyle(el);
    if (!style) return true;
    if (style.visibility === 'hidden' || style.display === 'none') return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };

  const selectors = [
    'a[href]',
    'button',
    'input',
    'select',
    'textarea',
    '[role="button"]',
    '[role="link"]',
    '[contenteditable="true"]',
    '[tabindex]'
  ].join(',');

  const nodes = Array.from(document.querySelectorAll(selectors)).filter((el) => el instanceof HTMLElement && isVisible(el));
  const target = nodes[index];
  return target ? selectorFor(target) : null;
}
"""

SELECTOR_BY_TARGET_JS = r"""
(targetHint) => {
  const normalize = (value) => String(value || '').replace(/\s+/g, ' ').trim().toLowerCase();
  const hint = normalize(targetHint);
  if (!hint) return null;

  const cssEscape = (value) => {
    if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') {
      return CSS.escape(value);
    }
    return String(value).replace(/([ #;?%&,.+*~':"!^$\[\]()=>|\/\\@])/g, '\\$1');
  };

  const selectorFor = (el) => {
    if (!el || el.nodeType !== 1) return null;
    if (el.id) return `#${cssEscape(el.id)}`;
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== document.body) {
      let part = node.tagName.toLowerCase();
      const parent = node.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter((child) => child.tagName === node.tagName);
        if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
      }
      parts.unshift(part);
      const selector = parts.join(' > ');
      try {
        if (document.querySelector(selector) === el) return selector;
      } catch (_err) {}
      node = parent;
    }
    return parts.join(' > ') || null;
  };

  const isVisible = (el) => {
    const style = window.getComputedStyle(el);
    if (!style) return true;
    if (style.visibility === 'hidden' || style.display === 'none') return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };

  const selectors = [
    'a[href]',
    'button',
    'input',
    'select',
    'textarea',
    '[role="button"]',
    '[role="link"]',
    '[contenteditable="true"]',
    '[tabindex]'
  ].join(',');

  const nodes = Array.from(document.querySelectorAll(selectors)).filter((el) => el instanceof HTMLElement && isVisible(el));
  for (const el of nodes) {
    const labels = el.labels && el.labels.length
      ? Array.from(el.labels).map((labelEl) => labelEl.innerText || labelEl.textContent || '').join(' ')
      : '';
    const haystack = normalize([
      el.innerText,
      el.textContent,
      el.getAttribute('aria-label'),
      el.getAttribute('placeholder'),
      el.getAttribute('name'),
      el.getAttribute('title'),
      labels,
    ].filter(Boolean).join(' '));
    if (haystack === hint || haystack.includes(hint)) return selectorFor(el);
  }
  return null;
}
"""


JS_CLICK_FALLBACK = r"""
(selector) => {
  const el = document.querySelector(selector);
  if (!el) return { found: false };
  if (typeof el.scrollIntoView === 'function') {
    el.scrollIntoView({ block: 'center', inline: 'center' });
  }
  if (el instanceof HTMLElement) {
    el.focus({ preventScroll: true });
  }
  if (typeof el.click === 'function') {
    el.click();
  } else {
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window }));
  }
  return { found: true, tag: el.tagName.toLowerCase() };
}
"""


JS_FILL_FALLBACK = r"""
(selector, value) => {
  const el = document.querySelector(selector);
  if (!el) return { found: false };
  if (typeof el.scrollIntoView === 'function') {
    el.scrollIntoView({ block: 'center', inline: 'center' });
  }
  if (el instanceof HTMLElement) {
    el.focus({ preventScroll: true });
  }
  if ('value' in el) {
    el.value = '';
    el.value = value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { found: true, tag: el.tagName.toLowerCase() };
  }
  if (el.isContentEditable) {
    el.textContent = value;
    el.dispatchEvent(new InputEvent('input', { bubbles: true, data: value, inputType: 'insertText' }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { found: true, tag: el.tagName.toLowerCase() };
  }
  return { found: false, reason: 'not_fillable' };
}
"""


@dataclass
class RpcError(Exception):
    code: str
    message: str
    data: Optional[dict[str, Any]] = None

    def to_payload(self) -> dict[str, Any]:
        payload: dict[str, Any] = {"code": self.code, "message": self.message}
        if self.data:
            payload["data"] = self.data
        return payload


async def maybe_await(value: Any) -> Any:
    if inspect.isawaitable(value):
        return await value
    return value


class BrowserUseAdapter:
    def __init__(self) -> None:
        self.browser = None
        self.page = None
        self.browser_class = None
        self.browser_profile_class = None
        self.browser_use_version = None
        self.import_error: Optional[str] = None
        self._artifact_root = prepare_artifact_root(default_artifact_root())
        self._session_dir = self._artifact_root / f"session-{uuid.uuid4().hex[:12]}"
        self._session_dir.mkdir(parents=True, exist_ok=True)
        self.headless = parse_optional_bool(os.getenv("OMIGA_BROWSER_OPERATOR_HEADLESS"))
        self.cdp_url = (os.getenv("OMIGA_BROWSER_OPERATOR_CDP_URL") or "").strip() or None
        self._resolve_browser_use()

    def _resolve_browser_use(self) -> None:
        try:
            import importlib.metadata as importlib_metadata
        except Exception:  # pragma: no cover
            import importlib_metadata  # type: ignore

        try:
            import browser_use  # type: ignore

            self.browser_use_version = getattr(browser_use, "__version__", None)
            if not self.browser_use_version:
                try:
                    self.browser_use_version = importlib_metadata.version("browser-use")
                except Exception:
                    self.browser_use_version = None

            self.browser_class = getattr(browser_use, "Browser", None) or getattr(browser_use, "BrowserSession", None)
            self.browser_profile_class = getattr(browser_use, "BrowserProfile", None)
            if self.browser_class is None:
                raise RpcError(
                    "browser_use_unavailable",
                    "browser_use imported, but Browser/BrowserSession was not found.",
                )
        except RpcError as exc:
            self.import_error = exc.message
            self.browser_class = None
        except Exception as exc:
            self.import_error = f"{type(exc).__name__}: {exc}"
            self.browser_class = None

    @property
    def available(self) -> bool:
        return self.browser_class is not None

    def health(self) -> dict[str, Any]:
        return {
            "status": "ok",
            "browser_use_available": self.available,
            "browser_use_version": self.browser_use_version,
            "import_error": self.import_error,
            "headless": self.headless,
            "cdp_url_configured": bool(self.cdp_url),
            "session_started": self.browser is not None,
            "artifact_dir": str(self._artifact_root),
            "session_dir": str(self._session_dir),
        }

    async def ensure_browser(self) -> Any:
        if not self.available:
            raise RpcError(
                "browser_use_unavailable",
                "browser_use is not installed or does not expose Browser/BrowserSession.",
                {"detail": self.import_error},
            )
        if self.browser is not None:
            return self.browser

        kwargs: dict[str, Any] = {}
        if self.headless is not None:
            kwargs["headless"] = self.headless
        if self.cdp_url:
            kwargs["cdp_url"] = self.cdp_url

        try:
            self.browser = self.browser_class(**kwargs)
        except TypeError as exc:
            if self.browser_profile_class is None:
                raise RpcError(
                    "browser_use_api_mismatch",
                    "browser_use Browser constructor rejected known kwargs and BrowserProfile is unavailable.",
                    {"detail": str(exc), "kwargs": sorted(kwargs.keys())},
                )
            try:
                profile = self.browser_profile_class(**kwargs)
                self.browser = self.browser_class(browser_profile=profile)
            except Exception as nested_exc:
                raise RpcError(
                    "browser_use_api_mismatch",
                    "browser_use API did not accept Browser(...) or Browser(browser_profile=...).",
                    {"detail": f"{type(nested_exc).__name__}: {nested_exc}"},
                )
        except Exception as exc:
            raise RpcError(
                "browser_use_api_mismatch",
                "Failed to construct browser_use Browser session.",
                {"detail": f"{type(exc).__name__}: {exc}"},
            )

        await self._call_method(self.browser, "start")
        return self.browser

    async def close(self) -> dict[str, Any]:
        browser = self.browser
        self.page = None
        self.browser = None
        if browser is None:
            return {"closed": True, "was_open": False}

        for method_name in ("stop", "close"):
            if hasattr(browser, method_name):
                try:
                    await self._call_method(browser, method_name)
                    return {"closed": True, "was_open": True, "method": method_name}
                except Exception:
                    continue
        return {"closed": True, "was_open": True, "method": None}

    async def open(self, params: dict[str, Any]) -> dict[str, Any]:
        url = require_str(params, "url")
        page = await self._ensure_page(url=url, prefer_existing=True)
        current_url = await self._page_text(page, "get_url", "() => location.href")
        title = await self._page_text(page, "get_title", "() => document.title")
        return {"url": current_url, "title": title}

    async def snapshot(self, params: dict[str, Any]) -> dict[str, Any]:
        page = await self._ensure_page()
        max_elements = require_int(params, "max_elements", default=200, minimum=1, maximum=2000)
        max_text_chars = require_int(params, "max_text_chars", default=20000, minimum=1, maximum=200000)
        meta = await self._evaluate(page, SNAPSHOT_JS, max_elements, max_text_chars)
        url = await self._page_text(page, "get_url", "() => location.href")
        title = await self._page_text(page, "get_title", "() => document.title")
        return {
            "url": url,
            "title": title,
            "text": meta.get("text", "") if isinstance(meta, dict) else "",
            "elements": meta.get("elements", []) if isinstance(meta, dict) else [],
        }

    async def click(self, params: dict[str, Any]) -> dict[str, Any]:
        page = await self._ensure_page()
        selector = await self._resolve_selector(page, params)

        native_error = None
        if hasattr(page, "get_elements_by_css_selector"):
            try:
                elements = await self._call_method(page, "get_elements_by_css_selector", selector)
                if elements:
                    await self._call_method(elements[0], "click")
                    return {"clicked": True, "selector": selector}
            except Exception as exc:
                native_error = f"{type(exc).__name__}: {exc}"

        fallback = await self._evaluate(page, JS_CLICK_FALLBACK, selector)
        if not isinstance(fallback, dict) or not fallback.get("found"):
            raise RpcError(
                "element_not_found",
                "Could not find clickable element for selector.",
                {"selector": selector, "native_error": native_error},
            )
        return {"clicked": True, "selector": selector, "fallback": True}

    async def fill(self, params: dict[str, Any]) -> dict[str, Any]:
        page = await self._ensure_page()
        selector = await self._resolve_selector(page, params)
        value = require_str(params, "value")

        native_error = None
        if hasattr(page, "get_elements_by_css_selector"):
            try:
                elements = await self._call_method(page, "get_elements_by_css_selector", selector)
                if elements:
                    target = elements[0]
                    try:
                        await self._call_method(target, "fill", value, True)
                    except TypeError:
                        await self._call_method(target, "fill", value)
                    return {"filled": True, "selector": selector, "redacted": True}
            except Exception as exc:
                native_error = f"{type(exc).__name__}: {exc}"

        fallback = await self._evaluate(page, JS_FILL_FALLBACK, selector, value)
        if not isinstance(fallback, dict) or not fallback.get("found"):
            raise RpcError(
                "action_failed",
                "Could not fill the target element.",
                {"selector": selector, "native_error": native_error},
            )
        return {"filled": True, "selector": selector, "redacted": True, "fallback": True}

    async def screenshot(self, params: dict[str, Any]) -> dict[str, Any]:
        page = await self._ensure_page()
        fmt = require_str(params, "format", default="png")
        if fmt not in {"png", "jpeg", "jpg"}:
            raise RpcError("invalid_params", "format must be png, jpeg, or jpg")
        quality = params.get("quality")

        screenshot_method = getattr(page, "screenshot", None)
        if screenshot_method is None:
            raise RpcError("browser_use_api_mismatch", "Current browser_use page object has no screenshot method.")

        try:
            if quality is None:
                payload = await maybe_await(screenshot_method(format=fmt))
            else:
                payload = await maybe_await(screenshot_method(format=fmt, quality=quality))
        except TypeError:
            payload = await maybe_await(screenshot_method())
        except Exception as exc:
            raise RpcError("screenshot_failed", "Failed to capture screenshot.", {"detail": f"{type(exc).__name__}: {exc}"})

        path = persist_screenshot_payload(payload, self._session_dir, fmt)
        return {"path": str(path)}

    async def _ensure_page(self, url: Optional[str] = None, prefer_existing: bool = False) -> Any:
        browser = await self.ensure_browser()
        page = self.page

        if page is None:
            page = await self._existing_page(browser)

        if page is None and url is not None:
            page = await self._new_page(browser, url)
        elif page is not None and url is not None:
            if prefer_existing and hasattr(page, "goto"):
                await self._call_method(page, "goto", url)
            else:
                page = await self._new_page(browser, url)

        if page is None:
            raise RpcError("browser_not_open", "No active browser page. Call open first.")

        self.page = page
        return page

    async def _existing_page(self, browser: Any) -> Any:
        if hasattr(browser, "get_current_page"):
            page = await self._call_method(browser, "get_current_page")
            if page is not None:
                return page
        if hasattr(browser, "get_pages"):
            pages = await self._call_method(browser, "get_pages")
            if pages:
                return pages[0]
        return None

    async def _new_page(self, browser: Any, url: str) -> Any:
        if not hasattr(browser, "new_page"):
            raise RpcError(
                "browser_use_api_mismatch",
                "Current browser_use Browser session has no new_page method.",
            )
        try:
            page = await self._call_method(browser, "new_page", url)
        except TypeError:
            page = await self._call_method(browser, "new_page")
            if hasattr(page, "goto"):
                await self._call_method(page, "goto", url)
            else:
                raise RpcError(
                    "browser_use_api_mismatch",
                    "new_page() succeeded but the returned page has no goto method.",
                )
        return page

    async def _resolve_selector(self, page: Any, params: dict[str, Any]) -> str:
        selector = params.get("selector")
        if selector is not None:
            if not isinstance(selector, str) or not selector.strip():
                raise RpcError("invalid_params", "selector must be a non-empty string")
            return selector.strip()
        if "index" in params:
            index = require_int(params, "index", minimum=0)
            selector = await self._evaluate(page, SELECTOR_BY_INDEX_JS, index)
            if not selector:
                raise RpcError("element_not_found", "No element matched the requested snapshot index.", {"index": index})
            return selector
        target = params.get("target")
        if isinstance(target, str) and target.strip():
            selector = await self._evaluate(page, SELECTOR_BY_TARGET_JS, target.strip())
            if not selector:
                raise RpcError("element_not_found", "No element matched the requested target hint.", {"target": target.strip()})
            return selector
        raise RpcError("invalid_params", "Provide selector, index, or target")

    async def _page_text(self, page: Any, method_name: str, fallback_js: str) -> str:
        if hasattr(page, method_name):
            try:
                value = await self._call_method(page, method_name)
                if value is None:
                    return ""
                return str(value)
            except Exception:
                pass
        try:
            value = await self._evaluate(page, fallback_js)
            return "" if value is None else str(value)
        except Exception:
            return ""

    async def _evaluate(self, page: Any, script: str, *args: Any) -> Any:
        if not hasattr(page, "evaluate"):
            raise RpcError("browser_use_api_mismatch", "Current browser_use page object has no evaluate method.")
        try:
            return await self._call_method(page, "evaluate", script, *args)
        except Exception as exc:
            raise RpcError(
                "browser_use_api_mismatch",
                "browser_use page.evaluate failed.",
                {"detail": f"{type(exc).__name__}: {exc}"},
            )

    async def _call_method(self, obj: Any, method_name: str, *args: Any, **kwargs: Any) -> Any:
        if not hasattr(obj, method_name):
            raise RpcError("browser_use_api_mismatch", f"Object has no method {method_name}.")
        method = getattr(obj, method_name)
        return await maybe_await(method(*args, **kwargs))


class JsonLineRpcServer:
    def __init__(self) -> None:
        self.adapter = BrowserUseAdapter()
        self.handlers = {
            "health": self._handle_health,
            "open": self._handle_open,
            "snapshot": self._handle_snapshot,
            "click": self._handle_click,
            "fill": self._handle_fill,
            "screenshot": self._handle_screenshot,
            "close": self._handle_close,
        }

    async def process_line(self, raw_line: str) -> dict[str, Any]:
        raw_line = raw_line.strip()
        if not raw_line:
            raise RpcError("invalid_json", "Input line is empty")

        try:
            payload = json.loads(raw_line)
        except json.JSONDecodeError as exc:
            return error_response(None, RpcError("invalid_json", f"Invalid JSON: {exc.msg}", {"line": exc.lineno, "column": exc.colno}))

        request_id = payload.get("id") if isinstance(payload, dict) else None
        try:
            if not isinstance(payload, dict):
                raise RpcError("invalid_request", "Request must be a JSON object")
            method = payload.get("method")
            if not isinstance(method, str) or not method:
                raise RpcError("invalid_request", "method must be a non-empty string")
            params = payload.get("params", {})
            if params is None:
                params = {}
            if not isinstance(params, dict):
                raise RpcError("invalid_request", "params must be an object when provided")
            handler = self.handlers.get(method)
            if handler is None:
                raise RpcError("unknown_method", f"Unknown method: {method}")
            result = await handler(params)
            return {"id": request_id, "ok": True, "result": result}
        except RpcError as exc:
            return error_response(request_id, exc)
        except Exception as exc:  # pragma: no cover - unexpected path
            return error_response(
                request_id,
                RpcError(
                    "internal_error",
                    f"Unexpected error: {type(exc).__name__}: {exc}",
                    {"traceback": traceback.format_exc(limit=5)},
                ),
            )

    async def serve_forever(self) -> int:
        for raw_line in sys.stdin:
            response = await self.process_line(raw_line)
            sys.stdout.write(json.dumps(response, ensure_ascii=False) + "\n")
            sys.stdout.flush()
        await self.adapter.close()
        return 0

    async def _handle_health(self, _params: dict[str, Any]) -> dict[str, Any]:
        return self.adapter.health()

    async def _handle_open(self, params: dict[str, Any]) -> dict[str, Any]:
        return await self.adapter.open(params)

    async def _handle_snapshot(self, params: dict[str, Any]) -> dict[str, Any]:
        return await self.adapter.snapshot(params)

    async def _handle_click(self, params: dict[str, Any]) -> dict[str, Any]:
        return await self.adapter.click(params)

    async def _handle_fill(self, params: dict[str, Any]) -> dict[str, Any]:
        return await self.adapter.fill(params)

    async def _handle_screenshot(self, params: dict[str, Any]) -> dict[str, Any]:
        return await self.adapter.screenshot(params)

    async def _handle_close(self, _params: dict[str, Any]) -> dict[str, Any]:
        return await self.adapter.close()


async def run_self_test() -> int:
    server = JsonLineRpcServer()
    cases = [
        ("{bad json", "invalid_json"),
        (json.dumps({"id": 1, "method": "health", "params": {}}), None),
        (json.dumps({"id": 2, "method": "unknown", "params": {}}), "unknown_method"),
    ]

    for raw, expected_code in cases:
        response = await server.process_line(raw)
        if expected_code is None:
            if not response.get("ok"):
                print(json.dumps(response, ensure_ascii=False), file=sys.stderr)
                return 1
            if response.get("id") != 1:
                print("self-test failed: health response id mismatch", file=sys.stderr)
                return 1
            result = response.get("result") or {}
            if result.get("status") != "ok":
                print("self-test failed: health status mismatch", file=sys.stderr)
                return 1
        else:
            error = response.get("error") or {}
            if error.get("code") != expected_code:
                print(json.dumps(response, ensure_ascii=False), file=sys.stderr)
                return 1

    print("SELF-TEST OK")
    return 0


def error_response(request_id: Any, error: RpcError) -> dict[str, Any]:
    return {"id": request_id, "ok": False, "error": error.to_payload()}


def parse_optional_bool(value: Optional[str]) -> Optional[bool]:
    if value is None:
        return None
    normalized = value.strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    return None


def default_artifact_root() -> Path:
    configured = os.getenv("OMIGA_BROWSER_OPERATOR_ARTIFACT_DIR")
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".omiga" / "browser-operator" / "artifacts"


def prepare_artifact_root(root: Path) -> Path:
    try:
        root.mkdir(parents=True, exist_ok=True)
        return root
    except OSError:
        # Keep artifacts in an Omiga-owned directory even when the default
        # home location is unavailable, e.g. sandboxed verification runs.
        fallback = Path(tempfile.gettempdir()) / "omiga-browser-operator" / "artifacts"
        fallback.mkdir(parents=True, exist_ok=True)
        return fallback


def require_str(params: dict[str, Any], key: str, default: Optional[str] = None) -> str:
    value = params.get(key, default)
    if not isinstance(value, str) or not value.strip():
        raise RpcError("invalid_params", f"{key} must be a non-empty string")
    return value.strip()


def require_int(
    params: dict[str, Any],
    key: str,
    default: Optional[int] = None,
    minimum: Optional[int] = None,
    maximum: Optional[int] = None,
) -> int:
    value = params.get(key, default)
    if not isinstance(value, int):
        raise RpcError("invalid_params", f"{key} must be an integer")
    if minimum is not None and value < minimum:
        raise RpcError("invalid_params", f"{key} must be >= {minimum}")
    if maximum is not None and value > maximum:
        raise RpcError("invalid_params", f"{key} must be <= {maximum}")
    return value


def maybe_existing_artifact_source(payload: Any) -> Optional[Path]:
    if isinstance(payload, str):
        candidate = Path(payload.strip()).expanduser()
        return candidate if candidate.is_file() else None
    if isinstance(payload, dict):
        for key in ("path", "filePath", "filepath"):
            value = payload.get(key)
            if isinstance(value, str):
                candidate = Path(value.strip()).expanduser()
                if candidate.is_file():
                    return candidate
    return None


def persist_screenshot_payload(payload: Any, session_dir: Path, fmt: str) -> Path:
    session_dir.mkdir(parents=True, exist_ok=True)
    suffix = ".jpg" if fmt in {"jpg", "jpeg"} else ".png"
    target = session_dir / f"screenshot-{uuid.uuid4().hex[:12]}{suffix}"
    existing_source = maybe_existing_artifact_source(payload)
    if existing_source is not None:
        target.write_bytes(existing_source.read_bytes())
        return target

    if isinstance(payload, (bytes, bytearray)):
        target.write_bytes(bytes(payload))
        return target

    if isinstance(payload, str):
        trimmed = payload.strip()
        if trimmed.startswith("data:") and "," in trimmed:
            trimmed = trimmed.split(",", 1)[1]
        try:
            target.write_bytes(base64.b64decode(trimmed, validate=True))
            return target
        except Exception:
            target.write_text(trimmed, encoding="utf-8")
            return target

    target.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    return target


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="OMIGA Browser Operator Python sidecar")
    parser.add_argument("--self-test", action="store_true", help="Validate JSON parsing and health handling without opening a browser")
    return parser


async def async_main(argv: list[str]) -> int:
    parser = build_arg_parser()
    args = parser.parse_args(argv)
    if args.self_test:
        return await run_self_test()
    server = JsonLineRpcServer()
    return await server.serve_forever()


def main() -> int:
    return asyncio.run(async_main(sys.argv[1:]))


if __name__ == "__main__":
    raise SystemExit(main())
