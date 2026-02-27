import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { runAgentLoop } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import { PermissionChecker, readOnlyToolFilter } from "../../src/permissions/index.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import { mockTextResponse, mockToolCallResponse } from "../mocks/openrouter.ts";

function readTool(): HeddleTool {
	return {
		name: "read_file",
		description: "Read a file",
		parameters: Type.Object({ path: Type.String() }),
		execute: async (params) => {
			const { path } = params as { path: string };
			return `contents of ${path}`;
		},
	};
}

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

function bashTool(): HeddleTool {
	return {
		name: "bash",
		description: "Execute bash",
		parameters: Type.Object({ command: Type.String() }),
		execute: async (params) => {
			const { command } = params as { command: string };
			return `executed: ${command}`;
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
			throw new Error("not used");
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

describe("Plan Mode", () => {
	test("loop only sends read tools to provider when toolFilter applied", async () => {
		let capturedTools: ToolDefinition[] | undefined;
		const p: Provider = {
			async send(_messages: Message[], tools?: ToolDefinition[]): Promise<ChatCompletionResponse> {
				capturedTools = tools;
				return mockTextResponse("Here is my plan...");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("not used");
			},
			with() {
				return p;
			},
		};

		const registry = new ToolRegistry();
		registry.register(readTool());
		registry.register(writeTool());
		registry.register(bashTool());

		await collectEvents(
			runAgentLoop(p, registry, [{ role: "user", content: "plan something" }], {
				toolFilter: readOnlyToolFilter,
			}),
		);

		expect(capturedTools).toBeDefined();
		const toolNames = capturedTools!.map((t) => t.function.name);
		expect(toolNames).toContain("read_file");
		expect(toolNames).not.toContain("write_file");
		expect(toolNames).not.toContain("bash");
	});

	test("write tool denied even if LLM hallucinates the call (belt+suspenders)", async () => {
		const checker = new PermissionChecker("plan");
		// LLM somehow returns a write_file call despite not being in the tool list
		const provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { path: "foo.txt", content: "bar" } }]),
			mockTextResponse("Plan complete"),
		]);

		const registry = new ToolRegistry();
		registry.register(readTool());
		registry.register(writeTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "plan" }], {
				permissionChecker: checker,
				toolFilter: readOnlyToolFilter,
			}),
		);

		const denied = events.filter((e) => e.type === "permission_denied");
		expect(denied).toHaveLength(1);
		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(0);
	});

	test("plan_complete event contains final assistant message content", async () => {
		const provider = mockProvider([
			mockToolCallResponse([{ name: "read_file", arguments: { path: "src/index.ts" } }]),
			mockTextResponse("## Plan\n1. Do X\n2. Do Y\n3. Do Z"),
		]);

		const registry = new ToolRegistry();
		registry.register(readTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "plan" }], {
				planMode: true,
			}),
		);

		const planComplete = events.filter((e) => e.type === "plan_complete");
		expect(planComplete).toHaveLength(1);
		if (planComplete[0]?.type === "plan_complete") {
			expect(planComplete[0].plan).toBe("## Plan\n1. Do X\n2. Do Y\n3. Do Z");
		}
	});

	test("integration: plan phase with tool filtering + permission checker", async () => {
		const checker = new PermissionChecker("plan");
		const provider = mockProvider([
			// LLM reads a file (allowed)
			mockToolCallResponse([{ name: "read_file", arguments: { path: "src/main.ts" } }]),
			// LLM produces plan text
			mockTextResponse("Here is my plan: refactor main.ts"),
		]);

		const registry = new ToolRegistry();
		registry.register(readTool());
		registry.register(writeTool());
		registry.register(bashTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "plan the refactor" }], {
				permissionChecker: checker,
				toolFilter: readOnlyToolFilter,
				planMode: true,
			}),
		);

		// read_file tool should execute
		const toolEnds = events.filter((e) => e.type === "tool_end");
		expect(toolEnds).toHaveLength(1);
		if (toolEnds[0]?.type === "tool_end") {
			expect(toolEnds[0].name).toBe("read_file");
		}

		// plan_complete should be emitted
		const planComplete = events.filter((e) => e.type === "plan_complete");
		expect(planComplete).toHaveLength(1);
		if (planComplete[0]?.type === "plan_complete") {
			expect(planComplete[0].plan).toContain("refactor main.ts");
		}
	});

	test("plan_complete not emitted when planMode is false/undefined", async () => {
		const provider = mockProvider([mockTextResponse("Just a response")]);

		const registry = new ToolRegistry();

		const events = await collectEvents(runAgentLoop(provider, registry, [{ role: "user", content: "hello" }]));

		const planComplete = events.filter((e) => e.type === "plan_complete");
		expect(planComplete).toHaveLength(0);
	});

	test("plan_complete uses last assistant message even without tools", async () => {
		const provider = mockProvider([mockTextResponse("My plan is: do nothing")]);

		const registry = new ToolRegistry();

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "plan" }], {
				planMode: true,
			}),
		);

		const planComplete = events.filter((e) => e.type === "plan_complete");
		expect(planComplete).toHaveLength(1);
		if (planComplete[0]?.type === "plan_complete") {
			expect(planComplete[0].plan).toBe("My plan is: do nothing");
		}
	});
});
