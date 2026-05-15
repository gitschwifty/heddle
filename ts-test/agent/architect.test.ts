import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { runArchitectPipeline } from "../../src/agent/architect.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import { mockTextResponse } from "../mocks/openrouter.ts";

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

function readFileTool(): HeddleTool {
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

function writeFileTool(): HeddleTool {
	return {
		name: "write_file",
		description: "Write a file",
		parameters: Type.Object({ path: Type.String(), content: Type.String() }),
		execute: async (params) => {
			const { path } = params as { path: string };
			return `wrote to ${path}`;
		},
	};
}

function bashTool(): HeddleTool {
	return {
		name: "bash",
		description: "Execute a bash command",
		parameters: Type.Object({ command: Type.String() }),
		execute: async (params) => {
			const { command } = params as { command: string };
			return `ran: ${command}`;
		},
	};
}

describe("Architect/Editor Pipeline", () => {
	test("architect phase runs in plan mode and produces plan_complete event", async () => {
		const architectProvider = mockProvider([mockTextResponse("Step 1: Read the file\nStep 2: Edit it")]);
		const editorProvider = mockProvider([mockTextResponse("Done editing.")]);

		const registry = new ToolRegistry();
		registry.register(readFileTool());
		registry.register(writeFileTool());

		const messages: Message[] = [{ role: "user", content: "Refactor the code" }];
		const events = await collectEvents(runArchitectPipeline(architectProvider, editorProvider, registry, messages));

		// Should have a plan_complete event from the architect phase
		const planEvent = events.find((e) => e.type === "plan_complete");
		expect(planEvent).toBeDefined();
		if (planEvent?.type === "plan_complete") {
			expect(planEvent.plan).toContain("Step 1");
		}
	});

	test("editor phase receives the plan and produces a final response", async () => {
		const architectProvider = mockProvider([mockTextResponse("Plan: do the thing")]);
		const editorProvider = mockProvider([mockTextResponse("I have done the thing.")]);

		const registry = new ToolRegistry();
		registry.register(readFileTool());

		const messages: Message[] = [{ role: "user", content: "Do the thing" }];
		const events = await collectEvents(runArchitectPipeline(architectProvider, editorProvider, registry, messages));

		// Should have assistant messages from both phases
		const assistantMessages = events.filter((e) => e.type === "assistant_message");
		expect(assistantMessages.length).toBeGreaterThanOrEqual(2);

		// The last assistant message should be from the editor
		const lastMsg = assistantMessages[assistantMessages.length - 1];
		if (lastMsg?.type === "assistant_message") {
			expect(lastMsg.message.content).toBe("I have done the thing.");
		}
	});

	test("architect phase only has access to read-only tools", async () => {
		let architectTools: ToolDefinition[] | undefined;
		let editorTools: ToolDefinition[] | undefined;

		const architectProvider: Provider = {
			async send(_messages: Message[], tools?: ToolDefinition[]): Promise<ChatCompletionResponse> {
				architectTools = tools;
				return mockTextResponse("Plan: read first, then write");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return architectProvider;
			},
		};

		const editorProvider: Provider = {
			async send(_messages: Message[], tools?: ToolDefinition[]): Promise<ChatCompletionResponse> {
				editorTools = tools;
				return mockTextResponse("Done.");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return editorProvider;
			},
		};

		const registry = new ToolRegistry();
		registry.register(readFileTool());
		registry.register(writeFileTool());
		registry.register(bashTool());

		const messages: Message[] = [{ role: "user", content: "Fix the bug" }];
		await collectEvents(runArchitectPipeline(architectProvider, editorProvider, registry, messages));

		// Architect should only see read-only tools (read_file is "read" category)
		expect(architectTools).toBeDefined();
		const architectToolNames = architectTools!.map((t) => t.function.name);
		expect(architectToolNames).toContain("read_file");
		expect(architectToolNames).not.toContain("write_file");
		expect(architectToolNames).not.toContain("bash");

		// Editor should see all tools
		expect(editorTools).toBeDefined();
		const editorToolNames = editorTools!.map((t) => t.function.name);
		expect(editorToolNames).toContain("read_file");
		expect(editorToolNames).toContain("write_file");
		expect(editorToolNames).toContain("bash");
	});

	test("onPlanReady callback can abort the pipeline", async () => {
		const architectProvider = mockProvider([mockTextResponse("A bad plan")]);
		const editorProvider = mockProvider([mockTextResponse("Should not reach here")]);

		const registry = new ToolRegistry();
		const messages: Message[] = [{ role: "user", content: "Do something" }];

		const events = await collectEvents(
			runArchitectPipeline(architectProvider, editorProvider, registry, messages, {
				onPlanReady: async (_plan) => false, // reject the plan
			}),
		);

		// Should have an error event
		const errorEvent = events.find((e) => e.type === "error");
		expect(errorEvent).toBeDefined();

		// Should NOT have any assistant messages from the editor phase
		const assistantMessages = events.filter((e) => e.type === "assistant_message");
		// Only the architect's assistant message
		expect(assistantMessages).toHaveLength(1);
	});

	test("onPlanReady callback receives the plan text", async () => {
		let receivedPlan = "";
		const architectProvider = mockProvider([mockTextResponse("Step 1: Do X\nStep 2: Do Y")]);
		const editorProvider = mockProvider([mockTextResponse("Done")]);

		const registry = new ToolRegistry();
		const messages: Message[] = [{ role: "user", content: "Plan and do" }];

		await collectEvents(
			runArchitectPipeline(architectProvider, editorProvider, registry, messages, {
				onPlanReady: async (plan) => {
					receivedPlan = plan;
					return true;
				},
			}),
		);

		expect(receivedPlan).toBe("Step 1: Do X\nStep 2: Do Y");
	});

	test("editor messages include the plan from architect", async () => {
		let editorMessages: Message[] = [];
		const architectProvider = mockProvider([mockTextResponse("The plan is: refactor utils")]);

		const editorProvider: Provider = {
			async send(messages: Message[]): Promise<ChatCompletionResponse> {
				editorMessages = [...messages];
				return mockTextResponse("Refactored.");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return editorProvider;
			},
		};

		const registry = new ToolRegistry();
		const messages: Message[] = [{ role: "user", content: "Refactor" }];

		await collectEvents(runArchitectPipeline(architectProvider, editorProvider, registry, messages));

		// Editor should receive original messages + a user message containing the plan
		expect(editorMessages.length).toBeGreaterThan(1);
		const lastUserMsg = editorMessages.filter((m) => m.role === "user").pop();
		expect(lastUserMsg).toBeDefined();
		expect((lastUserMsg as { content: string }).content).toContain("The plan is: refactor utils");
	});

	test("events from both phases are yielded", async () => {
		const architectProvider = mockProvider([mockTextResponse("The plan")]);
		const editorProvider = mockProvider([mockTextResponse("Executed")]);

		const registry = new ToolRegistry();
		const messages: Message[] = [{ role: "user", content: "Go" }];

		const events = await collectEvents(runArchitectPipeline(architectProvider, editorProvider, registry, messages));

		// Should have usage + assistant_message + plan_complete from architect
		// Then usage + assistant_message from editor
		const types = events.map((e) => e.type);
		expect(types).toContain("plan_complete");
		expect(types.filter((t) => t === "assistant_message").length).toBe(2);
		expect(types.filter((t) => t === "usage").length).toBe(2);
	});

	test("handles architect error gracefully", async () => {
		const architectProvider: Provider = {
			async send(): Promise<ChatCompletionResponse> {
				throw new Error("Architect model failed");
			},
			async *stream(): AsyncGenerator<StreamChunk> {
				throw new Error("unused");
			},
			with() {
				return architectProvider;
			},
		};
		const editorProvider = mockProvider([mockTextResponse("Should not run")]);

		const registry = new ToolRegistry();
		const messages: Message[] = [{ role: "user", content: "Go" }];

		const events = await collectEvents(runArchitectPipeline(architectProvider, editorProvider, registry, messages));

		const errorEvent = events.find((e) => e.type === "error");
		expect(errorEvent).toBeDefined();
	});
});
