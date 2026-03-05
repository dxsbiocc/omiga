"""Expert Agents for Omiga.

This module provides specialized expert agents for different domains:
- BrowserExpert: Browser automation expert
- CodingExpert: Code writing and analysis expert
- AnalysisExpert: Data analysis expert

Each expert inherits from ToolCallAgent and has domain-specific tools.
"""
from __future__ import annotations

import logging
from typing import Any, Dict, List, Optional, Callable, Awaitable

from pydantic import Field

from omiga.agent.toolcall import ToolCallAgent
from omiga.tools.registry import ToolRegistry

logger = logging.getLogger("omiga.agent.experts")


class BrowserExpert(ToolCallAgent):
    """Browser automation expert agent.

    This agent specializes in web automation tasks:
    - Web scraping and data extraction
    - Form filling and submission
    - Navigation and interaction
    - Screenshot capture

    Attributes:
        browser_config: Browser configuration options
        headless: Run browser in headless mode
    """

    name: str = "browser_expert"
    description: str = "Expert in browser automation and web scraping"

    browser_config: Dict[str, Any] = Field(default_factory=dict)
    headless: bool = True

    def __init__(
        self,
        headless: bool = True,
        browser_config: Optional[Dict[str, Any]] = None,
        on_tool_update: Optional[Callable[[str], Awaitable[None]]] = None,
        **kwargs: Any,
    ):
        """Initialize browser expert.

        Args:
            headless: Run in headless mode
            browser_config: Browser configuration
            on_tool_update: Callback for tool execution updates
            **kwargs: Additional arguments for ToolCallAgent
        """
        super().__init__(
            headless=headless,
            browser_config=browser_config or {},
            on_tool_update=on_tool_update,
            **kwargs,
        )

        # Register browser-specific tools
        self._register_browser_tools()

    def _register_browser_tools(self) -> None:
        """Register browser automation tools."""
        # Import browser tools here to avoid circular imports
        try:
            from omiga.tools.browser import (
                browser_navigate,
                browser_click,
                browser_fill,
                browser_screenshot,
                browser_get_text,
            )

            # Register tools
            self.tool_registry.register(browser_navigate)
            self.tool_registry.register(browser_click)
            self.tool_registry.register(browser_fill)
            self.tool_registry.register(browser_screenshot)
            self.tool_registry.register(browser_get_text)

            logger.info("Registered browser automation tools")

        except ImportError:
            logger.warning(
                "Browser tools not available - install playwright dependency"
            )

    async def _think_impl(self) -> bool:
        """Decide which browser action to take.

        Returns:
            True if browser action decided, False otherwise
        """
        # Placeholder implementation
        # Will analyze current state and decide browser action
        self.pending_tool_calls = []
        return False

    def get_capabilities(self) -> List[str]:
        """Get list of browser expert capabilities.

        Returns:
            List of capability descriptions
        """
        return [
            "Navigate to URLs",
            "Click elements",
            "Fill form fields",
            "Capture screenshots",
            "Extract text content",
        ]


class CodingExpert(ToolCallAgent):
    """Code writing and analysis expert agent.

    This agent specializes in software development tasks:
    - Code generation
    - Code review
    - Refactoring
    - Debugging
    - Test generation

    Attributes:
        language: Primary programming language
        style_guide: Coding style preferences
    """

    name: str = "coding_expert"
    description: str = "Expert in code writing, analysis, and review"

    language: str = "python"
    style_guide: Dict[str, Any] = Field(default_factory=dict)

    def __init__(
        self,
        language: str = "python",
        style_guide: Optional[Dict[str, Any]] = None,
        on_tool_update: Optional[Callable[[str], Awaitable[None]]] = None,
        **kwargs: Any,
    ):
        """Initialize coding expert.

        Args:
            language: Primary programming language
            style_guide: Coding style configuration
            on_tool_update: Callback for tool execution updates
            **kwargs: Additional arguments for ToolCallAgent
        """
        super().__init__(
            language=language,
            style_guide=style_guide or {},
            on_tool_update=on_tool_update,
            **kwargs,
        )

        # Register coding-specific tools
        self._register_coding_tools()

    def _register_coding_tools(self) -> None:
        """Register code analysis and generation tools."""
        # Import coding tools here to avoid circular imports
        try:
            from omiga.tools.code import (
                code_format,
                code_lint,
                code_analyze,
                code_generate,
                code_refactor,
            )

            # Register tools
            self.tool_registry.register(code_format)
            self.tool_registry.register(code_lint)
            self.tool_registry.register(code_analyze)
            self.tool_registry.register(code_generate)
            self.tool_registry.register(code_refactor)

            logger.info("Registered coding tools")

        except ImportError:
            logger.warning("Coding tools not fully configured")

    async def _think_impl(self) -> bool:
        """Decide which coding action to take.

        Returns:
            True if coding action decided, False otherwise
        """
        # Placeholder implementation
        # Will analyze code context and decide action
        self.pending_tool_calls = []
        return False

    def get_capabilities(self) -> List[str]:
        """Get list of coding expert capabilities.

        Returns:
            List of capability descriptions
        """
        return [
            "Generate code from specifications",
            "Review code for quality and security",
            "Refactor code for maintainability",
            "Generate unit tests",
            "Fix bugs and errors",
        ]


class AnalysisExpert(ToolCallAgent):
    """Data analysis expert agent.

    This agent specializes in data processing and analysis:
    - Data loading and cleaning
    - Statistical analysis
    - Visualization generation
    - Report creation

    Attributes:
        data_format: Supported data formats
        visualization_backend: Plotting backend preference
    """

    name: str = "analysis_expert"
    description: str = "Expert in data analysis and visualization"

    data_format: List[str] = Field(default_factory=lambda: ["csv", "json", "xlsx"])
    visualization_backend: str = "matplotlib"

    def __init__(
        self,
        data_format: Optional[List[str]] = None,
        visualization_backend: str = "matplotlib",
        on_tool_update: Optional[Callable[[str], Awaitable[None]]] = None,
        **kwargs: Any,
    ):
        """Initialize analysis expert.

        Args:
            data_format: Supported data formats
            visualization_backend: Plotting backend
            on_tool_update: Callback for tool execution updates
            **kwargs: Additional arguments for ToolCallAgent
        """
        super().__init__(
            data_format=data_format or ["csv", "json", "xlsx"],
            visualization_backend=visualization_backend,
            on_tool_update=on_tool_update,
            **kwargs,
        )

        # Register analysis-specific tools
        self._register_analysis_tools()

    def _register_analysis_tools(self) -> None:
        """Register data analysis tools."""
        # Import analysis tools here to avoid circular imports
        try:
            from omiga.tools.analysis import (
                data_load,
                data_clean,
                data_analyze,
                data_visualize,
                data_export,
            )

            # Register tools
            self.tool_registry.register(data_load)
            self.tool_registry.register(data_clean)
            self.tool_registry.register(data_analyze)
            self.tool_registry.register(data_visualize)
            self.tool_registry.register(data_export)

            logger.info("Registered analysis tools")

        except ImportError:
            logger.warning("Analysis tools not fully configured")

    async def _think_impl(self) -> bool:
        """Decide which analysis action to take.

        Returns:
            True if analysis action decided, False otherwise
        """
        # Placeholder implementation
        # Will analyze data context and decide action
        self.pending_tool_calls = []
        return False

    def get_capabilities(self) -> List[str]:
        """Get list of analysis expert capabilities.

        Returns:
            List of capability descriptions
        """
        return [
            "Load and parse data files",
            "Clean and preprocess data",
            "Perform statistical analysis",
            "Generate visualizations",
            "Export analysis reports",
        ]


# Factory function for creating expert agents
def create_expert(
    expert_type: str,
    on_tool_update: Optional[Callable[[str], Awaitable[None]]] = None,
    **kwargs: Any,
) -> ToolCallAgent:
    """Create an expert agent by type.

    Args:
        expert_type: Type of expert ("browser", "coding", "analysis")
        on_tool_update: Callback for tool execution updates
        **kwargs: Additional arguments passed to expert constructor

    Returns:
        Configured expert agent instance

    Raises:
        ValueError: If expert_type is not recognized
    """
    experts = {
        "browser": BrowserExpert,
        "coding": CodingExpert,
        "analysis": AnalysisExpert,
    }

    if expert_type.lower() not in experts:
        available = ", ".join(experts.keys())
        raise ValueError(
            f"Unknown expert type: {expert_type}. Available: {available}"
        )

    expert_class = experts[expert_type.lower()]
    return expert_class(on_tool_update=on_tool_update, **kwargs)  # type: ignore[call-arg]
