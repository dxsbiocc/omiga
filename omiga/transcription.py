"""Voice/audio transcription via OpenAI Whisper API.

Usage
-----
Set in .env:
    WHISPER_ENABLED=true
    OPENAI_API_KEY=sk-...

Supported audio formats: mp3, mp4, mpeg, mpga, m4a, wav, webm, ogg
(all formats Telegram/Feishu/Discord may send).

The module is intentionally dependency-light — it uses ``httpx`` (already
pulled in by python-telegram-bot) so no extra packages are needed.
"""
from __future__ import annotations

import logging
import mimetypes
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)

# Whisper API endpoint
_WHISPER_URL = "https://api.openai.com/v1/audio/transcriptions"
_WHISPER_MODEL = "whisper-1"

# Map file extensions to MIME types not covered by mimetypes stdlib
_EXT_MIME: dict[str, str] = {
    ".ogg": "audio/ogg",
    ".oga": "audio/ogg",
    ".opus": "audio/ogg",
    ".m4a": "audio/mp4",
    ".weba": "audio/webm",
}


def _mime_for(path: Path) -> str:
    ext = path.suffix.lower()
    if ext in _EXT_MIME:
        return _EXT_MIME[ext]
    mime, _ = mimetypes.guess_type(str(path))
    return mime or "audio/mpeg"


async def transcribe_audio(path: Path, language: Optional[str] = None) -> Optional[str]:
    """Transcribe *path* using OpenAI Whisper and return the text.

    Returns ``None`` if transcription is disabled, the API key is missing,
    or the request fails for any reason.

    Parameters
    ----------
    path:
        Absolute path to the audio file on disk.
    language:
        Optional BCP-47 language hint (e.g. ``"zh"``).  When ``None``
        Whisper auto-detects the language.
    """
    from omiga.config import WHISPER_ENABLED, get_secret

    if not WHISPER_ENABLED:
        return None

    if not path.exists() or path.stat().st_size == 0:
        logger.warning("Transcription skipped: file missing or empty — %s", path)
        return None

    api_key = get_secret("OPENAI_API_KEY")
    if not api_key:
        logger.warning("Transcription skipped: OPENAI_API_KEY not set")
        return None

    try:
        import httpx

        mime = _mime_for(path)
        audio_bytes = path.read_bytes()

        files = {"file": (path.name, audio_bytes, mime)}
        data: dict[str, str] = {"model": _WHISPER_MODEL}
        if language:
            data["language"] = language

        async with httpx.AsyncClient(timeout=90) as client:
            resp = await client.post(
                _WHISPER_URL,
                headers={"Authorization": f"Bearer {api_key}"},
                files=files,
                data=data,
            )

        if resp.status_code != 200:
            logger.error(
                "Whisper API error %s for %s: %s",
                resp.status_code,
                path.name,
                resp.text[:300],
            )
            return None

        text = resp.json().get("text", "").strip()
        if text:
            logger.debug("Transcribed %s → %d chars", path.name, len(text))
        return text or None

    except Exception as exc:
        logger.error("Transcription failed for %s: %s", path, exc)
        return None
