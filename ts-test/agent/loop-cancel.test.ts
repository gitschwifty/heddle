import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { runAgentLoop, runAgentLoopStreaming } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import { finishChunk, mockTextResponse, mockToolCallResponse, textChunk, toolCallChunk } from "../mocks/openrouter.ts";

function mockProvider(responses: ChatCompletionResponse[]): Provider {
	let callIndex = 0;
	const p: Provider = {
		async send(_messages: Message[], _tools?: ToolDefinition[]): Promise<ChatCompletionResponse> {
			const resp = responses[callIndex];
			if (!resp) throw new Error("No more mock responses");
			callIndex++;
			return resp;
		},
		async *stream(): AsyncGenerator<StreamChunk> {
			throw new Error("stream not used in non-streaming tests");
		},
		with() {
			return p;
		},
	};
	return p;
}

function mockStreamProvider(chunkSets: StreamChunk[][]): Provider {
	let callIndex = 0;
	const p: Provider = {
		async send(): Promise<ChatCompletionResponse> {
			throw new Error("send not used in streaming tests");
		},
		async *stream(): AsyncGenerator<StreamChunk> {
			const chunks = chunkSets[callIndex];
			if (!chunks) throw new Error("No more mock chunk sets");
			callIndex++;
			for (const chunk of chunks) {
				yield chunk;
			}
		},
		with() {
			return p;
		},
	};
	return p;
}

async function collectEvents(gen: AsyncGenerator<AgentEvent>): Promise<AgentEvent[]> {
	const events: AgentEvent[] = [];
	for await (const event of gen) {
		events.push(event);
	}
	return events;
}

function slowTool(): HeddleTool {
	return {
		name: "slow",
		description: "A tool that takes a while",
		parameters: Type.Object({ ms: Type.Number() }),
		execute: async (params, options) => {
			const { ms } = params as { ms: number };
			const signal = options?.signal;
			if (signal?.aborted) return "aborted";
			await new Promise<void>((resolve) => {
				const timer = setTimeout(resolve, ms);
				if (signal) {
					signal.addEventListener(
						"abort",
						() => {
							clearTimeout(timer);
							resolve();
						},
						{ once: true },
					);
				}
			});
			return signal?.aborted ? "aborted" : "done";
		},
	};
}

describe("Agent Loop — cancel via AbortSignal", () => {
	test("loop exits early when signal is already aborted", async () => {
		const provider = mockProvider([mockTextResponse("Hello!")]);
		const registry = new ToolRegistry();
		const ac = new AbortController();
		ac.abort();

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "Hi" }], { signal: ac.signal }),
		);

		// Should yield nothing — exited at start of first iteration
		expect(events).toHaveLength(0);
	});

	test("loop exits mid-iteration when signal is aborted before tool execution", async () => {
		const ac = new AbortController();
		const registry = new ToolRegistry();
		registry.register(slowTool());

		// Provider returns a tool call, then text
		const provider = mockProvider([
			mockToolCallResponse([{ name: "slow", arguments: { ms: 5000 } }]),
			mockTextResponse("Done!"),
		]);

		// Abort after the first provider.send() returns (but before tool execution completes)
		// We'll abort immediately after iteration starts — the tool call response arrives,
		// then we check signal before executing tools
		setTimeout(() => ac.abort(), 50);

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "Do something slow" }], { signal: ac.signal }),
		);

		// Should have usage + assistant_message from first iteration, but tool might not complete
		// or might complete with "aborted" — either way, no second iteration
		const assistantMsgs = events.filter((e) => e.type === "assistant_message");
		expect(assistantMsgs.length).toBeLessThanOrEqual(1);
		// Should NOT have a second assistant_message (the "Done!" response)
		const secondResponse = events.find((e) => e.type === "assistant_message" && e.message.content === "Done!");
		expect(secondResponse).toBeUndefined();
	});

	test("streaming loop exits early when signal is already aborted", async () => {
		const provider = mockStreamProvider([[textChunk("Hello!"), finishChunk("stop")]]);
		const registry = new ToolRegistry();
		const ac = new AbortController();
		ac.abort();

		const events = await collectEvents(
			runAgentLoopStreaming(provider, registry, [{ role: "user", content: "Hi" }], { signal: ac.signal }),
		);

		expect(events).toHaveLength(0);
	});

	test("streaming loop exits after stream when signal aborted", async () => {
		const ac = new AbortController();
		const registry = new ToolRegistry();
		registry.register(slowTool());

		const provider = mockStreamProvider([
			[toolCallChunk(0, { id: "call_0", name: "slow", arguments: '{"ms":5000}' }), finishChunk("tool_calls")],
			[textChunk("Done!"), finishChunk("stop")],
		]);

		// Abort shortly after stream completes but before/during tool execution
		setTimeout(() => ac.abort(), 50);

		const events = await collectEvents(
			runAgentLoopStreaming(provider, registry, [{ role: "user", content: "Go" }], { signal: ac.signal }),
		);

		// Should not reach second iteration's text response
		const secondResponse = events.find((e) => e.type === "content_delta" && e.text === "Done!");
		expect(secondResponse).toBeUndefined();
	});
});
