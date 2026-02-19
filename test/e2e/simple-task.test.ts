import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runAgentLoop } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import type { Provider } from "../../src/provider/types.ts";
import { createEditTool } from "../../src/tools/edit.ts";
import { createReadTool } from "../../src/tools/read.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import { mockTextResponse, mockToolCallResponse } from "../mocks/openrouter.ts";

/** Create a mock provider from ordered responses */
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
			throw new Error("stream not used in E2E tests");
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

describe("E2E: simple task", () => {
	let dir: string;

	beforeEach(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-e2e-"));
	});

	afterEach(async () => {
		await rm(dir, { recursive: true });
	});

	test("read_file tool call → content returned → text response", async () => {
		const filePath = join(dir, "hello.txt");
		await writeFile(filePath, "Hello from the test file!");

		const provider = mockProvider([
			// First: model calls read_file
			mockToolCallResponse([{ name: "read_file", arguments: { file_path: filePath } }]),
			// Second: model responds with text after seeing file contents
			mockTextResponse(`The file contains: "Hello from the test file!"`),
		]);

		const registry = new ToolRegistry();
		registry.register(createReadTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: `Read the file at ${filePath}` }]),
		);

		// Should have: assistant_message (tool call), tool_start, tool_end, assistant_message (text)
		expect(events).toHaveLength(4);
		expect(events[0]?.type).toBe("assistant_message");
		expect(events[1]?.type).toBe("tool_start");
		expect(events[2]?.type).toBe("tool_end");
		expect(events[3]?.type).toBe("assistant_message");

		// Verify the tool actually read the file
		if (events[2]?.type === "tool_end") {
			expect(events[2].result).toContain("Hello from the test file!");
		}

		// Verify the model's final response
		if (events[3]?.type === "assistant_message") {
			expect(events[3].message.content).toContain("Hello from the test file!");
		}
	});

	test("edit_file tool call → file modified → confirmation response", async () => {
		const filePath = join(dir, "code.ts");
		await writeFile(filePath, 'const greeting = "hello";\nconsole.log(greeting);');

		const provider = mockProvider([
			// Model calls edit_file
			mockToolCallResponse([
				{
					name: "edit_file",
					arguments: {
						file_path: filePath,
						old_string: '"hello"',
						new_string: '"world"',
					},
				},
			]),
			// Model confirms the edit
			mockTextResponse('I\'ve updated the greeting from "hello" to "world".'),
		]);

		const registry = new ToolRegistry();
		registry.register(createEditTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "Change the greeting to world" }]),
		);

		expect(events).toHaveLength(4);

		// Verify the file was actually modified
		const content = await Bun.file(filePath).text();
		expect(content).toBe('const greeting = "world";\nconsole.log(greeting);');

		// Verify tool_end reported success
		if (events[2]?.type === "tool_end") {
			expect(events[2].result).toContain("Applied edit");
		}
	});

	test("multi-tool chain: read then edit", async () => {
		const filePath = join(dir, "data.txt");
		await writeFile(filePath, "count: 0");

		const provider = mockProvider([
			// Step 1: model reads the file
			mockToolCallResponse([{ name: "read_file", arguments: { file_path: filePath } }]),
			// Step 2: model edits based on what it read
			mockToolCallResponse([
				{
					name: "edit_file",
					arguments: {
						file_path: filePath,
						old_string: "count: 0",
						new_string: "count: 1",
					},
				},
			]),
			// Step 3: model confirms
			mockTextResponse("I read the file, saw count: 0, and updated it to count: 1."),
		]);

		const registry = new ToolRegistry();
		registry.register(createReadTool());
		registry.register(createEditTool());

		const events = await collectEvents(
			runAgentLoop(provider, registry, [{ role: "user", content: "Increment the count in data.txt" }]),
		);

		// 2 tool rounds + 1 final = 7 events
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

		// File should be updated
		const content = await Bun.file(filePath).text();
		expect(content).toBe("count: 1");
	});
});
