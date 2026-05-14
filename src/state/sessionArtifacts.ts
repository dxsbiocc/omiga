import { invoke } from "@tauri-apps/api/core";

export interface ArtifactEntry {
  path: string;
  operation: "write" | "edit";
  ts: string;
}

export async function fetchSessionArtifacts(
  sessionId: string
): Promise<ArtifactEntry[]> {
  return invoke<ArtifactEntry[]>("get_session_artifacts", { sessionId });
}
