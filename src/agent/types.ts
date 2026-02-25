import type { AssistantMessage, ToolCall, Usage } from "../types.ts";

export type AgentEvent =
	| { type: "assistant_message"; message: AssistantMessage }
	| { type: "content_delta"; text: string }
	| { type: "tool_start"; name: string; call: ToolCall }
	| { type: "tool_end"; name: string; result: string; call: ToolCall }
	| { type: "usage"; usage: Usage }
	| { type: "loop_detected"; count: number }
	| { type: "error"; error: Error }
	| { type: "permission_request"; name: string; call: ToolCall; reason?: string }
	| { type: "permission_denied"; name: string; call: ToolCall; reason: string }
	| { type: "plan_complete"; plan: string };
