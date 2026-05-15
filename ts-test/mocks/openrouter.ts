import type { ChatCompletionResponse, StreamChunk } from "../../src/types.ts";

/**
 * Build a mock SSE ReadableStream from an array of stream chunks.
 */
export function mockSSE(chunks: StreamChunk[]): ReadableStream<Uint8Array> {
	const encoder = new TextEncoder();
	return new ReadableStream({
		start(controller) {
			for (const chunk of chunks) {
				controller.enqueue(encoder.encode(`data: ${JSON.stringify(chunk)}\n\n`));
			}
			controller.enqueue(encoder.encode("data: [DONE]\n\n"));
			controller.close();
		},
	});
}

/** Create a stream chunk with text content */
export function textChunk(content: string, id = "chatcmpl-test"): StreamChunk {
	return {
		id,
		choices: [{ index: 0, delta: { content }, finish_reason: null }],
	};
}

/** Create a stream chunk with a finish reason */
export function finishChunk(finish_reason: string, id = "chatcmpl-test"): StreamChunk {
	return {
		id,
		choices: [{ index: 0, delta: {}, finish_reason }],
	};
}

/** Create a stream chunk with tool call delta */
export function toolCallChunk(
	index: number,
	opts: { id?: string; name?: string; arguments?: string },
	id = "chatcmpl-test",
): StreamChunk {
	return {
		id,
		choices: [
			{
				index: 0,
				delta: {
					tool_calls: [
						{
							index,
							...(opts.id ? { id: opts.id, type: "function" as const } : {}),
							...(opts.name || opts.arguments
								? {
										function: {
											...(opts.name ? { name: opts.name } : {}),
											...(opts.arguments ? { arguments: opts.arguments } : {}),
										},
									}
								: {}),
						},
					],
				},
				finish_reason: null,
			},
		],
	};
}

/** Create a non-streaming response with text content */
export function mockTextResponse(content: string): ChatCompletionResponse {
	return {
		id: "chatcmpl-test",
		choices: [
			{
				index: 0,
				message: { role: "assistant", content, tool_calls: undefined },
				finish_reason: "stop",
			},
		],
		usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
	};
}

/** Create a non-streaming response with tool calls */
export function mockToolCallResponse(
	calls: Array<{ name: string; arguments: Record<string, unknown> }>,
): ChatCompletionResponse {
	return {
		id: "chatcmpl-test",
		choices: [
			{
				index: 0,
				message: {
					role: "assistant",
					content: null,
					tool_calls: calls.map((call, i) => ({
						id: `call_${i}`,
						type: "function" as const,
						function: {
							name: call.name,
							arguments: JSON.stringify(call.arguments),
						},
					})),
				},
				finish_reason: "tool_calls",
			},
		],
		usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
	};
}

/** Create a stream chunk with usage data (final SSE chunk from OpenRouter) */
export function usageChunk(
	usage: { prompt_tokens: number; completion_tokens: number; total_tokens: number; cost?: number },
	id = "chatcmpl-test",
): StreamChunk {
	return {
		id,
		choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
		usage,
	};
}

/** Create a mock error response (non-200) */
export function mockErrorResponse(status: number, message: string): Response {
	return new Response(JSON.stringify({ error: { message, type: "error", code: status } }), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

/** Create a mock successful JSON response */
export function mockJsonResponse(body: ChatCompletionResponse): Response {
	return new Response(JSON.stringify(body), {
		status: 200,
		headers: { "Content-Type": "application/json" },
	});
}

/** Create a mock SSE response */
export function mockStreamResponse(chunks: StreamChunk[]): Response {
	return new Response(mockSSE(chunks), {
		status: 200,
		headers: { "Content-Type": "text/event-stream" },
	});
}
