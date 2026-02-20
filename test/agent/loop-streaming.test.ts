import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { runAgentLoop, runAgentLoopStreaming } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import {
	finishChunk,
	mockTextResponse,
	mockToolCallResponse,
	textChunk,
	toolCallChunk,
	usageChunk,
} from "../mocks/openrouter.ts";

/** Create a mock streaming provider from arrays of stream chunks (one per call). */
function mockStreamProvider(streamSets: StreamChunk[][]): Provider {
	let callIndex = 0;
	const p: Provider = {
		async send() {
			throw new Error("send not used");
		},
		async *stream() {
			const chunks = streamSets[callIndex++] ?? [];
			for (const chunk of chunks) yield chunk;
		},
		with() {
			return p;
		},
	};
	return p;
}

/** Create a mock provider for non-streaming runAgentLoop. */
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
			throw new Error("stream not used");
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

function echoTool(): HeddleTool {
	return {
		name: "echo",
		description: "Returns the input string",
		parameters: Type.Object({ text: Type.String() }),
		execute: async (params) => {
			const { text } = params as { text: string };
			return text;
		},
	};
}

describe("Streaming Agent Loop", () => {
	test("text-only streaming: content_delta events then assistant_message", async () => {
		const provider = mockStreamProvider([[textChunk("Hello"), textChunk(" world"), finishChunk("stop")]]);
		const registry = new ToolRegistry();
		const messages: Message[] = [{ role: "user", content: "Hi" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages));

		// Should get 2 content_deltas + 1 assistant_message
		const deltas = events.filter((e) => e.type === "content_delta");
		expect(deltas).toHaveLength(2);
		if (deltas[0]?.type === "content_delta") expect(deltas[0].text).toBe("Hello");
		if (deltas[1]?.type === "content_delta") expect(deltas[1].text).toBe(" world");

		const assistantEvents = events.filter((e) => e.type === "assistant_message");
		expect(assistantEvents).toHaveLength(1);
		if (assistantEvents[0]?.type === "assistant_message") {
			expect(assistantEvents[0].message.content).toBe("Hello world");
		}
	});

	test("tool call assembly from stream: deltas assembled, tools executed", async () => {
		const provider = mockStreamProvider([
			[
				// Tool call delta: id + name start
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				// Tool call delta: arguments in fragments
				toolCallChunk(0, { arguments: '{"te' }),
				toolCallChunk(0, { arguments: 'xt":"' }),
				toolCallChunk(0, { arguments: 'ping"}' }),
				finishChunk("tool_calls"),
			],
			// Second call: text response after tool result
			[textChunk("Got: ping"), finishChunk("stop")],
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "echo ping" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages));

		const types = events.map((e) => e.type);
		expect(types).toContain("assistant_message");
		expect(types).toContain("tool_start");
		expect(types).toContain("tool_end");

		const toolStart = events.find((e) => e.type === "tool_start");
		if (toolStart?.type === "tool_start") {
			expect(toolStart.name).toBe("echo");
		}

		const toolEnd = events.find((e) => e.type === "tool_end");
		if (toolEnd?.type === "tool_end") {
			expect(toolEnd.name).toBe("echo");
			expect(toolEnd.result).toBe("ping");
		}
	});

	test("mixed content + tool calls in stream", async () => {
		const provider = mockStreamProvider([
			[
				// Text content first
				textChunk("Let me "),
				textChunk("do that."),
				// Then tool call
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"hi"}' }),
				finishChunk("tool_calls"),
			],
			[textChunk("Done"), finishChunk("stop")],
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "do it" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages));

		const deltas = events.filter((e) => e.type === "content_delta");
		expect(deltas).toHaveLength(3); // "Let me ", "do that.", "Done"

		const assistantMsgs = events.filter((e) => e.type === "assistant_message");
		expect(assistantMsgs).toHaveLength(2);
		if (assistantMsgs[0]?.type === "assistant_message") {
			expect(assistantMsgs[0].message.content).toBe("Let me do that.");
			expect(assistantMsgs[0].message.tool_calls).toHaveLength(1);
		}
	});

	test("multiple parallel tool calls from stream", async () => {
		const provider = mockStreamProvider([
			[
				// Tool call at index 0
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"a"}' }),
				// Tool call at index 1
				toolCallChunk(1, { id: "call_1", name: "echo" }),
				toolCallChunk(1, { arguments: '{"text":"b"}' }),
				finishChunk("tool_calls"),
			],
			[textChunk("Both done"), finishChunk("stop")],
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "parallel" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages));

		const toolStarts = events.filter((e) => e.type === "tool_start");
		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolStarts).toHaveLength(2);
		expect(toolEnds).toHaveLength(2);

		if (toolEnds[0]?.type === "tool_end") expect(toolEnds[0].result).toBe("a");
		if (toolEnds[1]?.type === "tool_end") expect(toolEnds[1].result).toBe("b");
	});

	test("multi-turn streaming: tool call → execute → text response", async () => {
		const provider = mockStreamProvider([
			// Turn 1: tool call
			[
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"first"}' }),
				finishChunk("tool_calls"),
			],
			// Turn 2: text response
			[textChunk("Got first"), finishChunk("stop")],
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "do it" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages));

		const types = events.map((e) => e.type);
		expect(types).toEqual([
			"assistant_message", // tool call assembled
			"tool_start",
			"tool_end",
			"content_delta", // "Got first"
			"assistant_message", // final text
		]);
		// No usage events since no usageChunk was provided

		// Verify messages array was mutated correctly
		expect(messages).toHaveLength(4); // user, assistant(tool), tool_result, assistant(text)
	});

	test("requestOverrides passes through to provider.stream()", async () => {
		let capturedOverrides: Record<string, unknown> | undefined;
		const p: Provider = {
			async send() {
				throw new Error("not used");
			},
			async *stream(_messages, _tools, overrides) {
				capturedOverrides = overrides;
				yield textChunk("Hi");
				yield finishChunk("stop");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();
		const overrides = { temperature: 0.7 };
		await collectEvents(
			runAgentLoopStreaming(p, registry, [{ role: "user", content: "Hi" }], { requestOverrides: overrides }),
		);
		expect(capturedOverrides).toEqual(overrides);
	});

	test("doom loop detection (streaming): 3 identical iterations yields loop_detected", async () => {
		const provider = mockStreamProvider([
			// 3 identical tool call iterations
			...[1, 2, 3].map(() => [
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"same"}' }),
				finishChunk("tool_calls"),
			]),
			// This should never be reached
			[textChunk("unreachable"), finishChunk("stop")],
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "loop" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages, { doomLoopThreshold: 3 }));

		const loopEvents = events.filter((e) => e.type === "loop_detected");
		expect(loopEvents).toHaveLength(1);
		if (loopEvents[0]?.type === "loop_detected") {
			expect(loopEvents[0].count).toBe(3);
		}
	});

	test("doom loop: 2 identical calls does NOT trigger (threshold 3)", async () => {
		const provider = mockStreamProvider([
			// 2 identical tool call iterations
			...[1, 2].map(() => [
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"same"}' }),
				finishChunk("tool_calls"),
			]),
			// Then a text response
			[textChunk("done"), finishChunk("stop")],
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "twice" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages, { doomLoopThreshold: 3 }));

		const loopEvents = events.filter((e) => e.type === "loop_detected");
		expect(loopEvents).toHaveLength(0);
	});

	test("usage event from streaming (via usageChunk)", async () => {
		const provider = mockStreamProvider([
			[
				textChunk("Hello"),
				finishChunk("stop"),
				usageChunk({ prompt_tokens: 20, completion_tokens: 10, total_tokens: 30 }),
			],
		]);
		const registry = new ToolRegistry();
		const events = await collectEvents(runAgentLoopStreaming(provider, registry, [{ role: "user", content: "Hi" }]));

		const usageEvents = events.filter((e) => e.type === "usage");
		expect(usageEvents).toHaveLength(1);
		if (usageEvents[0]?.type === "usage") {
			expect(usageEvents[0].usage.prompt_tokens).toBe(20);
			expect(usageEvents[0].usage.total_tokens).toBe(30);
		}
	});

	test("no usage event when no chunk has usage", async () => {
		const provider = mockStreamProvider([[textChunk("Hello"), finishChunk("stop")]]);
		const registry = new ToolRegistry();
		const events = await collectEvents(runAgentLoopStreaming(provider, registry, [{ role: "user", content: "Hi" }]));

		const usageEvents = events.filter((e) => e.type === "usage");
		expect(usageEvents).toHaveLength(0);
	});

	test("usage event comes after assistant_message", async () => {
		const provider = mockStreamProvider([
			[
				textChunk("Hello"),
				finishChunk("stop"),
				usageChunk({ prompt_tokens: 20, completion_tokens: 10, total_tokens: 30 }),
			],
		]);
		const registry = new ToolRegistry();
		const events = await collectEvents(runAgentLoopStreaming(provider, registry, [{ role: "user", content: "Hi" }]));

		const types = events.map((e) => e.type);
		const assistantIdx = types.indexOf("assistant_message");
		const usageIdx = types.indexOf("usage");
		expect(usageIdx).toBeGreaterThan(assistantIdx);
	});

	test("multi-turn streaming: 2 rounds produce 2 usage events", async () => {
		const provider = mockStreamProvider([
			[
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"ping"}' }),
				finishChunk("tool_calls"),
				usageChunk({ prompt_tokens: 20, completion_tokens: 10, total_tokens: 30 }),
			],
			[
				textChunk("Done"),
				finishChunk("stop"),
				usageChunk({ prompt_tokens: 30, completion_tokens: 15, total_tokens: 45 }),
			],
		]);
		const registry = new ToolRegistry();
		registry.register(echoTool());
		const events = await collectEvents(runAgentLoopStreaming(provider, registry, [{ role: "user", content: "echo" }]));

		const usageEvents = events.filter((e) => e.type === "usage");
		expect(usageEvents).toHaveLength(2);
	});

	test("doom loop: different calls does NOT trigger", async () => {
		const provider = mockStreamProvider([
			[
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"a"}' }),
				finishChunk("tool_calls"),
			],
			[
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"b"}' }),
				finishChunk("tool_calls"),
			],
			[
				toolCallChunk(0, { id: "call_0", name: "echo" }),
				toolCallChunk(0, { arguments: '{"text":"c"}' }),
				finishChunk("tool_calls"),
			],
			[textChunk("done"), finishChunk("stop")],
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "different" }];

		const events = await collectEvents(runAgentLoopStreaming(provider, registry, messages, { doomLoopThreshold: 3 }));

		const loopEvents = events.filter((e) => e.type === "loop_detected");
		expect(loopEvents).toHaveLength(0);
	});
});

describe("Doom Loop Detection in runAgentLoop (non-streaming)", () => {
	test("3 identical tool call iterations yields loop_detected", async () => {
		const provider = mockProvider([
			// 3 identical tool call responses
			...Array.from({ length: 3 }, () => mockToolCallResponse([{ name: "echo", arguments: { text: "same" } }])),
			// Should not be reached
			mockTextResponse("unreachable"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "loop" }];

		const events = await collectEvents(runAgentLoop(provider, registry, messages, { doomLoopThreshold: 3 }));

		const loopEvents = events.filter((e) => e.type === "loop_detected");
		expect(loopEvents).toHaveLength(1);
		if (loopEvents[0]?.type === "loop_detected") {
			expect(loopEvents[0].count).toBe(3);
		}
	});

	test("different tool calls do NOT trigger doom loop", async () => {
		const provider = mockProvider([
			mockToolCallResponse([{ name: "echo", arguments: { text: "a" } }]),
			mockToolCallResponse([{ name: "echo", arguments: { text: "b" } }]),
			mockToolCallResponse([{ name: "echo", arguments: { text: "c" } }]),
			mockTextResponse("done"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const messages: Message[] = [{ role: "user", content: "different" }];

		const events = await collectEvents(runAgentLoop(provider, registry, messages, { doomLoopThreshold: 3 }));

		const loopEvents = events.filter((e) => e.type === "loop_detected");
		expect(loopEvents).toHaveLength(0);

		// Should complete normally with a final text response
		const assistantMsgs = events.filter((e) => e.type === "assistant_message");
		expect(assistantMsgs.length).toBeGreaterThanOrEqual(4); // 3 tool calls + 1 text
	});
});
