import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../types.ts";

export interface ProviderConfig {
	apiKey: string;
	model: string;
	baseUrl?: string;
}

export interface Provider {
	send(messages: Message[], tools?: ToolDefinition[]): Promise<ChatCompletionResponse>;
	stream(messages: Message[], tools?: ToolDefinition[]): AsyncGenerator<StreamChunk>;
}
