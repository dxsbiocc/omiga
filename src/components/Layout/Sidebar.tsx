import { FolderTree, Terminal, History, Settings, Plus } from "lucide-react";
import { useSessionStore } from "../../state/sessionStore";

interface SidebarProps {
  activePanel: "files" | "terminal" | "sessions" | null;
  onPanelChange: (panel: "files" | "terminal" | "sessions" | null) => void;
  onSettingsClick: () => void;
}

export function Sidebar({ activePanel, onPanelChange, onSettingsClick }: SidebarProps) {
  const { createSessionQuick } = useSessionStore();

  const togglePanel = (panel: "files" | "terminal" | "sessions") => {
    onPanelChange(activePanel === panel ? null : panel);
  };

  const handleNewSession = () => {
    void createSessionQuick().then(() => onPanelChange("sessions"));
  };

  return (
    <div className="w-12 bg-card border-r border-border flex flex-col items-center py-2">
      {/* New Session */}
      <button
        onClick={handleNewSession}
        className="p-2 rounded-lg hover:bg-accent mb-4"
        title="New Session"
      >
        <Plus className="w-5 h-5" />
      </button>

      <div className="w-8 h-px bg-border mb-4" />

      {/* Navigation */}
      <button
        onClick={() => togglePanel("sessions")}
        className={`p-2 rounded-lg mb-2 ${
          activePanel === "sessions" ? "bg-accent" : "hover:bg-accent"
        }`}
        title="Session History"
      >
        <History className="w-5 h-5" />
      </button>

      <button
        onClick={() => togglePanel("files")}
        className={`p-2 rounded-lg mb-2 ${
          activePanel === "files" ? "bg-accent" : "hover:bg-accent"
        }`}
        title="Files"
      >
        <FolderTree className="w-5 h-5" />
      </button>

      <button
        onClick={() => togglePanel("terminal")}
        className={`p-2 rounded-lg mb-2 ${
          activePanel === "terminal" ? "bg-accent" : "hover:bg-accent"
        }`}
        title="Terminal"
      >
        <Terminal className="w-5 h-5" />
      </button>

      <div className="flex-1" />

      {/* Bottom */}
      <button
        onClick={onSettingsClick}
        className="p-2 rounded-lg hover:bg-accent mb-2"
        title="Settings"
      >
        <Settings className="w-5 h-5" />
      </button>
    </div>
  );
}
