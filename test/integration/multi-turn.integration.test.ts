import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runAgentLoop } from "../../src/agent/loop.ts";
import type { AgentEvent } from "../../src/agent/types.ts";
import { loadConfig } from "../../src/config/loader.ts";
import { createOpenRouterProvider } from "../../src/provider/openrouter.ts";
import { appendMessage, loadSession, writeSessionMeta } from "../../src/session/jsonl.ts";
import { createEditTool } from "../../src/tools/edit.ts";
import { createReadTool } from "../../src/tools/read.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import { createWriteTool } from "../../src/tools/write.ts";
import type { Message } from "../../src/types.ts";

const INTEGRATION = process.env.HEDDLE_INTEGRATION_TESTS === "1";
const SLOW_TESTS = process.env.HEDDLE_SLOW_TESTS === "1";
const describeIf = INTEGRATION && SLOW_TESTS ? describe : describe.skip;

async function collectEvents(gen: AsyncGenerator<AgentEvent>): Promise<AgentEvent[]> {
	const events: AgentEvent[] = [];
	for await (const event of gen) events.push(event);
	return events;
}

describeIf("Multi-turn integration (real model)", () => {
	// Shared dir — tests use distinct filenames (data.txt, persist.txt, session.jsonl)
	let dir: string;
	let registry: ToolRegistry;

	beforeAll(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-mt-integ-"));
		registry = new ToolRegistry();
		registry.register(createReadTool());
		registry.register(createEditTool());
		registry.register(createWriteTool());
	});

	afterAll(async () => {
		await rm(dir, { recursive: true });
	});

	test("2-turn tool chain — read then edit", async () => {
		const config = loadConfig();
		const provider = createOpenRouterProvider({
			apiKey: config.apiKey ?? process.env.OPENROUTER_API_KEY ?? "",
			model: config.model,
		});

		const filePath = join(dir, "data.txt");
		await Bun.write(filePath, "count: 0");

		const messages: Message[] = [
			{ role: "system", content: "You are a helpful assistant. Use tools when asked to interact with files." },
		];

		// Turn 1: read the file
		messages.push({ role: "user", content: `Read the file at ${filePath} and tell me what's in it.` });
		const turn1Events = await collectEvents(runAgentLoop(provider, registry, messages));

		// Verify messages grew
		expect(messages.length).toBeGreaterThan(2);

		// Verify at least one tool_call for read_file occurred
		const readToolEvents = turn1Events.filter((e) => e.type === "tool_start" && e.name === "read_file");
		expect(readToolEvents.length).toBeGreaterThanOrEqual(1);

		// Verify file content appears in a tool result
		const toolResults = messages.filter((m) => m.role === "tool");
		expect(toolResults.length).toBeGreaterThanOrEqual(1);
		const hasFileContent = toolResults.some((m) => m.role === "tool" && m.content.includes("count: 0"));
		expect(hasFileContent).toBe(true);

		const messagesAfterTurn1 = messages.length;

		// Turn 2: edit the file
		messages.push({ role: "user", content: `Edit the file at ${filePath} to change "count: 0" to "count: 1".` });
		await collectEvents(runAgentLoop(provider, registry, messages));

		// Verify messages grew further
		expect(messages.length).toBeGreaterThan(messagesAfterTurn1);

		// Verify file was actually changed on disk
		const content = await Bun.file(filePath).text();
		expect(content).toContain("count: 1");
	}, 120_000);

	test("session persistence round-trip", async () => {
		const config = loadConfig();
		const provider = createOpenRouterProvider({
			apiKey: config.apiKey ?? process.env.OPENROUTER_API_KEY ?? "",
			model: config.model,
		});

		const filePath = join(dir, "persist.txt");
		await Bun.write(filePath, "original content");

		const messages: Message[] = [
			{ role: "system", content: "You are a helpful assistant. Use tools when asked to interact with files." },
		];

		// Turn 1: read the file
		messages.push({ role: "user", content: `Read the file at ${filePath}.` });
		await collectEvents(runAgentLoop(provider, registry, messages));

		const messageCount = messages.length;
		expect(messageCount).toBeGreaterThan(2);

		// Write messages to JSONL
		const sessionPath = join(dir, "session.jsonl");
		await writeSessionMeta(sessionPath, {
			type: "session_meta",
			id: "integ-test-001",
			cwd: dir,
			model: config.model,
			created: new Date().toISOString(),
			heddle_version: "0.0.1-test",
		});

		for (const msg of messages) {
			await appendMessage(sessionPath, msg);
		}

		// Load from JSONL
		const loaded = await loadSession(sessionPath);

		// Verify message count matches
		expect(loaded).toHaveLength(messageCount);

		// Verify roles alternate correctly
		const roles = loaded.map((m) => m.role);
		expect(roles[0]).toBe("system");
		expect(roles[1]).toBe("user");

		// After system + user, roles should follow assistant/tool patterns
		for (const role of roles.slice(2)) {
			expect(["assistant", "tool", "user"]).toContain(role);
		}

		// No two consecutive user messages (would indicate a bug)
		for (let i = 1; i < roles.length; i++) {
			if (roles[i] === "user") {
				expect(roles[i - 1]).not.toBe("user");
			}
		}
	}, 120_000);
});
