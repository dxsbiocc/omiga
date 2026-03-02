"""
Playwright browser automation for the NanoClaw Python agent.

One browser instance per agent process (module-level singleton).
Created on first use, cleaned up at process exit via atexit.

Usage in tool definitions:
  BrowserNavigate(url)
  BrowserSnapshot()
  BrowserClick(selector)
  BrowserFill(selector, value)
  BrowserScroll(direction)
  BrowserBack()
  BrowserClose()
"""
from __future__ import annotations

import atexit
import subprocess
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from playwright.sync_api import Browser, BrowserContext, Page, Playwright

_pw: "Playwright | None" = None
_browser: "Browser | None" = None
_page: "Page | None" = None


# ── Lifecycle ───────────────────────────────────────────────────────────────

def _launch() -> "Page":
    """Launch Chromium (auto-install browsers if missing) and return a Page."""
    global _pw, _browser, _page
    from playwright.sync_api import sync_playwright

    _pw = sync_playwright().start()
    try:
        _browser = _pw.chromium.launch(
            headless=True,
            args=["--no-sandbox", "--disable-setuid-sandbox", "--disable-dev-shm-usage"],
        )
    except Exception as exc:
        if "Executable doesn't exist" in str(exc) or "not found" in str(exc).lower():
            _pw.stop()
            _pw = None
            _install_browsers()
            _pw = sync_playwright().start()
            _browser = _pw.chromium.launch(
                headless=True,
                args=["--no-sandbox", "--disable-setuid-sandbox", "--disable-dev-shm-usage"],
            )
        else:
            raise

    _page = _browser.new_page(viewport={"width": 1280, "height": 800})
    _page.set_default_timeout(15_000)
    atexit.register(_cleanup)
    return _page


def _install_browsers() -> None:
    print("[browser] Installing Chromium (one-time setup)…", file=sys.stderr, flush=True)
    result = subprocess.run(
        [sys.executable, "-m", "playwright", "install", "chromium", "--with-deps"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"playwright install failed:\n{result.stderr}")
    print("[browser] Chromium installed.", file=sys.stderr, flush=True)


def _get_page() -> "Page":
    global _page
    if _page is None or _page.is_closed():
        _page = _launch()
    return _page


def _cleanup() -> None:
    global _pw, _browser, _page
    for obj, method in [(_page, "close"), (_browser, "close"), (_pw, "stop")]:
        if obj is not None:
            try:
                getattr(obj, method)()
            except Exception:
                pass
    _pw = _browser = _page = None


# ── Tools ────────────────────────────────────────────────────────────────────

def navigate(url: str) -> str:
    """Navigate to a URL."""
    page = _get_page()
    try:
        page.goto(url, wait_until="domcontentloaded", timeout=20_000)
        return f"Navigated to: {page.url}\nTitle: {page.title()}"
    except Exception as e:
        return f"[Navigation error: {e}]"


def snapshot() -> str:
    """
    Return a structured text snapshot of the current page:
    URL, title, interactive elements (links / inputs / buttons), page text.

    Use element labels returned here to target BrowserClick and BrowserFill.
    """
    page = _get_page()

    lines = [
        f"URL: {page.url}",
        f"Title: {page.title()}",
        "",
    ]

    def safe_text(loc, attr: str = "") -> str:
        try:
            return (loc.inner_text(timeout=500) if not attr
                    else loc.get_attribute(attr) or "").strip().replace("\n", " ")
        except Exception:
            return ""

    # Links
    link_els = page.locator("a:visible").all()[:30]
    if link_els:
        lines.append("## Links")
        for i, el in enumerate(link_els, 1):
            text = safe_text(el)[:80]
            href = safe_text(el, "href")[:70]
            if text:
                lines.append(f"  [{i}] {text}" + (f"  ({href})" if href else ""))
        lines.append("")

    # Inputs
    input_els = page.locator(
        "input:visible:not([type=hidden]):not([type=submit]):not([type=button]),"
        "textarea:visible, select:visible"
    ).all()[:15]
    if input_els:
        lines.append("## Inputs")
        for i, el in enumerate(input_els, 1):
            label = (
                safe_text(el, "placeholder") or
                safe_text(el, "aria-label") or
                safe_text(el, "name") or
                safe_text(el, "type") or
                "input"
            )
            try:
                val = el.input_value(timeout=300)
            except Exception:
                val = ""
            lines.append(f"  [{i}] {label}" + (f" = {val!r}" if val else ""))
        lines.append("")

    # Buttons
    btn_els = page.locator(
        "button:visible, input[type=submit]:visible, input[type=button]:visible"
    ).all()[:20]
    if btn_els:
        lines.append("## Buttons")
        for i, el in enumerate(btn_els, 1):
            text = (
                safe_text(el) or
                safe_text(el, "value") or
                safe_text(el, "aria-label")
            )[:60]
            if text:
                lines.append(f"  [{i}] {text}")
        lines.append("")

    # Page text
    try:
        body = page.locator("body").inner_text(timeout=2_000)
        body = "\n".join(l for l in body.splitlines() if l.strip())[:4_000]
        lines.append("## Page Text")
        lines.append(body)
    except Exception:
        pass

    return "\n".join(lines)


def click(selector: str) -> str:
    """
    Click an element on the page.

    selector formats:
      - plain text  → clicks first element whose visible text matches
      - "css:..."   → use as CSS selector
      - "label:..."  → click by aria-label
    """
    page = _get_page()
    try:
        if selector.startswith("css:"):
            loc = page.locator(selector[4:]).first
        elif selector.startswith("label:"):
            loc = page.get_by_label(selector[6:]).first
        else:
            loc = page.get_by_text(selector, exact=False).first

        loc.click(timeout=8_000)
        try:
            page.wait_for_load_state("domcontentloaded", timeout=5_000)
        except Exception:
            pass
        return f"Clicked {selector!r}. Now at: {page.url}"
    except Exception as e:
        return f"[Click error: {e}]"


def fill(selector: str, value: str) -> str:
    """
    Fill a text input or textarea.

    selector formats:
      - plain text       → tries placeholder, aria-label, name attr, then css input/textarea
      - "css:..."        → CSS selector directly
      - "label:..."      → aria-label exact match
      - "placeholder:..." → placeholder exact match
    """
    page = _get_page()

    _FILL_TIMEOUT = 5_000
    _INPUT_CSS = "input:not([type=hidden]):not([type=submit]):not([type=button]):not([type=checkbox]):not([type=radio]), textarea"

    def _do_fill(loc) -> bool:
        """Try to fill; if loc resolves to a container, look for input inside it."""
        try:
            loc.first.fill(value, timeout=_FILL_TIMEOUT)
            return True
        except Exception as e:
            # If the matched element is a container (form, div), search inside it
            if "not an <input>" in str(e) or "not a fillable" in str(e).lower():
                inner = loc.first.locator(_INPUT_CSS)
                if inner.count() > 0:
                    inner.first.fill(value, timeout=_FILL_TIMEOUT)
                    return True
            raise

    try:
        if selector.startswith("css:"):
            _do_fill(page.locator(selector[4:]))
        elif selector.startswith("label:"):
            _do_fill(page.get_by_label(selector[6:]))
        elif selector.startswith("placeholder:"):
            _do_fill(page.get_by_placeholder(selector[12:]))
        else:
            # Try in order: placeholder → label → name attr → visible inputs
            for attempt in [
                lambda: page.get_by_placeholder(selector),
                lambda: page.get_by_label(selector),
                lambda: page.locator(f'{_INPUT_CSS}[name="{selector}"]'),
                lambda: page.locator(_INPUT_CSS),
            ]:
                try:
                    _do_fill(attempt())
                    break
                except Exception:
                    continue
            else:
                return f"[Fill error: could not find input for {selector!r}]"

        return f"Filled {selector!r} with {value!r}"
    except Exception as e:
        return f"[Fill error: {e}]"


def press_key(key: str) -> str:
    """
    Press a keyboard key on the current focused element.
    Common keys: Enter, Tab, Escape, ArrowDown, ArrowUp, Backspace
    """
    page = _get_page()
    try:
        page.keyboard.press(key)
        return f"Pressed key: {key}"
    except Exception as e:
        return f"[Key error: {e}]"


def scroll(direction: str = "down", amount: int = 3) -> str:
    """Scroll the page. direction: up | down | left | right"""
    page = _get_page()
    try:
        delta = amount * 300
        dx = delta if direction == "right" else (-delta if direction == "left" else 0)
        dy = delta if direction == "down"  else (-delta if direction == "up"    else 0)
        page.mouse.wheel(dx, dy)
        return f"Scrolled {direction}"
    except Exception as e:
        return f"[Scroll error: {e}]"


def go_back() -> str:
    """Navigate back in browser history."""
    page = _get_page()
    try:
        page.go_back(wait_until="domcontentloaded", timeout=8_000)
        return f"Back to: {page.url}"
    except Exception as e:
        return f"[Back error: {e}]"


def close() -> str:
    """Close the browser and free resources."""
    _cleanup()
    return "Browser closed."
