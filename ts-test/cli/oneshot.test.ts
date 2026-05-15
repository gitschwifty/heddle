import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import type { OneshotResult } from "../../src/cli/oneshot.ts";
import { formatOneshotOutput, runOneshotWithContext } from "../../src/cli/oneshot.ts";
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
			throw new Error("stream not used in oneshot tests");
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

describe("runOneshotWithContext", () => {
	test("basic prompt returns response with exit code 0", async () => {
		const provider = mockProvider([mockTextResponse("Hello from the model!")]);
		const registry = new ToolRegistry();

		const result = await runOneshotWithContext({
			prompt: "Say hello",
			provider,
			registry,
			messages: [{ role: "system", content: "You are a test assistant." }],
		});

		expect(result.output).toBe("Hello from the model!");
		expect(result.exitCode).toBe(0);
		expect(result.toolCalls).toBe(0);
	});

	test("response with tool calls tracks tool call count", async () => {
		const provider = mockProvider([
			mockToolCallResponse([{ name: "echo", arguments: { text: "ping" } }]),
			mockTextResponse("Got: ping"),
		]);
		const registry = new ToolRegistry();
		registry.register(echoTool());

		const result = await runOneshotWithContext({
			prompt: "Echo ping",
			provider,
			registry,
			messages: [{ role: "system", content: "You are a test assistant." }],
		});

		expect(result.output).toBe("Got: ping");
		expect(result.exitCode).toBe(0);
		expect(result.toolCalls).toBe(1);
	});

	test("multiple tool calls in sequence are counted", async () => {
		const provider = mockProvider([
			mockToolCallResponse([
				{ name: "echo", arguments: { text: "one" } },
				{ name: "echo", arguments: { text: "two" } },
			]),
			mockTextResponse("Done with two calls"),
		]);
		const registry = new ToolRegistry();
		registry.register(echoTool());

		const result = await runOneshotWithContext({
			prompt: "Echo both",
			provider,
			registry,
			messages: [{ role: "system", content: "You are a test assistant." }],
		});

		expect(result.output).toBe("Done with two calls");
		expect(result.exitCode).toBe(0);
		expect(result.toolCalls).toBe(2);
	});

	test("provider error returns exit code 1", async () => {
		const p: Provider = {
			async send(): Promise<ChatCompletionResponse> {
				throw new Error("Server exploded");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("not used");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();

		const result = await runOneshotWithContext({
			prompt: "This will fail",
			provider: p,
			registry,
			messages: [{ role: "system", content: "You are a test assistant." }],
		});

		expect(result.exitCode).toBe(1);
		expect(result.output).toContain("Server exploded");
	});

	test("empty prompt returns error with exit code 1", async () => {
		const provider = mockProvider([]);
		const registry = new ToolRegistry();

		const result = await runOneshotWithContext({
			prompt: "",
			provider,
			registry,
			messages: [{ role: "system", content: "You are a test assistant." }],
		});

		expect(result.exitCode).toBe(1);
		expect(result.output).toContain("No prompt provided");
	});

	test("no response choices returns error", async () => {
		const provider = mockProvider([
			{
				id: "chatcmpl-test",
				choices: [
					{
						index: 0,
						message: { role: "assistant", content: null, tool_calls: undefined },
						finish_reason: "stop",
					},
				],
				usage: { prompt_tokens: 10, completion_tokens: 0, total_tokens: 10 },
			},
		]);
		const registry = new ToolRegistry();

		const result = await runOneshotWithContext({
			prompt: "What?",
			provider,
			registry,
			messages: [{ role: "system", content: "You are a test assistant." }],
		});

		// null content should return empty string output, not error
		expect(result.exitCode).toBe(0);
		expect(result.output).toBe("");
	});
});

describe("formatOneshotOutput", () => {
	const baseResult: OneshotResult = {
		output: "The answer is 42",
		exitCode: 0,
		toolCalls: 2,
	};

	test("json mode returns JSON with all fields", () => {
		const formatted = formatOneshotOutput(baseResult, { prompt: "test", json: true });

		const parsed = JSON.parse(formatted);
		expect(parsed.output).toBe("The answer is 42");
		expect(parsed.exitCode).toBe(0);
		expect(parsed.toolCalls).toBe(2);
	});

	test("quiet mode returns just the output text", () => {
		const formatted = formatOneshotOutput(baseResult, { prompt: "test", quiet: true });

		expect(formatted).toBe("The answer is 42");
	});

	test("default mode returns the output text", () => {
		const formatted = formatOneshotOutput(baseResult, { prompt: "test" });

		expect(formatted).toBe("The answer is 42");
	});

	test("json mode with error result", () => {
		const errorResult: OneshotResult = {
			output: "Something went wrong",
			exitCode: 1,
			toolCalls: 0,
		};
		const formatted = formatOneshotOutput(errorResult, { prompt: "test", json: true });

		const parsed = JSON.parse(formatted);
		expect(parsed.exitCode).toBe(1);
		expect(parsed.toolCalls).toBe(0);
	});
});
