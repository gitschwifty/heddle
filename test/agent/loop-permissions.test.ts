import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { runAgentLoop, runAgentLoopStreaming } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import { PermissionChecker } from "../../src/permissions/index.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolCall, ToolDefinition } from "../../src/types.ts";
import { finishChunk, mockTextResponse, mockToolCallResponse, textChunk, toolCallChunk } from "../mocks/openrouter.ts";

function writeTool(): HeddleTool {
	return {
		name: "write_file",
		description: "Write a file",
		parameters: Type.Object({ path: Type.String(), content: Type.String() }),
		execute: async (params) => {
			const { path } = params as { path: string; content: string };
			return `wrote ${path}`;
		},
	};
}

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

function mockStreamProvider(chunks: StreamChunk[][]): Provider {
	let callIndex = 0;
	const p: Provider = {
		async send(): Promise<ChatCompletionResponse> {
			throw new Error("send not used");
		},
		async *stream(): AsyncGenerator<StreamChunk> {
			const c = chunks[callIndex];
			if (!c) throw new Error("No more mock stream chunks");
			callIndex++;
			for (const chunk of c) {
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

describe("Agent Loop Permissions", () => {
	test("deny event emitted and tool gets error message", async () => {
		const checker = new PermissionChecker("plan");
		const provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { path: "foo.txt", content: "bar" } }]),
			mockTextResponse("I cannot write in plan mode"),
		]);

		const registry = new ToolRegistry();
		registry.register(writeTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "write foo" }], {
				permissionChecker: checker,
			}),
		);

		const denied = events.filter((e) => e.type === "permission_denied");
		expect(denied).toHaveLength(1);
		if (denied[0]?.type === "permission_denied") {
			expect(denied[0].name).toBe("write_file");
			expect(denied[0].reason).toBeDefined();
		}

		// Tool should NOT have executed — no tool_end events
		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(0);
	});

	test("ask + resolver returns allow → tool executes", async () => {
		const checker = new PermissionChecker("suggest");
		const provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { path: "foo.txt", content: "bar" } }]),
			mockTextResponse("wrote it"),
		]);

		const registry = new ToolRegistry();
		registry.register(writeTool());

		const resolver = async (_name: string, _call: ToolCall) => "allow" as const;

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "write foo" }], {
				permissionChecker: checker,
				permissionResolver: resolver,
			}),
		);

		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(1);
		if (toolEnds[0]?.type === "tool_end") {
			expect(toolEnds[0].result).toBe("wrote foo.txt");
		}
	});

	test("ask + resolver returns deny → tool gets error", async () => {
		const checker = new PermissionChecker("suggest");
		const provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { path: "foo.txt", content: "bar" } }]),
			mockTextResponse("denied"),
		]);

		const registry = new ToolRegistry();
		registry.register(writeTool());

		const resolver = async (_name: string, _call: ToolCall) => "deny" as const;

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "write foo" }], {
				permissionChecker: checker,
				permissionResolver: resolver,
			}),
		);

		const denied = events.filter((e) => e.type === "permission_denied");
		expect(denied).toHaveLength(1);

		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(0);
	});

	test("ask + resolver returns always → subsequent calls auto-allow", async () => {
		const checker = new PermissionChecker("suggest");
		let resolverCalls = 0;
		const resolver = async (_name: string, _call: ToolCall) => {
			resolverCalls++;
			return "always" as const;
		};

		const provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { path: "a.txt", content: "1" } }]),
			mockToolCallResponse([{ name: "write_file", arguments: { path: "b.txt", content: "2" } }]),
			mockTextResponse("done"),
		]);

		const registry = new ToolRegistry();
		registry.register(writeTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "write two files" }], {
				permissionChecker: checker,
				permissionResolver: resolver,
			}),
		);

		// Resolver should only be called once — second time, write_file is in allowAlways set
		expect(resolverCalls).toBe(1);
		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(2);
	});

	test("no resolver = deny by default", async () => {
		const checker = new PermissionChecker("suggest");
		const provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { path: "foo.txt", content: "bar" } }]),
			mockTextResponse("ok"),
		]);

		const registry = new ToolRegistry();
		registry.register(writeTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "write foo" }], {
				permissionChecker: checker,
				// no permissionResolver
			}),
		);

		const denied = events.filter((e) => e.type === "permission_denied");
		expect(denied).toHaveLength(1);
	});

	test(".env file protection overrides full-auto mode", async () => {
		const checker = new PermissionChecker("full-auto");
		const provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { path: ".env.local", content: "SECRET=x" } }]),
			mockTextResponse("cannot write .env"),
		]);

		const registry = new ToolRegistry();
		registry.register(writeTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "write .env" }], {
				permissionChecker: checker,
			}),
		);

		const denied = events.filter((e) => e.type === "permission_denied");
		expect(denied).toHaveLength(1);
		if (denied[0]?.type === "permission_denied") {
			expect(denied[0].reason).toContain(".env");
		}
	});

	test("streaming loop parity — deny event emitted", async () => {
		const checker = new PermissionChecker("plan");
		const streamChunks: StreamChunk[][] = [
			// First response: tool call for write_file
			[
				toolCallChunk(0, { id: "call_0", name: "write_file", arguments: '{"path":"foo.txt","content":"bar"}' }),
				finishChunk("tool_calls"),
			],
			// Second response: text
			[textChunk("I cannot write in plan mode"), finishChunk("stop")],
		];

		const provider = mockStreamProvider(streamChunks);

		const registry = new ToolRegistry();
		registry.register(writeTool());

		const events = await collectEvents(
			runAgentLoopStreaming(provider, registry, [{ role: "user", content: "write foo" }], {
				permissionChecker: checker,
			}),
		);

		const denied = events.filter((e) => e.type === "permission_denied");
		expect(denied).toHaveLength(1);

		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(0);
	});
});
