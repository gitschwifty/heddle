import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Type } from "@sinclair/typebox";
import { runAgentLoop } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import type { Provider } from "../../src/provider/types.ts";
import { appendMessage, loadSession, writeSessionMeta } from "../../src/session/jsonl.ts";
import { createEditTool } from "../../src/tools/edit.ts";
import { createReadTool } from "../../src/tools/read.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import { createWriteTool } from "../../src/tools/write.ts";
import type { ChatCompletionResponse, Message, StreamChunk, ToolDefinition } from "../../src/types.ts";
import { mockTextResponse, mockToolCallResponse } from "../mocks/openrouter.ts";

function mockProvider(responses: ChatCompletionResponse[]): Provider {
	let callIndex = 0;
	return {
		async send(_messages: Message[], _tools?: ToolDefinition[]): Promise<ChatCompletionResponse> {
			const resp = responses[callIndex];
			if (!resp) throw new Error("No more mock responses");
			callIndex++;
			return resp;
		},
		stream(): AsyncGenerator<StreamChunk> {
			throw new Error("stream not used");
		},
	};
}

async function collectEvents(gen: AsyncGenerator<AgentEvent>): Promise<AgentEvent[]> {
	const events: AgentEvent[] = [];
	for await (const event of gen) events.push(event);
	return events;
}

describe("Multi-turn conversations", () => {
	let dir: string;

	beforeEach(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-multiturn-"));
	});

	afterEach(async () => {
		await rm(dir, { recursive: true });
	});

	test("message accumulation across 2 turns", async () => {
		const messages: Message[] = [{ role: "system", content: "You are a helpful assistant." }];

		// Turn 1: user asks, model calls a tool, then responds with text
		messages.push({ role: "user", content: "What is 2+2?" });

		const turn1Provider = mockProvider([
			mockToolCallResponse([{ name: "echo", arguments: { text: "4" } }]),
			mockTextResponse("The answer is 4."),
		]);

		const registry = new ToolRegistry();
		registry.register({
			name: "echo",
			description: "Returns the input",
			parameters: Type.Object({ text: Type.String() }),
			execute: async (params) => (params as { text: string }).text,
		});

		await collectEvents(runAgentLoop(turn1Provider, registry, messages));

		// system + user + assistant(tool_call) + tool_result + assistant(text) = 5
		expect(messages).toHaveLength(5);
		expect(messages[0]?.role).toBe("system");
		expect(messages[1]?.role).toBe("user");
		expect(messages[2]?.role).toBe("assistant");
		expect(messages[3]?.role).toBe("tool");
		expect(messages[4]?.role).toBe("assistant");

		// Turn 2: another user message, model responds with text only
		messages.push({ role: "user", content: "And 3+3?" });

		const turn2Provider = mockProvider([mockTextResponse("The answer is 6.")]);

		await collectEvents(runAgentLoop(turn2Provider, registry, messages));

		// 5 + user + assistant(text) = 7
		expect(messages).toHaveLength(7);
		expect(messages[5]?.role).toBe("user");
		expect(messages[6]?.role).toBe("assistant");

		// Verify full role sequence
		const roles = messages.map((m) => m.role);
		expect(roles).toEqual(["system", "user", "assistant", "tool", "assistant", "user", "assistant"]);
	});

	test("context carryover — read then edit across turns", async () => {
		const filePath = join(dir, "data.txt");
		await writeFile(filePath, "count: 0");

		const registry = new ToolRegistry();
		registry.register(createReadTool());
		registry.register(createEditTool());

		const messages: Message[] = [{ role: "system", content: "You are a helpful assistant." }];

		// Turn 1: read the file
		messages.push({ role: "user", content: `Read the file at ${filePath}` });

		const turn1Provider = mockProvider([
			mockToolCallResponse([{ name: "read_file", arguments: { file_path: filePath } }]),
			mockTextResponse("The file contains: count: 0"),
		]);

		await collectEvents(runAgentLoop(turn1Provider, registry, messages));

		// Verify tool result contains file content
		const toolResultMsg = messages.find((m) => m.role === "tool");
		expect(toolResultMsg).toBeDefined();
		if (toolResultMsg?.role === "tool") {
			expect(toolResultMsg.content).toContain("count: 0");
		}

		// Turn 2: edit the file
		messages.push({ role: "user", content: "Change count to 1" });

		const turn2Provider = mockProvider([
			mockToolCallResponse([
				{
					name: "edit_file",
					arguments: { file_path: filePath, old_string: "count: 0", new_string: "count: 1" },
				},
			]),
			mockTextResponse("Done, count is now 1."),
		]);

		await collectEvents(runAgentLoop(turn2Provider, registry, messages));

		// Verify file was actually changed on disk
		const content = await Bun.file(filePath).text();
		expect(content).toBe("count: 1");

		// Verify messages contain both tool results
		const toolResults = messages.filter((m) => m.role === "tool");
		expect(toolResults).toHaveLength(2);
	});

	test("multi-tool chains across 3 turns — write, read, edit", async () => {
		const filePath = join(dir, "chain.txt");

		const registry = new ToolRegistry();
		registry.register(createWriteTool());
		registry.register(createReadTool());
		registry.register(createEditTool());

		const messages: Message[] = [{ role: "system", content: "You are a helpful assistant." }];

		// Turn 1: write a file
		messages.push({ role: "user", content: "Create a file" });

		const turn1Provider = mockProvider([
			mockToolCallResponse([{ name: "write_file", arguments: { file_path: filePath, content: "hello world" } }]),
			mockTextResponse("Created the file."),
		]);

		await collectEvents(runAgentLoop(turn1Provider, registry, messages));

		let content = await Bun.file(filePath).text();
		expect(content).toBe("hello world");

		// Turn 2: read it back
		messages.push({ role: "user", content: "Read the file" });

		const turn2Provider = mockProvider([
			mockToolCallResponse([{ name: "read_file", arguments: { file_path: filePath } }]),
			mockTextResponse("It says: hello world"),
		]);

		await collectEvents(runAgentLoop(turn2Provider, registry, messages));

		const readToolResult = messages.filter((m) => m.role === "tool").pop();
		if (readToolResult?.role === "tool") {
			expect(readToolResult.content).toContain("hello world");
		}

		// Turn 3: edit it
		messages.push({ role: "user", content: "Change hello to goodbye" });

		const turn3Provider = mockProvider([
			mockToolCallResponse([
				{
					name: "edit_file",
					arguments: { file_path: filePath, old_string: "hello", new_string: "goodbye" },
				},
			]),
			mockTextResponse("Changed hello to goodbye."),
		]);

		await collectEvents(runAgentLoop(turn3Provider, registry, messages));

		content = await Bun.file(filePath).text();
		expect(content).toBe("goodbye world");
	});

	test("no duplicate messages across turns", async () => {
		const registry = new ToolRegistry();
		registry.register({
			name: "echo",
			description: "Returns the input",
			parameters: Type.Object({ text: Type.String() }),
			execute: async (params) => (params as { text: string }).text,
		});

		const messages: Message[] = [{ role: "system", content: "You are a helpful assistant." }];

		// Turn 1
		messages.push({ role: "user", content: "Say hello" });
		const turn1Provider = mockProvider([mockTextResponse("Hello!")]);
		await collectEvents(runAgentLoop(turn1Provider, registry, messages));

		// Turn 2
		messages.push({ role: "user", content: "Say goodbye" });
		const turn2Provider = mockProvider([mockTextResponse("Goodbye!")]);
		await collectEvents(runAgentLoop(turn2Provider, registry, messages));

		// Check for duplicates: serialize each message and check uniqueness
		// Note: system messages are inherently unique (only one), and each user/assistant
		// message should have unique content in this test
		const serialized = messages.map((m) => JSON.stringify(m));
		const uniqueSet = new Set(serialized);

		// Every message should be unique (no duplicates)
		expect(serialized.length).toBe(uniqueSet.size);
	});

	test("session round-trip — write to JSONL and reload", async () => {
		const registry = new ToolRegistry();
		registry.register({
			name: "echo",
			description: "Returns the input",
			parameters: Type.Object({ text: Type.String() }),
			execute: async (params) => (params as { text: string }).text,
		});

		const messages: Message[] = [{ role: "system", content: "You are a helpful assistant." }];

		// Turn 1: tool call + text response
		messages.push({ role: "user", content: "Echo ping" });
		const turn1Provider = mockProvider([
			mockToolCallResponse([{ name: "echo", arguments: { text: "ping" } }]),
			mockTextResponse("Got: ping"),
		]);
		await collectEvents(runAgentLoop(turn1Provider, registry, messages));

		// Turn 2: text-only response
		messages.push({ role: "user", content: "Thanks" });
		const turn2Provider = mockProvider([mockTextResponse("You're welcome!")]);
		await collectEvents(runAgentLoop(turn2Provider, registry, messages));

		// Write all messages to JSONL
		const sessionPath = join(dir, "session.jsonl");
		await writeSessionMeta(sessionPath, {
			type: "session_meta",
			id: "test-session-001",
			cwd: dir,
			model: "test-model",
			created: new Date().toISOString(),
			heddle_version: "0.0.1-test",
		});

		for (const msg of messages) {
			await appendMessage(sessionPath, msg);
		}

		// Load messages back from JSONL
		const loaded = await loadSession(sessionPath);

		// Verify message count matches
		expect(loaded).toHaveLength(messages.length);

		// Verify roles match (loadSession strips session_meta, appendMessage adds timestamp)
		const originalRoles = messages.map((m) => m.role);
		const loadedRoles = loaded.map((m) => m.role);
		expect(loadedRoles).toEqual(originalRoles);

		// Verify content matches (ignoring timestamp field added by appendMessage)
		for (let i = 0; i < messages.length; i++) {
			const loadedMsg = loaded[i];
			const originalMsg = messages[i];
			expect(loadedMsg?.role).toBe(originalMsg?.role);
			if (originalMsg && "content" in originalMsg && originalMsg.content !== null) {
				expect((loadedMsg as { content: string }).content).toBe((originalMsg as { content: string }).content);
			}
		}

		// Run another turn on loaded messages with a new provider — verify it works
		const turn3Provider = mockProvider([mockTextResponse("Continuing from loaded session.")]);
		loaded.push({ role: "user", content: "Are you still there?" });
		await collectEvents(runAgentLoop(turn3Provider, registry, loaded));

		// loaded should now have the new user message + assistant response
		expect(loaded).toHaveLength(messages.length + 2);
		expect(loaded[loaded.length - 1]?.role).toBe("assistant");
	});
});
