import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { appendMessage, loadSession } from "../../src/session/jsonl.ts";
import type { Message } from "../../src/types.ts";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";

const TEST_DIR = join(import.meta.dir, ".tmp-session-test");

function testPath(name: string): string {
	return join(TEST_DIR, name);
}

describe("JSONL session logging", () => {
	beforeEach(() => {
		mkdirSync(TEST_DIR, { recursive: true });
	});

	afterEach(() => {
		if (existsSync(TEST_DIR)) {
			rmSync(TEST_DIR, { recursive: true, force: true });
		}
	});

	describe("appendMessage()", () => {
		test("creates file if it does not exist", async () => {
			const filePath = testPath("new-session.jsonl");
			const msg: Message = { role: "user", content: "Hello" };

			await appendMessage(filePath, msg);

			expect(existsSync(filePath)).toBe(true);
		});

		test("appends a user message as a JSON line", async () => {
			const filePath = testPath("append.jsonl");
			const msg: Message = { role: "user", content: "Hello" };

			await appendMessage(filePath, msg);

			const content = await Bun.file(filePath).text();
			const parsed = JSON.parse(content.trim());
			expect(parsed.role).toBe("user");
			expect(parsed.content).toBe("Hello");
		});

		test("appends multiple messages as separate lines", async () => {
			const filePath = testPath("multi.jsonl");
			const msg1: Message = { role: "user", content: "Hello" };
			const msg2: Message = { role: "assistant", content: "Hi there!" };

			await appendMessage(filePath, msg1);
			await appendMessage(filePath, msg2);

			const content = await Bun.file(filePath).text();
			const lines = content.trim().split("\n");
			expect(lines).toHaveLength(2);
			expect(JSON.parse(lines[0]!).role).toBe("user");
			expect(JSON.parse(lines[1]!).role).toBe("assistant");
		});

		test("handles assistant message with tool_calls", async () => {
			const filePath = testPath("tool-calls.jsonl");
			const msg: Message = {
				role: "assistant",
				content: null,
				tool_calls: [
					{
						id: "call_1",
						type: "function",
						function: { name: "read_file", arguments: '{"path":"/tmp/test.txt"}' },
					},
				],
			};

			await appendMessage(filePath, msg);

			const content = await Bun.file(filePath).text();
			const parsed = JSON.parse(content.trim());
			expect(parsed.role).toBe("assistant");
			expect(parsed.tool_calls).toHaveLength(1);
			expect(parsed.tool_calls[0].function.name).toBe("read_file");
		});

		test("handles tool result message", async () => {
			const filePath = testPath("tool-result.jsonl");
			const msg: Message = {
				role: "tool",
				tool_call_id: "call_1",
				content: "file contents here",
			};

			await appendMessage(filePath, msg);

			const content = await Bun.file(filePath).text();
			const parsed = JSON.parse(content.trim());
			expect(parsed.role).toBe("tool");
			expect(parsed.tool_call_id).toBe("call_1");
		});

		test("creates parent directories if needed", async () => {
			const filePath = join(TEST_DIR, "nested", "deep", "session.jsonl");
			const msg: Message = { role: "user", content: "Hello" };

			await appendMessage(filePath, msg);

			expect(existsSync(filePath)).toBe(true);
		});
	});

	describe("loadSession()", () => {
		test("returns empty array for missing file", async () => {
			const filePath = testPath("nonexistent.jsonl");

			const messages = await loadSession(filePath);

			expect(messages).toEqual([]);
		});

		test("returns empty array for empty file", async () => {
			const filePath = testPath("empty.jsonl");
			await Bun.write(filePath, "");

			const messages = await loadSession(filePath);

			expect(messages).toEqual([]);
		});

		test("loads single message", async () => {
			const filePath = testPath("single.jsonl");
			const msg: Message = { role: "user", content: "Hello" };
			await Bun.write(filePath, JSON.stringify(msg) + "\n");

			const messages = await loadSession(filePath);

			expect(messages).toHaveLength(1);
			expect(messages[0]!.role).toBe("user");
			expect((messages[0] as { content: string }).content).toBe("Hello");
		});

		test("loads multiple messages in order", async () => {
			const filePath = testPath("multiple.jsonl");
			const msgs: Message[] = [
				{ role: "user", content: "Hello" },
				{ role: "assistant", content: "Hi!" },
				{ role: "user", content: "How are you?" },
			];
			const content = msgs.map((m) => JSON.stringify(m)).join("\n") + "\n";
			await Bun.write(filePath, content);

			const messages = await loadSession(filePath);

			expect(messages).toHaveLength(3);
			expect(messages[0]!.role).toBe("user");
			expect(messages[1]!.role).toBe("assistant");
			expect(messages[2]!.role).toBe("user");
		});

		test("skips blank lines gracefully", async () => {
			const filePath = testPath("blanks.jsonl");
			const msg: Message = { role: "user", content: "Hello" };
			await Bun.write(filePath, `${JSON.stringify(msg)}\n\n\n`);

			const messages = await loadSession(filePath);

			expect(messages).toHaveLength(1);
		});
	});

	describe("round-trip", () => {
		test("write then read back preserves messages", async () => {
			const filePath = testPath("roundtrip.jsonl");
			const msgs: Message[] = [
				{ role: "system", content: "You are a helpful assistant." },
				{ role: "user", content: "Hello" },
				{
					role: "assistant",
					content: null,
					tool_calls: [
						{
							id: "call_1",
							type: "function",
							function: { name: "read_file", arguments: '{"path":"/tmp/a.txt"}' },
						},
					],
				},
				{ role: "tool", tool_call_id: "call_1", content: "file content" },
				{ role: "assistant", content: "Here is the file content." },
			];

			for (const msg of msgs) {
				await appendMessage(filePath, msg);
			}

			const loaded = await loadSession(filePath);

			expect(loaded).toHaveLength(msgs.length);
			for (let i = 0; i < msgs.length; i++) {
				expect(loaded[i]).toEqual(msgs[i]);
			}
		});
	});
});
