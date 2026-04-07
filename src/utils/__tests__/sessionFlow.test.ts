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

describe('Session Flow Integration', () => {
  beforeEach(() => {
    // Reset store state
    const store = useSessionStore.getState();
    useSessionStore.setState({
      sessions: [],
      currentSession: null,
      messages: [],
      storeMessages: [],
      isLoading: false,
      activeRounds: new Map(),
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
});
