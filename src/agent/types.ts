import type { AssistantMessage, ToolCall } from "../types.ts";

export type AgentEvent =
	| { type: "assistant_message"; message: AssistantMessage }
	| { type: "content_delta"; text: string }
	| { type: "tool_start"; name: string; call: ToolCall }
	| { type: "tool_end"; name: string; result: string; call: ToolCall }
	| { type: "loop_detected"; count: number }
	| { type: "error"; error: Error };
