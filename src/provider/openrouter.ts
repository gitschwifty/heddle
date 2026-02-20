import { debug } from "../debug.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../types.ts";
import type { OpenRouterOverrides } from "./overrides.ts";
import { validateOverrides } from "./overrides.ts";
import type { Provider, ProviderConfig, RequestOverrides, RetryConfig } from "./types.ts";

const DEFAULT_BASE_URL = "https://openrouter.ai/api/v1";
const DEFAULT_MAX_RETRIES = 3;
const DEFAULT_BASE_DELAY_MS = 1000;

function getRetryConfig(config: ProviderConfig): RetryConfig | null {
	if (config.retry === false) return null;
	return {
		maxRetries: config.retry?.maxRetries ?? DEFAULT_MAX_RETRIES,
		baseDelayMs: config.retry?.baseDelayMs ?? DEFAULT_BASE_DELAY_MS,
	};
}

/** Parse Retry-After header value to ms. Supports seconds (integer) and HTTP-date. */
function parseRetryAfter(header: string | null): number | null {
	if (!header) return null;
	const seconds = Number(header);
	if (!Number.isNaN(seconds)) return seconds * 1000;
	// Try HTTP-date
	const date = Date.parse(header);
	if (!Number.isNaN(date)) return Math.max(0, date - Date.now());
	return null;
}

/** Calculate delay for a retry attempt. Uses Retry-After header if available, otherwise exponential backoff. */
function getDelay(attempt: number, retryAfterMs: number | null, baseDelayMs: number): number {
	if (retryAfterMs !== null) return retryAfterMs;
	return baseDelayMs * 2 ** attempt;
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

export function createOpenRouterProvider(config: ProviderConfig): Provider {
	const baseUrl = config.baseUrl ?? DEFAULT_BASE_URL;
	const retryConfig = getRetryConfig(config);

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
		debug("provider", "request:", body);
		return JSON.stringify(body);
	}

	async function fetchWithRetry(url: string, init: RequestInit): Promise<Response> {
		const maxRetries = retryConfig?.maxRetries ?? 0;

		for (let attempt = 0; attempt <= maxRetries; attempt++) {
			const response = await fetch(url, init);

			if (response.status !== 429 || !retryConfig || attempt === maxRetries) {
				return response;
			}

			const retryAfterMs = parseRetryAfter(response.headers.get("Retry-After"));
			const delay = getDelay(attempt, retryAfterMs, retryConfig.baseDelayMs ?? DEFAULT_BASE_DELAY_MS);
			debug("provider", `429 rate limited, retry ${attempt + 1}/${maxRetries} after ${delay}ms`);
			await sleep(delay);
		}

		// Unreachable, but TypeScript needs it
		throw new Error("Retry loop exited unexpectedly");
	}

	async function send(
		messages: Message[],
		tools?: ToolDefinition[],
		overrides?: RequestOverrides,
	): Promise<ChatCompletionResponse> {
		const response = await fetchWithRetry(`${baseUrl}/chat/completions`, {
			method: "POST",
			headers: buildHeaders(),
			body: buildBody(messages, tools, false, overrides),
		});

		if (!response.ok) {
			const errorBody = await response.text();
			debug("provider", `error ${response.status}: ${errorBody}`);
			throw new Error(`OpenRouter API error (${response.status}): ${errorBody}`);
		}

		const body = await response.json();
		debug("provider", "response", body);
		return body as ChatCompletionResponse;
	}

	async function* stream(
		messages: Message[],
		tools?: ToolDefinition[],
		overrides?: RequestOverrides,
	): AsyncGenerator<StreamChunk> {
		const response = await fetchWithRetry(`${baseUrl}/chat/completions`, {
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
				debug("provider", "chunk", chunk);
				yield chunk;
			}
		}

		// Process any remaining buffer
		if (buffer.trim()) {
			const trimmed = buffer.trim();
			if (trimmed.startsWith("data: ") && trimmed.slice(6) !== "[DONE]") {
				const chunk = JSON.parse(trimmed.slice(6)) as StreamChunk;
				debug("provider", "chunk", chunk);
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
