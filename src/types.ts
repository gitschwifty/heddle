import { type Static, Type } from "@sinclair/typebox";

// ── Message Types (OpenAI Chat Completions format) ──────────────────────

export const SystemMessage = Type.Object({
	role: Type.Literal("system"),
	content: Type.String(),
});
export type SystemMessage = Static<typeof SystemMessage>;

export const UserMessage = Type.Object({
	role: Type.Literal("user"),
	content: Type.String(),
});
export type UserMessage = Static<typeof UserMessage>;

export const FunctionCall = Type.Object({
	name: Type.String(),
	arguments: Type.String(),
});
export type FunctionCall = Static<typeof FunctionCall>;

export const ToolCall = Type.Object({
	id: Type.String(),
	type: Type.Literal("function"),
	function: FunctionCall,
});
export type ToolCall = Static<typeof ToolCall>;

export const AssistantMessage = Type.Object({
	role: Type.Literal("assistant"),
	content: Type.Union([Type.String(), Type.Null()]),
	tool_calls: Type.Optional(Type.Array(ToolCall)),
});
export type AssistantMessage = Static<typeof AssistantMessage>;

export const ToolMessage = Type.Object({
	role: Type.Literal("tool"),
	tool_call_id: Type.String(),
	content: Type.String(),
});
export type ToolMessage = Static<typeof ToolMessage>;

export const Message = Type.Union([SystemMessage, UserMessage, AssistantMessage, ToolMessage]);
export type Message = Static<typeof Message>;

// ── Tool Definition (OpenAI format) ─────────────────────────────────────

export const ToolFunction = Type.Object({
	name: Type.String(),
	description: Type.String(),
	parameters: Type.Any(),
});
export type ToolFunction = Static<typeof ToolFunction>;

export const ToolDefinition = Type.Object({
	type: Type.Literal("function"),
	function: ToolFunction,
});
export type ToolDefinition = Static<typeof ToolDefinition>;

// ── API Response Types ──────────────────────────────────────────────────

export const Choice = Type.Object({
	index: Type.Number(),
	message: Type.Object({
		role: Type.Literal("assistant"),
		content: Type.Union([Type.String(), Type.Null()]),
		tool_calls: Type.Optional(Type.Array(ToolCall)),
	}),
	finish_reason: Type.Union([Type.String(), Type.Null()]),
});
export type Choice = Static<typeof Choice>;

export const Usage = Type.Object({
	prompt_tokens: Type.Number(),
	completion_tokens: Type.Number(),
	total_tokens: Type.Number(),
	// OpenRouter-specific (optional for other future providers)
	cost: Type.Optional(Type.Number()),
	prompt_tokens_details: Type.Optional(
		Type.Object({
			cached_tokens: Type.Optional(Type.Number()),
			cache_write_tokens: Type.Optional(Type.Number()),
		}),
	),
	completion_tokens_details: Type.Optional(
		Type.Object({
			reasoning_tokens: Type.Optional(Type.Number()),
		}),
	),
});
export type Usage = Static<typeof Usage>;

export const ChatCompletionResponse = Type.Object({
	id: Type.String(),
	choices: Type.Array(Choice),
	usage: Type.Optional(Usage),
});
export type ChatCompletionResponse = Static<typeof ChatCompletionResponse>;

// ── Streaming Delta Types ───────────────────────────────────────────────

export const Delta = Type.Object({
	role: Type.Optional(Type.String()),
	content: Type.Optional(Type.Union([Type.String(), Type.Null()])),
	tool_calls: Type.Optional(
		Type.Array(
			Type.Object({
				index: Type.Number(),
				id: Type.Optional(Type.String()),
				type: Type.Optional(Type.Literal("function")),
				function: Type.Optional(
					Type.Object({
						name: Type.Optional(Type.String()),
						arguments: Type.Optional(Type.String()),
					}),
				),
			}),
		),
	),
});
export type Delta = Static<typeof Delta>;

export const StreamChoice = Type.Object({
	index: Type.Number(),
	delta: Delta,
	finish_reason: Type.Union([Type.String(), Type.Null()]),
});
export type StreamChoice = Static<typeof StreamChoice>;

export const StreamChunk = Type.Object({
	id: Type.String(),
	choices: Type.Array(StreamChoice),
	usage: Type.Optional(Usage),
});
export type StreamChunk = Static<typeof StreamChunk>;
