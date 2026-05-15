import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import { createSubagentTool } from "../../src/tools/subagent.ts";
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
			throw new Error("stream not used in subagent tests");
		},
		with() {
			return p;
		},
	};
	return p;
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

function uppercaseTool(): HeddleTool {
	return {
		name: "uppercase",
		description: "Uppercases the input string",
		parameters: Type.Object({ text: Type.String() }),
		execute: async (params) => {
			const { text } = params as { text: string };
			return text.toUpperCase();
		},
	};
}

describe("Subagent Tool", () => {
	test("returns a HeddleTool with correct name and schema", () => {
		const provider = mockProvider([]);
		const registry = new ToolRegistry();
		const tool = createSubagentTool(provider, registry, {});

		expect(tool.name).toBe("subagent");
		expect(tool.description).toBeTruthy();
		expect(tool.parameters).toBeDefined();
	});

	test("runs a simple prompt and returns the assistant response", async () => {
		const provider = mockProvider([mockTextResponse("The answer is 42.")]);
		const registry = new ToolRegistry();
		const tool = createSubagentTool(provider, registry, {});

		const result = await tool.execute({ prompt: "What is the meaning of life?" });
		expect(result).toBe("The answer is 42.");
	});

	test("subagent can use tools from the registry", async () => {
		const provider = mockProvider([
			// First call: subagent decides to call echo tool
			mockToolCallResponse([{ name: "echo", arguments: { text: "hello" } }]),
			// Second call: subagent responds with final text
			mockTextResponse("Echo returned: hello"),
		]);

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const tool = createSubagentTool(provider, registry, {});

		const result = await tool.execute({ prompt: "Use echo to say hello" });
		expect(result).toBe("Echo returned: hello");
	});

	test("filters tools when tools param is provided", async () => {
		// Provider will get called with only the echo tool available
		let capturedTools: ToolDefinition[] | undefined;
		const p: Provider = {
			async send(_messages: Message[], tools?: ToolDefinition[]): Promise<ChatCompletionResponse> {
				capturedTools = tools;
				return mockTextResponse("Done");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();
		registry.register(echoTool());
		registry.register(uppercaseTool());
		const tool = createSubagentTool(p, registry, {});

		await tool.execute({ prompt: "Use echo", tools: ["echo"] });

		expect(capturedTools).toBeDefined();
		expect(capturedTools!.length).toBe(1);
		expect(capturedTools![0]!.function.name).toBe("echo");
	});

	test("returns error string when subagent loop produces no content", async () => {
		// Response with null content and no tool calls
		const emptyResponse: ChatCompletionResponse = {
			id: "chatcmpl-test",
			choices: [
				{
					index: 0,
					message: { role: "assistant", content: null, tool_calls: undefined },
					finish_reason: "stop",
				},
			],
			usage: { prompt_tokens: 10, completion_tokens: 0, total_tokens: 10 },
		};
		const provider = mockProvider([emptyResponse]);
		const registry = new ToolRegistry();
		const tool = createSubagentTool(provider, registry, {});

		const result = await tool.execute({ prompt: "Do something" });
		expect(result).toContain("Error");
	});

	test("returns error string when subagent loop throws", async () => {
		// Provider that throws
		const p: Provider = {
			async send(): Promise<ChatCompletionResponse> {
				throw new Error("API connection failed");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();
		const tool = createSubagentTool(p, registry, {});

		const result = await tool.execute({ prompt: "Do something" });
		expect(result).toContain("Error");
		expect(result).toContain("API connection failed");
	});

	test("respects maxIterations option", async () => {
		// Provider that always returns tool calls (would loop forever)
		let callCount = 0;
		const p: Provider = {
			async send(): Promise<ChatCompletionResponse> {
				callCount++;
				return mockToolCallResponse([{ name: "echo", arguments: { text: "loop" } }]);
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();
		registry.register(echoTool());
		const tool = createSubagentTool(p, registry, { maxIterations: 2 });

		await tool.execute({ prompt: "Loop forever" });
		// Should have stopped after maxIterations
		expect(callCount).toBeLessThanOrEqual(2);
	});

	test("accumulates usage into cost tracker when provided", async () => {
		const { CostTracker } = await import("../../src/cost/tracker.ts");
		const costTracker = new CostTracker();

		const provider = mockProvider([mockTextResponse("Done")]);
		const registry = new ToolRegistry();
		const tool = createSubagentTool(provider, registry, { costTracker });

		await tool.execute({ prompt: "Quick task" });

		// The mock response includes usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }
		expect(costTracker.totalInputTokens).toBe(10);
		expect(costTracker.totalOutputTokens).toBe(5);
	});

	test("subagent messages are isolated from parent context", async () => {
		let capturedMessages: Message[] = [];
		const p: Provider = {
			async send(messages: Message[]): Promise<ChatCompletionResponse> {
				capturedMessages = [...messages];
				return mockTextResponse("Isolated response");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();
		const tool = createSubagentTool(p, registry, {});

		await tool.execute({ prompt: "Do a task" });

		// Should have system message and user message only — no parent context
		expect(capturedMessages.length).toBe(2);
		expect(capturedMessages[0]!.role).toBe("system");
		expect(capturedMessages[1]!.role).toBe("user");
		expect((capturedMessages[1] as { content: string }).content).toBe("Do a task");
	});
});

describe("ToolRegistry.subset", () => {
	test("returns a new registry with only the named tools", () => {
		const registry = new ToolRegistry();
		registry.register(echoTool());
		registry.register(uppercaseTool());

		const sub = registry.subset(["echo"]);
		expect(sub.all()).toHaveLength(1);
		expect(sub.get("echo")).toBeDefined();
		expect(sub.get("uppercase")).toBeUndefined();
	});

	test("ignores names that do not exist in the parent registry", () => {
		const registry = new ToolRegistry();
		registry.register(echoTool());

		const sub = registry.subset(["echo", "nonexistent"]);
		expect(sub.all()).toHaveLength(1);
		expect(sub.get("echo")).toBeDefined();
	});

	test("returns empty registry when no names match", () => {
		const registry = new ToolRegistry();
		registry.register(echoTool());

		const sub = registry.subset(["nonexistent"]);
		expect(sub.all()).toHaveLength(0);
	});

	test("subset registry is independent from parent", () => {
		const registry = new ToolRegistry();
		registry.register(echoTool());

		const sub = registry.subset(["echo"]);
		registry.register(uppercaseTool());

		// Parent got a new tool, but subset should not see it
		expect(sub.all()).toHaveLength(1);
		expect(registry.all()).toHaveLength(2);
	});
});
