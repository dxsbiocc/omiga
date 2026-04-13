/**
 * Agent Panel - 后台 Agent 任务管理面板
 * 
 * 显示所有后台运行的 Agent 任务，支持查看状态、结果和取消任务。
 */

import { useEffect, useMemo } from "react";
import {
  useAgentStore,
  getAgentTypeDisplayName,
  STATUS_COLORS,
  STATUS_ICONS,
  STATUS_LABELS,
  type BackgroundAgentTask,
} from "../../state/agentStore";
import { useSessionStore } from "../../state/sessionStore";
import { X, Trash2, RefreshCw, ExternalLink } from "lucide-react";
import { formatDistanceToNow } from "date-fns";
import { zhCN, enUS } from "date-fns/locale";
import { useLocaleStore } from "../../state/localeStore";

export function AgentPanel() {
  const { showTaskPanel, initEventListeners } = useAgentStore();

  // 初始化事件监听
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    initEventListeners().then((fn) => {
      cleanup = fn;
    });
    return () => cleanup?.();
  }, [initEventListeners]);

  if (!showTaskPanel) return null;

  return (
    <div className="fixed right-0 top-16 w-96 max-w-[calc(100vw-2rem)] h-[calc(100vh-4rem)] bg-white dark:bg-gray-900 border-l border-gray-200 dark:border-gray-700 shadow-xl z-50 flex flex-col">
      <AgentPanelHeader />
      <AgentTaskList />
    </div>
  );
}

function AgentPanelHeader() {
  const { setTaskPanelVisible, backgroundTasks } = useAgentStore();
  const hasCompleted = backgroundTasks.some(
    (t) => t.status === "completed" || t.status === "failed" || t.status === "cancelled"
  );

  return (
    <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
      <div>
        <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
          后台 Agent 任务
        </h2>
        <TaskCountBadge />
      </div>
      <div className="flex items-center gap-2">
        {hasCompleted && (
          <button
            onClick={() => useAgentStore.getState().cleanupCompleted()}
            className="p-2 text-gray-500 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
            title="清理已完成任务"
          >
            <Trash2 size={18} />
          </button>
        )}
        <button
          onClick={() => setTaskPanelVisible(false)}
          className="p-2 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
        >
          <X size={20} />
        </button>
      </div>
    </div>
  );
}

function TaskCountBadge() {
  const { backgroundTasks, getRunningTasks } = useAgentStore();
  const runningCount = getRunningTasks().length;
  const totalCount = backgroundTasks.length;

  if (totalCount === 0) {
    return (
      <span className="text-xs text-gray-500">暂无任务</span>
    );
  }

  return (
    <span className="text-xs text-gray-500">
      {runningCount > 0 && (
        <span className="text-blue-500 font-medium">{runningCount} 运行中</span>
      )}
      {runningCount > 0 && totalCount > runningCount && " / "}
      {totalCount > runningCount && (
        <span>{totalCount - runningCount} 已完成</span>
      )}
    </span>
  );
}

function AgentTaskList() {
  const { backgroundTasks, selectedTaskId, setSelectedTask } = useAgentStore();
  const { currentSession } = useSessionStore();
  const currentSessionId = currentSession?.id ?? null;

  // 按会话筛选并排序（最新的在前）
  const tasks = useMemo(() => {
    return backgroundTasks
      .filter((t) => t.sessionId === currentSessionId)
      .sort((a, b) => b.createdAt - a.createdAt);
  }, [backgroundTasks, currentSessionId]);

  if (tasks.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-gray-500 p-8">
        <RefreshCw size={48} className="mb-4 opacity-20" />
        <p className="text-center">
          暂无后台 Agent 任务
          <br />
          <span className="text-sm opacity-70">
            使用 verification 类型的 Agent 将在后台运行
          </span>
        </p>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto">
      {tasks.map((task) => (
        <TaskItem
          key={task.taskId}
          task={task}
          isSelected={task.taskId === selectedTaskId}
          onClick={() => setSelectedTask(task.taskId)}
        />
      ))}
    </div>
  );
}

interface TaskItemProps {
  task: BackgroundAgentTask;
  isSelected: boolean;
  onClick: () => void;
}

function TaskItem({ task, isSelected, onClick }: TaskItemProps) {
  const { locale } = useLocaleStore();
  const dateLocale = locale === "zh-CN" ? zhCN : enUS;

  const timeText = useMemo(() => {
    if (task.completedAt) {
      return formatDistanceToNow(task.completedAt, {
        addSuffix: true,
        locale: dateLocale,
      });
    }
    return formatDistanceToNow(task.createdAt, {
      addSuffix: true,
      locale: dateLocale,
    });
  }, [task.completedAt, task.createdAt, dateLocale]);

  return (
    <div
      onClick={onClick}
      className={`
        p-4 border-b border-gray-100 dark:border-gray-800 cursor-pointer
        transition-colors hover:bg-gray-50 dark:hover:bg-gray-800/50
        ${isSelected ? "bg-blue-50 dark:bg-blue-900/20 border-l-4 border-l-blue-500" : "border-l-4 border-l-transparent"}
      `}
    >
      <div className="flex items-start gap-3">
        <span className="text-lg" title={STATUS_LABELS[task.status]}>
          {STATUS_ICONS[task.status]}
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium px-2 py-0.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400">
              {getAgentTypeDisplayName(task.agentType)}
            </span>
            <span className={`text-xs ${STATUS_COLORS[task.status]}`}>
              {STATUS_LABELS[task.status]}
            </span>
          </div>
          <p className="mt-1 text-sm text-gray-900 dark:text-gray-100 font-medium truncate">
            {task.description}
          </p>
          <p className="text-xs text-gray-500 mt-1">{timeText}</p>
          
          {isSelected && <TaskDetails task={task} />}
        </div>
      </div>
    </div>
  );
}

function TaskDetails({ task }: { task: BackgroundAgentTask }) {
  const { removeTask } = useAgentStore();

  return (
    <div className="mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
      <div className="space-y-2">
        <div className="text-xs text-gray-500">
          <span className="font-medium">任务 ID:</span> {task.taskId.slice(0, 8)}...
        </div>
        
        {task.resultSummary && (
          <div className="text-sm text-gray-700 dark:text-gray-300 bg-gray-50 dark:bg-gray-800 p-2 rounded max-h-32 overflow-y-auto">
            <pre className="whitespace-pre-wrap break-words text-xs">
              {task.resultSummary.slice(0, 500)}
              {task.resultSummary.length > 500 && "..."}
            </pre>
          </div>
        )}
        
        {task.errorMessage && (
          <div className="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 p-2 rounded">
            {task.errorMessage}
          </div>
        )}
        
        <div className="flex items-center gap-2 pt-2">
          {task.outputPath && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                // TODO: 打开输出文件
              }}
              className="flex items-center gap-1 text-xs text-blue-500 hover:text-blue-600 px-2 py-1 rounded hover:bg-blue-50 dark:hover:bg-blue-900/20 transition-colors"
            >
              <ExternalLink size={12} />
              查看完整输出
            </button>
          )}
          
          {(task.status === "completed" || task.status === "failed" || task.status === "cancelled") && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                removeTask(task.taskId);
              }}
              className="flex items-center gap-1 text-xs text-red-500 hover:text-red-600 px-2 py-1 rounded hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
            >
              <Trash2 size={12} />
              删除
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/** Agent 面板触发按钮 */
export function AgentPanelButton() {
  const { showTaskPanel, toggleTaskPanel, getRunningTasks, backgroundTasks } = useAgentStore();
  const runningCount = getRunningTasks().length;
  const totalCount = backgroundTasks.length;

  return (
    <button
      onClick={toggleTaskPanel}
      className={`
        relative flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium
        transition-colors
        ${showTaskPanel
          ? "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300"
          : "bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-700"
        }
      `}
    >
      <RefreshCw size={16} className={runningCount > 0 ? "animate-spin" : ""} />
      <span>Agent 任务</span>
      {totalCount > 0 && (
        <span className={`
          min-w-[1.25rem] h-5 px-1 rounded-full text-xs flex items-center justify-center
          ${runningCount > 0
            ? "bg-blue-500 text-white"
            : "bg-gray-400 text-white dark:bg-gray-600"
          }
        `}>
          {totalCount}
        </span>
      )}
    </button>
  );
}
