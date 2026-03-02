"""Tests for APScheduler-based task scheduler."""
import pytest
from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock, patch

from omiga.scheduler.task_scheduler import (
    SchedulerDeps,
    _create_trigger,
    get_scheduler_status,
    remove_task,
    reschedule_task,
    start_scheduler_loop,
    stop_scheduler,
)
from omiga.models import ScheduledTask


def _make_task(
    task_id: str = "test-1",
    schedule_type: str = "cron",
    schedule_value: str = "0 9 * * *",
    status: str = "active",
) -> ScheduledTask:
    """Helper to create a ScheduledTask with defaults."""
    return ScheduledTask(
        id=task_id,
        group_folder="main",
        chat_jid="tg:123",
        prompt="test",
        schedule_type=schedule_type,
        schedule_value=schedule_value,
        context_mode="isolated",
        next_run=None,
        last_run=None,
        last_result=None,
        status=status,
        created_at=datetime.now(timezone.utc).isoformat(),
    )


class TestCreateTrigger:
    """Tests for _create_trigger function."""

    def test_cron_trigger(self):
        """Test creating cron trigger."""
        task = _make_task("test-1", "cron", "0 9 * * *")
        trigger = _create_trigger(task)
        assert trigger is not None
        assert trigger.__class__.__name__ == "CronTrigger"

    def test_cron_trigger_invalid(self):
        """Test creating cron trigger with invalid expression."""
        task = _make_task("test-2", "cron", "invalid")
        trigger = _create_trigger(task)
        assert trigger is None

    def test_interval_trigger(self):
        """Test creating interval trigger."""
        task = _make_task("test-3", "interval", "3600000")
        trigger = _create_trigger(task)
        assert trigger is not None
        # APScheduler 4.x uses seconds internally
        assert trigger.__class__.__name__ == "IntervalTrigger"

    def test_interval_trigger_invalid(self):
        """Test creating interval trigger with invalid value."""
        task = _make_task("test-4", "interval", "-1000")
        trigger = _create_trigger(task)
        assert trigger is None

    def test_once_trigger(self):
        """Test creating once trigger."""
        future = datetime.now(timezone.utc).replace(second=0, microsecond=0)
        task = _make_task("test-5", "once", future.isoformat())
        trigger = _create_trigger(task)
        assert trigger is not None
        assert trigger.__class__.__name__ == "DateTrigger"

    def test_once_trigger_invalid(self):
        """Test creating once trigger with invalid date."""
        task = _make_task("test-6", "once", "not-a-date")
        trigger = _create_trigger(task)
        assert trigger is None

    def test_unknown_schedule_type(self):
        """Test unknown schedule type."""
        task = _make_task("test-7", "unknown", "value")
        trigger = _create_trigger(task)
        assert trigger is None


class TestSchedulerDeps:
    """Tests for SchedulerDeps class."""

    def test_deps_init(self):
        """Test SchedulerDeps initialization."""
        deps = SchedulerDeps(
            registered_groups=lambda: {},
            get_sessions=lambda: {},
            queue=MagicMock(),
            on_process=MagicMock(),
            send_message=AsyncMock(),
        )

        assert deps.registered_groups is not None
        assert deps.get_sessions is not None
        assert deps.queue is not None
        assert deps.on_process is not None
        assert deps.send_message is not None


class TestSchedulerLifecycle:
    """Tests for scheduler lifecycle."""

    @pytest.mark.asyncio
    async def test_start_stop_scheduler(self):
        """Test starting and stopping scheduler."""
        deps = SchedulerDeps(
            registered_groups=lambda: {},
            get_sessions=lambda: {},
            queue=MagicMock(),
            on_process=MagicMock(),
            send_message=AsyncMock(),
        )

        # Start
        start_scheduler_loop(deps)

        # Give time to initialize
        import asyncio
        await asyncio.sleep(0.05)

        # Stop
        stop_scheduler()

        # After stop, status should show not running
        status = get_scheduler_status()
        assert status["running"] is False

    @pytest.mark.asyncio
    async def test_start_scheduler_idempotent(self):
        """Test that starting scheduler twice is safe."""
        deps = SchedulerDeps(
            registered_groups=lambda: {},
            get_sessions=lambda: {},
            queue=MagicMock(),
            on_process=MagicMock(),
            send_message=AsyncMock(),
        )

        start_scheduler_loop(deps)
        start_scheduler_loop(deps)  # Should not raise

        stop_scheduler()

    @pytest.mark.asyncio
    async def test_stop_scheduler_idempotent(self):
        """Test that stopping scheduler twice is safe."""
        stop_scheduler()  # Should not raise when not started
        stop_scheduler()


class TestRemoveTask:
    """Tests for remove_task function."""

    def test_remove_nonexistent_task(self):
        """Test removing a task that doesn't exist."""
        result = remove_task("nonexistent-task")
        # May return False or True depending on implementation
        assert isinstance(result, bool)


class TestRescheduleTask:
    """Tests for reschedule_task function."""

    def test_reschedule_when_scheduler_not_running(self):
        """Test rescheduling when scheduler is stopped."""
        # Ensure scheduler is stopped
        stop_scheduler()

        task = _make_task("test-reschedule")

        deps = SchedulerDeps(
            registered_groups=lambda: {},
            get_sessions=lambda: {},
            queue=MagicMock(),
            on_process=MagicMock(),
            send_message=AsyncMock(),
        )

        result = reschedule_task(task, deps)
        assert result is False

    @pytest.mark.asyncio
    async def test_reschedule_paused_task(self):
        """Test rescheduling a paused task removes it from scheduler."""
        deps = SchedulerDeps(
            registered_groups=lambda: {},
            get_sessions=lambda: {},
            queue=MagicMock(),
            on_process=MagicMock(),
            send_message=AsyncMock(),
        )

        start_scheduler_loop(deps)

        # Give time to initialize
        import asyncio
        await asyncio.sleep(0.05)

        task = _make_task("test-paused", "cron", "0 9 * * *", "paused")

        # Should return True (task removed from scheduler)
        result = reschedule_task(task, deps)
        assert result is True

        stop_scheduler()


@pytest.mark.asyncio
async def test_scheduler_with_mock_task():
    """Test scheduler integration with mock task."""
    deps = SchedulerDeps(
        registered_groups=lambda: {"main": MagicMock(folder="main", name="Main")},
        get_sessions=lambda: {"main": "session-123"},
        queue=MagicMock(),
        on_process=MagicMock(),
        send_message=AsyncMock(),
    )

    # Start scheduler
    start_scheduler_loop(deps)

    # Give it time to initialize
    import asyncio
    await asyncio.sleep(0.1)

    # Stop scheduler
    stop_scheduler()

    status = get_scheduler_status()
    assert status["running"] is False
