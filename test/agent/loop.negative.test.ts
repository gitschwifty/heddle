import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { runAgentLoop } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import type { Provider } from "../../src/provider/types.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";
import { mockToolCallResponse, mockTextResponse } from "../mocks/openrouter.ts";

function mockProvider(responses: ChatCompletionResponse[]): Provider {
	let callIndex = 0;
	return {
		async send(_messages: Message[], _tools?: ToolDefinition[]): Promise<ChatCompletionResponse> {
			const resp = responses[callIndex];
			if (!resp) throw new Error("No more mock responses");
			callIndex++;
			return resp;
		},
		async *stream(): AsyncGenerator<StreamChunk> {
			throw new Error("stream not used in loop tests");
		},
	};
}

async function collectEvents(gen: AsyncGenerator<AgentEvent>): Promise<AgentEvent[]> {
	const events: AgentEvent[] = [];
	for await (const event of gen) {
		events.push(event);
	}
	return events;
}

describe("Agent Loop (negative)", () => {
	test("throws when provider.send() throws", async () => {
		const provider: Provider = {
			async send(): Promise<ChatCompletionResponse> {
				throw new Error("API is down");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("not used");
			},
		};
		const registry = new ToolRegistry();

		expect(
			collectEvents(runAgentLoop(provider, registry, [{ role: "user", content: "Hi" }])),
		).rejects.toThrow("API is down");
	});

	test("yields error when response has empty choices array", async () => {
		const provider = mockProvider([
			{ id: "test", choices: [], usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 } },
		]);
		const registry = new ToolRegistry();

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "Hi" }]),
		);

		const errorEvents = events.filter((e) => e.type === "error");
		expect(errorEvents).toHaveLength(1);
		if (errorEvents[0]?.type === "error") {
			expect(errorEvents[0].error.message).toContain("No choice");
		}
	});

	test("throws when tool call references unknown tool", async () => {
		const provider = mockProvider([
			mockToolCallResponse([{ name: "nonexistent_tool", arguments: { x: 1 } }]),
			mockTextResponse("Done"),
		]);
		const registry = new ToolRegistry();

		expect(
			collectEvents(runAgentLoop(provider, registry, [{ role: "user", content: "call a tool" }])),
		).rejects.toThrow("Unknown tool: nonexistent_tool");
	});

	test("tool that returns error string doesn't crash the loop", async () => {
		const failTool: HeddleTool = {
			name: "fail",
			description: "Always errors",
			parameters: Type.Object({ input: Type.String() }),
			execute: async () => {
				throw new Error("Tool exploded");
			},
		};

		const provider = mockProvider([
			mockToolCallResponse([{ name: "fail", arguments: { input: "test" } }]),
			mockTextResponse("Handled the error"),
		]);

		const registry = new ToolRegistry();
		registry.register(failTool);

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "try it" }]),
		);

		// The loop should complete — registry.execute catches throws and returns error string
		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(1);
		if (toolEnds[0]?.type === "tool_end") {
			expect(toolEnds[0].result).toContain("Tool exploded");
		}

		// Loop should continue to final text response
		const assistantMessages = events.filter((e) => e.type === "assistant_message");
		expect(assistantMessages).toHaveLength(2);
	});

	test("maxIterations = 1 stops after single tool round", async () => {
		const echoTool: HeddleTool = {
			name: "echo",
			description: "Echo",
			parameters: Type.Object({ text: Type.String() }),
			execute: async (params) => (params as { text: string }).text,
		};

		const provider = mockProvider([
			mockToolCallResponse([{ name: "echo", arguments: { text: "a" } }]),
			mockToolCallResponse([{ name: "echo", arguments: { text: "b" } }]),
			mockTextResponse("done"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool);

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "go" }], { maxIterations: 1 }),
		);

		const errorEvents = events.filter((e) => e.type === "error");
		expect(errorEvents).toHaveLength(1);
		if (errorEvents[0]?.type === "error") {
			expect(errorEvents[0].error.message).toContain("Max iterations");
		}
	});
});
