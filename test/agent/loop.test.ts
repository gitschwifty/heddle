import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { runAgentLoop } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import { mockTextResponse, mockToolCallResponse } from "../mocks/openrouter.ts";

/** Create a mock provider from an array of responses (returned in order). */
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
			throw new Error("stream not used in loop tests");
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

describe("Agent Loop", () => {
	test("text-only response terminates immediately", async () => {
		const provider = mockProvider([mockTextResponse("Hello!")]);
		const registry = new ToolRegistry();

		const events = await collectEvents(runAgentLoop(provider, registry, [{ role: "user", content: "Hi" }]));

		expect(events).toHaveLength(1);
		expect(events[0]?.type).toBe("assistant_message");
		if (events[0]?.type === "assistant_message") {
			expect(events[0].message.content).toBe("Hello!");
		}
	});

	test("tool call → execute → result → text response (single turn)", async () => {
		const provider = mockProvider([
			// First response: tool call
			mockToolCallResponse([{ name: "echo", arguments: { text: "ping" } }]),
			// Second response: text after tool result
			mockTextResponse("Got: ping"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());

		const events = await collectEvents(runAgentLoop(provider, registry, [{ role: "user", content: "echo ping" }]));

		// Expect: assistant_message (tool_call), tool_start, tool_end, assistant_message (text)
		expect(events).toHaveLength(4);
		expect(events[0]?.type).toBe("assistant_message");
		expect(events[1]?.type).toBe("tool_start");
		expect(events[2]?.type).toBe("tool_end");
		expect(events[3]?.type).toBe("assistant_message");

		if (events[1]?.type === "tool_start") {
			expect(events[1].name).toBe("echo");
		}
		if (events[2]?.type === "tool_end") {
			expect(events[2].name).toBe("echo");
			expect(events[2].result).toBe("ping");
		}
		if (events[3]?.type === "assistant_message") {
			expect(events[3].message.content).toBe("Got: ping");
		}
	});

	test("multi-turn tool loop", async () => {
		const provider = mockProvider([
			// Turn 1: tool call
			mockToolCallResponse([{ name: "echo", arguments: { text: "first" } }]),
			// Turn 2: another tool call
			mockToolCallResponse([{ name: "echo", arguments: { text: "second" } }]),
			// Turn 3: final text response
			mockTextResponse("Done"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());

		const events = await collectEvents(runAgentLoop(provider, registry, [{ role: "user", content: "do two things" }]));

		// 2 tool rounds × (assistant + tool_start + tool_end) + 1 final assistant = 7
		expect(events).toHaveLength(7);

		const types = events.map((e) => e.type);
		expect(types).toEqual([
			"assistant_message",
			"tool_start",
			"tool_end",
			"assistant_message",
			"tool_start",
			"tool_end",
			"assistant_message",
		]);
	});

	test("multiple parallel tool calls in single response", async () => {
		const provider = mockProvider([
			// Response with 2 parallel tool calls
			{
				id: "chatcmpl-test",
				choices: [
					{
						index: 0,
						message: {
							role: "assistant",
							content: null,
							tool_calls: [
								{
									id: "call_0",
									type: "function",
									function: { name: "echo", arguments: '{"text":"a"}' },
								},
								{
									id: "call_1",
									type: "function",
									function: { name: "echo", arguments: '{"text":"b"}' },
								},
							],
						},
						finish_reason: "tool_calls",
					},
				],
				usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
			},
			mockTextResponse("Both done"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());

		const events = await collectEvents(runAgentLoop(provider, registry, [{ role: "user", content: "parallel" }]));

		// assistant_message, tool_start×2, tool_end×2, assistant_message
		expect(events).toHaveLength(6);
		const toolStarts = events.filter((e) => e.type === "tool_start");
		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolStarts).toHaveLength(2);
		expect(toolEnds).toHaveLength(2);
	});

	test("max iterations prevents infinite loop", async () => {
		// Provider always returns tool calls — use different args each time to avoid doom loop
		const infiniteToolCalls = Array.from({ length: 20 }, (_, i) =>
			mockToolCallResponse([{ name: "echo", arguments: { text: `loop-${i}` } }]),
		);
		const provider = mockProvider(infiniteToolCalls);

		const registry = new ToolRegistry();
		registry.register(echoTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "loop" }], { maxIterations: 3 }),
		);

		// Should stop after 3 iterations even though provider keeps returning tool calls
		const errorEvents = events.filter((e) => e.type === "error");
		expect(errorEvents).toHaveLength(1);
		if (errorEvents[0]?.type === "error") {
			expect(errorEvents[0].error.message).toContain("Max iterations");
		}
	});

	test("requestOverrides passes through to provider.send()", async () => {
		let capturedOverrides: Record<string, unknown> | undefined;
		const p: Provider = {
			async send(_messages, _tools, overrides) {
				capturedOverrides = overrides;
				return mockTextResponse("Hi");
			},
			async *stream() {
				throw new Error("not used");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();
		const overrides = { temperature: 0.7 };
		await collectEvents(runAgentLoop(p, registry, [{ role: "user", content: "Hi" }], { requestOverrides: overrides }));
		expect(capturedOverrides).toEqual(overrides);
	});

	test("mutates the passed-in messages array", async () => {
		const provider = mockProvider([
			mockToolCallResponse([{ name: "echo", arguments: { text: "ping" } }]),
			mockTextResponse("Done"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());

		const messages: Message[] = [{ role: "user", content: "echo ping" }];
		await collectEvents(runAgentLoop(provider, registry, messages));

		// Loop should have appended: assistant(tool_call), tool_result, assistant(text)
		expect(messages).toHaveLength(4);
		expect(messages[0]!.role).toBe("user");
		expect(messages[1]!.role).toBe("assistant");
		expect(messages[2]!.role).toBe("tool");
		expect(messages[3]!.role).toBe("assistant");
	});
});
