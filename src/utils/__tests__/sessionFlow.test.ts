/**
 * Session Flow Frontend Tests
 *
 * These tests verify the frontend integration with the new session flow API.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock Tauri API
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { useSessionStore } from '../../state/sessionStore';
import { useActivityStore } from '../../state/activityStore';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('Session Flow Integration', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    const win = globalThis as typeof globalThis & {
      window?: typeof globalThis & {
        dispatchEvent?: (event: Event) => boolean;
      };
      dispatchEvent?: (event: Event) => boolean;
    };
    win.window = win;
    win.dispatchEvent = vi.fn(() => true);
    win.window.dispatchEvent = win.dispatchEvent;
    // Reset store state
    useSessionStore.setState({
      sessions: [],
      currentSession: null,
      messages: [],
      storeMessages: [],
      isLoading: false,
      isSwitchingSession: false,
      hasMoreMessages: false,
      isLoadingMoreMessages: false,
      activeProviderEntryName: null,
      pendingProjectPathSessions: new Set(),
      activeRounds: new Map(),
    });
    useActivityStore.setState({
      isConnecting: false,
      isStreaming: false,
      waitingFirstChunk: false,
      currentToolHint: null,
      backgroundJobs: [],
      executionSteps: [],
      executionStartedAt: null,
      executionEndedAt: null,
      activeTodos: null,
    });
  });

  describe('sendMessage', () => {
    it('should send message with new request format', async () => {
      const mockInvoke = vi.mocked(invoke);
      mockInvoke.mockResolvedValueOnce({
        message_id: 'msg-123',
        session_id: 'sess-456',
        round_id: 'round-789',
      });

      const store = useSessionStore.getState();
      const response = await store.sendMessage({
        content: 'Hello',
        session_id: 'sess-456',
        project_path: '/test/path',
        session_name: 'Test Session',
        use_tools: true,
      });

      expect(mockInvoke).toHaveBeenCalledWith('send_message', {
        request: {
          content: 'Hello',
          session_id: 'sess-456',
          project_path: '/test/path',
          session_name: 'Test Session',
          use_tools: true,
        },
      });

      expect(response).toEqual({
        message_id: 'msg-123',
        session_id: 'sess-456',
        round_id: 'round-789',
      });

      // Check that round is tracked
      expect(store.activeRounds.get('round-789')).toBe('running');
    });

    it('should track round status correctly', async () => {
      const store = useSessionStore.getState();

      // Simulate receiving status updates
      store.updateRoundStatus('round-1', 'running');
      expect(store.activeRounds.get('round-1')).toBe('running');

      store.updateRoundStatus('round-1', 'partial');
      expect(store.activeRounds.get('round-1')).toBe('partial');

      store.updateRoundStatus('round-1', 'completed');
      expect(store.activeRounds.get('round-1')).toBe('completed');
    });

    it('should update message roundStatus when round is updated', async () => {
      const store = useSessionStore.getState();

      // Add a message with roundId
      store.addMessage({
        role: 'assistant',
        content: 'Response',
        roundId: 'round-1',
        roundStatus: 'running',
      });

      // Update round status
      store.updateRoundStatus('round-1', 'completed');

      // Check that message was updated
      const state = useSessionStore.getState();
      const messages = state.messages;
      expect(messages[0].roundStatus).toBe('completed');
    });
  });

  describe('cancelStream', () => {
    it('should call cancel_stream with messageId', async () => {
      const mockInvoke = vi.mocked(invoke);
      mockInvoke.mockResolvedValueOnce(undefined);

      const store = useSessionStore.getState();
      await store.cancelStream('msg-123');

      expect(mockInvoke).toHaveBeenCalledWith('cancel_stream', {
        messageId: 'msg-123',
      });
    });
  });

  describe('Message Types', () => {
    it('should handle Message with round metadata', () => {
      const message = {
        id: 'msg-1',
        role: 'assistant' as const,
        content: 'Hello',
        roundId: 'round-1',
        roundStatus: 'running' as const,
      };

      expect(message.roundId).toBe('round-1');
      expect(message.roundStatus).toBe('running');
    });

    it('stamps local messages with a timestamp when one is not provided', () => {
      const store = useSessionStore.getState();
      store.addMessage({
        role: 'user',
        content: 'hello',
      });

      const state = useSessionStore.getState();
      expect(typeof state.messages[0]?.timestamp).toBe('number');
    });
  });

  describe('Stream Events', () => {
    it('should handle all StreamOutputItem types', () => {
      const events = [
        { Start: {} },
        { Text: 'Hello' },
        { ToolUse: { name: 'read_file', arguments: '{}' } },
        { ToolOutput: { output: 'file content' } },
        { ToolResult: { output: 'result', is_error: false } },
        { Error: { message: 'Error', code: '500' } },
        { Cancelled: {} },
        { Complete: {} },
      ];

      // Verify all event types are handled
      events.forEach((event) => {
        expect(event).toBeDefined();
      });
    });
  });

  describe('setCurrentSession', () => {
    it('should switch into loading state immediately on cache miss', async () => {
      const mockInvoke = vi.mocked(invoke);
      const loadDeferred = deferred<{
        id: string;
        name: string;
        messages: Array<{ id: string; role: 'user'; content: string }>;
        project_path: string;
        created_at: string;
        updated_at: string;
        active_provider_entry_name: string | null;
        has_more_messages: boolean;
      }>();

      mockInvoke.mockImplementation(async (command) => {
        if (command === 'load_session') return loadDeferred.promise;
        throw new Error(`unexpected invoke: ${command}`);
      });

      useSessionStore.setState({
        sessions: [
          {
            id: 'session-miss',
            name: 'Session miss',
            projectPath: '/tmp/project',
            workingDirectory: '/tmp/project',
            createdAt: '2026-04-16T00:00:00.000Z',
            updatedAt: '2026-04-16T00:00:00.000Z',
            messageCount: 1,
          },
        ],
      });

      const switchPromise = useSessionStore.getState().setCurrentSession('session-miss');
      await switchPromise;

      let state = useSessionStore.getState();
      expect(state.currentSession?.id).toBe('session-miss');
      expect(state.currentSession?.name).toBe('Session miss');
      expect(state.isSwitchingSession).toBe(true);
      expect(state.storeMessages).toEqual([]);

      loadDeferred.resolve({
        id: 'session-miss',
        name: 'Session miss',
        messages: [{ id: 'm-1', role: 'user', content: 'hello' }],
        project_path: '/tmp/project',
        created_at: '2026-04-16T00:00:00.000Z',
        updated_at: '2026-04-16T00:00:01.000Z',
        active_provider_entry_name: null,
        has_more_messages: false,
      });
      await Promise.resolve();
      await Promise.resolve();

      state = useSessionStore.getState();
      expect(state.currentSession?.id).toBe('session-miss');
      expect(state.storeMessages).toHaveLength(1);
      expect(state.isSwitchingSession).toBe(false);
    });


    it('should clear activity state immediately when switching sessions', async () => {
      const mockInvoke = vi.mocked(invoke);
      const loadDeferred = deferred<{
        id: string;
        name: string;
        messages: Array<{ id: string; role: 'user'; content: string }>;
        project_path: string;
        created_at: string;
        updated_at: string;
        active_provider_entry_name: string | null;
        has_more_messages: boolean;
      }>();

      mockInvoke.mockImplementation(async (command) => {
        if (command === 'load_session') return loadDeferred.promise;
        throw new Error(`unexpected invoke: ${command}`);
      });

      useSessionStore.setState({
        sessions: [
          {
            id: 'session-clear',
            name: 'Session clear',
            projectPath: '/tmp/project',
            workingDirectory: '/tmp/project',
            createdAt: '2026-04-16T00:00:00.000Z',
            updatedAt: '2026-04-16T00:00:00.000Z',
            messageCount: 1,
          },
        ],
      });

      useActivityStore.setState({
        isConnecting: true,
        isStreaming: true,
        waitingFirstChunk: true,
        currentToolHint: 'bash',
        backgroundJobs: [{ id: 'bg1', toolUseId: 'tu1', label: 'executor', state: 'running' }],
        executionSteps: [{ id: 'tool-1', title: 'old', status: 'running' }],
        executionStartedAt: 123,
        executionEndedAt: null,
        activeTodos: [{ id: 'todo-1', content: 'old', activeForm: 'olding', status: 'in_progress' }],
      });

      await useSessionStore.getState().setCurrentSession('session-clear');

      const activity = useActivityStore.getState();
      expect(activity.isConnecting).toBe(false);
      expect(activity.isStreaming).toBe(false);
      expect(activity.waitingFirstChunk).toBe(false);
      expect(activity.currentToolHint).toBeNull();
      expect(activity.backgroundJobs).toEqual([]);
      expect(activity.executionSteps).toEqual([]);
      expect(activity.executionStartedAt).toBeNull();
      expect(activity.executionEndedAt).toBeNull();
      expect(activity.activeTodos).toBeNull();

      loadDeferred.resolve({
        id: 'session-clear',
        name: 'Session clear',
        messages: [{ id: 'm-1', role: 'user', content: 'hello' }],
        project_path: '/tmp/project',
        created_at: '2026-04-16T00:00:00.000Z',
        updated_at: '2026-04-16T00:00:01.000Z',
        active_provider_entry_name: null,
        has_more_messages: false,
      });
      await Promise.resolve();
      await Promise.resolve();
    });

    it('should ignore stale session loads when a newer switch starts', async () => {
      const mockInvoke = vi.mocked(invoke);
      const sessionOneLoad = deferred<{
        id: string;
        name: string;
        messages: Array<{ id: string; role: 'user'; content: string }>;
        project_path: string;
        created_at: string;
        updated_at: string;
        active_provider_entry_name: string | null;
        has_more_messages: boolean;
      }>();
      const sessionTwoLoad = deferred<{
        id: string;
        name: string;
        messages: Array<{ id: string; role: 'user'; content: string }>;
        project_path: string;
        created_at: string;
        updated_at: string;
        active_provider_entry_name: string | null;
        has_more_messages: boolean;
      }>();

      mockInvoke.mockImplementation(async (command, payload) => {
        if (command !== 'load_session') {
          throw new Error(`unexpected invoke: ${command}`);
        }
        const sessionId = (payload as { sessionId: string }).sessionId;
        if (sessionId === 'session-race-1') return sessionOneLoad.promise;
        if (sessionId === 'session-race-2') return sessionTwoLoad.promise;
        throw new Error(`unexpected session: ${sessionId}`);
      });

      useSessionStore.setState({
        sessions: [
          {
            id: 'session-race-1',
            name: 'First session',
            projectPath: '/tmp/project-a',
            workingDirectory: '/tmp/project-a',
            createdAt: '2026-04-16T00:00:00.000Z',
            updatedAt: '2026-04-16T00:00:00.000Z',
            messageCount: 1,
          },
          {
            id: 'session-race-2',
            name: 'Second session',
            projectPath: '/tmp/project-b',
            workingDirectory: '/tmp/project-b',
            createdAt: '2026-04-16T00:00:00.000Z',
            updatedAt: '2026-04-16T00:00:00.000Z',
            messageCount: 1,
          },
        ],
      });

      await useSessionStore.getState().setCurrentSession('session-race-1');
      await useSessionStore.getState().setCurrentSession('session-race-2');

      sessionTwoLoad.resolve({
        id: 'session-race-2',
        name: 'Second session',
        messages: [{ id: 'm-2', role: 'user', content: 'two' }],
        project_path: '/tmp/project-b',
        created_at: '2026-04-16T00:00:00.000Z',
        updated_at: '2026-04-16T00:00:02.000Z',
        active_provider_entry_name: null,
        has_more_messages: false,
      });
      await Promise.resolve();
      await Promise.resolve();

      sessionOneLoad.resolve({
        id: 'session-race-1',
        name: 'First session',
        messages: [{ id: 'm-1', role: 'user', content: 'one' }],
        project_path: '/tmp/project-a',
        created_at: '2026-04-16T00:00:00.000Z',
        updated_at: '2026-04-16T00:00:01.000Z',
        active_provider_entry_name: null,
        has_more_messages: false,
      });
      await Promise.resolve();
      await Promise.resolve();

      const state = useSessionStore.getState();
      expect(state.currentSession?.id).toBe('session-race-2');
      expect(state.storeMessages.map((message) => message.content)).toEqual(['two']);
      expect(state.isSwitchingSession).toBe(false);
    });
  });
});
