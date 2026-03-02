"""Channel factory for Omiga.

Instantiates and connects communication channels based on environment config.
"""
from __future__ import annotations

import logging
import os

import omiga.state as state
from omiga.channels.base import Channel, StubChannel
from omiga.config import get_secret

logger = logging.getLogger("omiga.channel_setup")


def resolve_proxy(channel_env_key: str) -> str:
    """Return the proxy URL to use for a channel.

    Priority:
      1. Channel-specific env var  (e.g. TELEGRAM_HTTP_PROXY)
      2. System HTTPS_PROXY / ALL_PROXY / HTTP_PROXY env vars
      3. Empty string (direct connection)

    This lets users rely on a system-wide proxy (set once in the shell or
    via a VPN tool) and only override per-channel when needed.
    """
    explicit = get_secret(channel_env_key)
    if explicit:
        return explicit
    return (
        os.environ.get("HTTPS_PROXY") or os.environ.get("https_proxy")
        or os.environ.get("ALL_PROXY") or os.environ.get("all_proxy")
        or os.environ.get("HTTP_PROXY") or os.environ.get("http_proxy")
        or ""
    )


async def build_channels(
    on_message,
    on_chat_meta,
) -> list[Channel]:
    """Instantiate and connect channels based on environment config.

    Priority:
      1. Telegram  — when TELEGRAM_BOT_TOKEN is set
      2. Feishu    — when FEISHU_APP_ID + FEISHU_APP_SECRET are set
      3. QQ        — when QQ_APP_ID + QQ_APP_SECRET are set
      4. Discord   — when DISCORD_BOT_TOKEN is set
      5. Stub      — always added last as catch-all (useful for testing)

    Returns the connected channel list.
    """
    channels: list[Channel] = []

    telegram_token = get_secret("TELEGRAM_BOT_TOKEN")
    if telegram_token:
        from omiga.channels.telegram import TelegramChannel
        tg = TelegramChannel(
            token=telegram_token,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: state._registered_groups,
            http_proxy=resolve_proxy("TELEGRAM_HTTP_PROXY"),
        )
        await tg.connect()
        channels.append(tg)
        logger.info("Telegram channel active")

    feishu_app_id = get_secret("FEISHU_APP_ID")
    feishu_app_secret = get_secret("FEISHU_APP_SECRET")
    if feishu_app_id and feishu_app_secret:
        from omiga.channels.feishu import FeishuChannel
        fs = FeishuChannel(
            app_id=feishu_app_id,
            app_secret=feishu_app_secret,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: state._registered_groups,
        )
        await fs.connect()
        channels.append(fs)
        logger.info("Feishu channel active")

    qq_app_id = get_secret("QQ_APP_ID")
    qq_app_secret = get_secret("QQ_APP_SECRET")
    if qq_app_id and qq_app_secret:
        from omiga.channels.qq import QQChannel
        qq = QQChannel(
            app_id=qq_app_id,
            app_secret=qq_app_secret,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: state._registered_groups,
        )
        await qq.connect()
        channels.append(qq)
        logger.info("QQ channel active")

    discord_token = get_secret("DISCORD_BOT_TOKEN")
    if discord_token:
        from omiga.channels.discord_ import DiscordChannel
        dc = DiscordChannel(
            token=discord_token,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: state._registered_groups,
            http_proxy=resolve_proxy("DISCORD_HTTP_PROXY"),
        )
        await dc.connect()
        channels.append(dc)
        logger.info("Discord channel active")

    if not channels:
        logger.info("No channel tokens set — using StubChannel")
        stub = StubChannel()
        await stub.connect()
        channels.append(stub)

    return channels
