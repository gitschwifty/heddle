import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../types.ts";

export type RequestOverrides = Record<string, unknown>;

export interface RetryConfig {
	/** Max number of retries on 429 responses. Default: 3. */
	maxRetries?: number;
	/** Base delay in ms for exponential backoff. Default: 1000. */
	baseDelayMs?: number;
}

export interface ProviderConfig {
	apiKey: string;
	model: string;
	baseUrl?: string;
	/** Extra fields merged into every request body (e.g., OpenRouter-specific: reasoning, transforms). */
	requestParams?: Record<string, unknown>;
	/** Retry config for 429 rate limit responses. On by default (3 retries, 1s base delay). Set to false to disable. */
	retry?: RetryConfig | false;
}

export interface Provider {
	send(messages: Message[], tools?: ToolDefinition[], overrides?: RequestOverrides): Promise<ChatCompletionResponse>;
	stream(messages: Message[], tools?: ToolDefinition[], overrides?: RequestOverrides): AsyncGenerator<StreamChunk>;
	with(overrides: RequestOverrides): Provider;
}
