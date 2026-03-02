"""News skill for fetching and summarizing news."""
from __future__ import annotations

import logging
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Optional

from omiga.skills.base import Skill, SkillContext, SkillMetadata, SkillError

logger = logging.getLogger("omiga.skills.news")


@dataclass
class NewsItem:
    """A news item."""

    title: str
    link: str
    published: Optional[str] = None
    summary: Optional[str] = None
    source: Optional[str] = None


class NewsSkill(Skill):
    """Skill for fetching and summarizing news."""

    metadata = SkillMetadata(
        name="news",
        description="获取和总结新闻摘要",
        emoji="📰",
        tags=["news", "rss", "summary"],
    )

    # Default RSS feeds
    DEFAULT_FEEDS = {
        "techcrunch": "https://techcrunch.com/feed/",
        "hackernews": "https://news.ycombinator.com/rss",
        "reddit_python": "https://www.reddit.com/r/Python/.rss",
    }

    def __init__(self, context: SkillContext):
        super().__init__(context)

    async def execute(
        self,
        action: str = "list_feeds",
        feed_url: Optional[str] = None,
        feed_name: Optional[str] = None,
        limit: int = 10,
        **kwargs: Any,
    ) -> Any:
        """Execute the news skill.

        Args:
            action: Action to perform (list_feeds, fetch, summarize)
            feed_url: URL of the RSS feed
            feed_name: Name of a predefined feed
            limit: Maximum number of items to return
            **kwargs: Additional arguments

        Returns:
            News items or feed list
        """
        actions = {
            "list_feeds": self._list_feeds,
            "fetch": self._fetch_feed,
            "summarize": self._summarize_feed,
        }

        if action not in actions:
            raise SkillError(f"Unknown action: {action}", self.name)

        return await actions[action](
            feed_url=feed_url,
            feed_name=feed_name,
            limit=limit,
            **kwargs,
        )

    def _list_feeds(self, **kwargs: Any) -> dict:
        """List available RSS feeds."""
        return {
            "feeds": [
                {"name": name, "url": url}
                for name, url in self.DEFAULT_FEEDS.items()
            ]
        }

    async def _fetch_feed(
        self,
        feed_url: Optional[str] = None,
        feed_name: Optional[str] = None,
        limit: int = 10,
        **kwargs: Any,
    ) -> list[dict]:
        """Fetch items from an RSS feed.

        Note: This is a stub implementation. In a full implementation,
        you would use a library like feedparser to fetch and parse RSS feeds.
        """
        # Resolve feed URL
        if feed_name and feed_name in self.DEFAULT_FEEDS:
            feed_url = self.DEFAULT_FEEDS[feed_name]

        if not feed_url:
            raise SkillError("Either feed_url or feed_name must be provided", self.name)

        # Stub implementation - return placeholder
        # In production, use: import feedparser; feed = feedparser.parse(feed_url)
        logger.warning("RSS fetching requires feedparser library - install with: pip install feedparser")

        return [
            {
                "title": f"[RSS fetch requires feedparser] Feed: {feed_url}",
                "link": "#",
                "published": datetime.now(timezone.utc).isoformat(),
                "summary": "Install feedparser to enable RSS fetching: pip install feedparser",
                "source": feed_url,
            }
        ]

    async def _summarize_feed(
        self,
        feed_url: Optional[str] = None,
        feed_name: Optional[str] = None,
        limit: int = 5,
        **kwargs: Any,
    ) -> dict:
        """Fetch and summarize an RSS feed."""
        items = await self._fetch_feed(
            feed_url=feed_url,
            feed_name=feed_name,
            limit=limit,
        )

        # Generate summary
        titles = [item["title"] for item in items[:limit]]

        return {
            "feed": feed_url or feed_name,
            "items_count": len(items),
            "top_stories": titles,
            "fetched_at": datetime.now(timezone.utc).isoformat(),
        }

    def get_usage(self) -> str:
        """Return usage instructions."""
        return """
News Skill - 新闻摘要

可用操作:
- list_feeds: 列出预定义的 RSS 源
- fetch <feed_url|feed_name> [limit]: 获取新闻
- summarize <feed_url|feed_name> [limit]: 总结新闻

预定义的源:
- techcrunch: TechCrunch 科技新闻
- hackernews: Hacker News
- reddit_python: Reddit Python 社区

示例:
- 列出源：execute(action="list_feeds")
- 获取新闻：execute(action="fetch", feed_name="hackernews", limit=10)
- 总结新闻：execute(action="summarize", feed_url="https://example.com/rss")

注意：需要安装 feedparser 库：pip install feedparser
"""
