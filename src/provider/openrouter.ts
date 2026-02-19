import { debug } from "../debug.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../types.ts";
import type { OpenRouterOverrides } from "./overrides.ts";
import { validateOverrides } from "./overrides.ts";
import type { Provider, ProviderConfig, RequestOverrides } from "./types.ts";

const DEFAULT_BASE_URL = "https://openrouter.ai/api/v1";

export function createOpenRouterProvider(config: ProviderConfig): Provider {
	const baseUrl = config.baseUrl ?? DEFAULT_BASE_URL;

	function buildHeaders(): Record<string, string> {
		return {
			Authorization: `Bearer ${config.apiKey}`,
			"Content-Type": "application/json",
			"HTTP-Referer": "https://github.com/gitschwifty/heddle",
			"X-Title": "Heddle",
		};
	}

	function buildBody(
		messages: Message[],
		tools: ToolDefinition[] | undefined,
		stream: boolean,
		overrides?: RequestOverrides,
	) {
		const validated = overrides ? validateOverrides(overrides) : undefined;
		const body: Record<string, unknown> = {
			model: config.model,
			messages,
			stream,
			...config.requestParams,
			...(validated ?? {}),
		};
		// model override handled explicitly (top-level, not nested in requestParams)
		if (validated?.model) body.model = validated.model;
		if (tools?.length) {
			body.tools = tools;
		}
		debug("provider", "request:", { model: body.model, stream, overrideKeys: Object.keys(validated ?? {}) });
		return JSON.stringify(body);
	}

	async function send(
		messages: Message[],
		tools?: ToolDefinition[],
		overrides?: RequestOverrides,
	): Promise<ChatCompletionResponse> {
		const response = await fetch(`${baseUrl}/chat/completions`, {
			method: "POST",
			headers: buildHeaders(),
			body: buildBody(messages, tools, false, overrides),
		});

		if (!response.ok) {
			const errorBody = await response.text();
			throw new Error(`OpenRouter API error (${response.status}): ${errorBody}`);
		}

		return (await response.json()) as ChatCompletionResponse;
	}

	async function* stream(
		messages: Message[],
		tools?: ToolDefinition[],
		overrides?: RequestOverrides,
	): AsyncGenerator<StreamChunk> {
		const response = await fetch(`${baseUrl}/chat/completions`, {
			method: "POST",
			headers: buildHeaders(),
			body: buildBody(messages, tools, true, overrides),
		});

		if (!response.ok) {
			const errorBody = await response.text();
			throw new Error(`OpenRouter API error (${response.status}): ${errorBody}`);
		}

		const reader = response.body?.getReader();
		if (!reader) throw new Error("No response body");

		const decoder = new TextDecoder();
		let buffer = "";

		while (true) {
			const { done, value } = await reader.read();
			if (done) break;

			buffer += decoder.decode(value, { stream: true });

			const lines = buffer.split("\n");
			// Keep the last potentially incomplete line in the buffer
			buffer = lines.pop() ?? "";

			for (const line of lines) {
				const trimmed = line.trim();
				if (!trimmed || !trimmed.startsWith("data: ")) continue;

				const data = trimmed.slice(6);
				if (data === "[DONE]") return;

				const chunk = JSON.parse(data) as StreamChunk;
				yield chunk;
			}
		}

		// Process any remaining buffer
		if (buffer.trim()) {
			const trimmed = buffer.trim();
			if (trimmed.startsWith("data: ") && trimmed.slice(6) !== "[DONE]") {
				const chunk = JSON.parse(trimmed.slice(6)) as StreamChunk;
				yield chunk;
			}
		}
	}

	function withOverrides(overrides: RequestOverrides): Provider {
		const validated = validateOverrides(overrides) as OpenRouterOverrides;
		return createOpenRouterProvider({
			...config,
			model: validated.model ?? config.model,
			requestParams: { ...config.requestParams, ...validated },
		});
	}

	return { send, stream, with: withOverrides };
}
