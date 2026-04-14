import { useChatComposerStore } from "../state/chatComposerStore";
import { useSessionStore } from "../state/sessionStore";

/** When composer is SSH + profile selected, workspace I/O uses `ssh_*` Tauri commands. */
export function getSshWorkspaceFileContext():
  | { mode: "ssh"; profile: string }
  | { mode: "local" } {
  const st = useChatComposerStore.getState();
  if (st.environment === "ssh" && st.sshServer?.trim()) {
    return { mode: "ssh", profile: st.sshServer.trim() };
  }
  return { mode: "local" };
}

/** Full workspace file context including sandbox support. */
export function getWorkspaceFileContext():
  | { mode: "local" }
  | { mode: "ssh"; profile: string }
  | { mode: "sandbox"; sessionId: string; backend: string } {
  const st = useChatComposerStore.getState();

  if (st.environment === "ssh" && st.sshServer?.trim()) {
    return { mode: "ssh", profile: st.sshServer.trim() };
  }

  if (st.environment === "sandbox" && st.sandboxBackend?.trim()) {
    const session = useSessionStore.getState().currentSession;
    const sessionId = session?.id ?? "";
    return { mode: "sandbox", sessionId, backend: st.sandboxBackend.trim() };
  }

  return { mode: "local" };
}
