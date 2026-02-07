import type { AssistantMessage, ToolCall } from "../types.ts";

export type AgentEvent =
	| { type: "assistant_message"; message: AssistantMessage }
	| { type: "tool_start"; name: string; call: ToolCall }
	| { type: "tool_end"; name: string; result: string; call: ToolCall }
	| { type: "error"; error: Error };
