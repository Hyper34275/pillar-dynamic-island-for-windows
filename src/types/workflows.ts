export type WorkflowActionId = "toggle_expand";

export interface WorkflowActionEnvelope {
  id: WorkflowActionId;
  args?: Record<string, unknown>;
  source: "tray" | "shortcut" | "ui";
  timestamp: number;
}
