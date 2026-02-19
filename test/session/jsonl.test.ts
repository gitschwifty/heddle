import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import {
	appendMessage,
	loadSession,
	loadSessionMeta,
	type SessionMeta,
	writeSessionMeta,
} from "../../src/session/jsonl.ts";
import type { Message } from "../../src/types.ts";

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

		test("includes ISO timestamp on each message", async () => {
			const filePath = testPath("timestamp.jsonl");
			const msg: Message = { role: "user", content: "Hello" };

			await appendMessage(filePath, msg);

			const content = await Bun.file(filePath).text();
			const parsed = JSON.parse(content.trim());
			expect(parsed.timestamp).toBeDefined();
			// Should be a valid ISO string
			expect(new Date(parsed.timestamp).toISOString()).toBe(parsed.timestamp);
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
			expect(parsed.timestamp).toBeDefined();
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
			expect(parsed.timestamp).toBeDefined();
		});

		test("creates parent directories if needed", async () => {
			const filePath = join(TEST_DIR, "nested", "deep", "session.jsonl");
			const msg: Message = { role: "user", content: "Hello" };

			await appendMessage(filePath, msg);

			expect(existsSync(filePath)).toBe(true);
		});
	});

	describe("writeSessionMeta() / loadSessionMeta()", () => {
		test("writes session_meta as first line", async () => {
			const filePath = testPath("meta.jsonl");
			const meta: SessionMeta = {
				type: "session_meta",
				id: "test-uuid-1234",
				cwd: "/home/user/repos/heddle",
				model: "moonshotai/kimi-k2.5",
				created: "2026-02-18T20:01:46Z",
				heddle_version: "0.1.0",
			};

			await writeSessionMeta(filePath, meta);

			const content = await Bun.file(filePath).text();
			const parsed = JSON.parse(content.trim());
			expect(parsed.type).toBe("session_meta");
			expect(parsed.id).toBe("test-uuid-1234");
			expect(parsed.cwd).toBe("/home/user/repos/heddle");
			expect(parsed.model).toBe("moonshotai/kimi-k2.5");
		});

		test("loadSessionMeta reads back the header", async () => {
			const filePath = testPath("meta-load.jsonl");
			const meta: SessionMeta = {
				type: "session_meta",
				id: "test-uuid-5678",
				cwd: "/home/user/repos/heddle",
				model: "test-model",
				created: "2026-02-18T20:01:46Z",
				heddle_version: "0.1.0",
			};

			await writeSessionMeta(filePath, meta);
			await appendMessage(filePath, { role: "user", content: "Hello" });

			const loaded = await loadSessionMeta(filePath);
			expect(loaded).not.toBeNull();
			expect(loaded!.id).toBe("test-uuid-5678");
			expect(loaded!.model).toBe("test-model");
		});

		test("loadSessionMeta returns null for missing file", async () => {
			const loaded = await loadSessionMeta(testPath("nope.jsonl"));
			expect(loaded).toBeNull();
		});

		test("loadSessionMeta returns null for file without session_meta", async () => {
			const filePath = testPath("no-meta.jsonl");
			await appendMessage(filePath, { role: "user", content: "Hello" });

			const loaded = await loadSessionMeta(filePath);
			expect(loaded).toBeNull();
		});

		test("session_meta supports extra fields", async () => {
			const filePath = testPath("meta-extra.jsonl");
			const meta: SessionMeta = {
				type: "session_meta",
				id: "test-uuid",
				cwd: "/tmp",
				model: "test",
				created: "2026-02-18T20:01:46Z",
				heddle_version: "0.1.0",
				name: "my-session",
				custom_field: 42,
			};

			await writeSessionMeta(filePath, meta);
			const loaded = await loadSessionMeta(filePath);
			expect(loaded!.name).toBe("my-session");
			expect(loaded!.custom_field).toBe(42);
		});
	});

	describe("loadSession()", () => {
		test("returns empty array for missing file", async () => {
			const messages = await loadSession(testPath("nonexistent.jsonl"));
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
			await appendMessage(filePath, { role: "user", content: "Hello" });

			const messages = await loadSession(filePath);

			expect(messages).toHaveLength(1);
			expect(messages[0]!.role).toBe("user");
		});

		test("loads multiple messages in order", async () => {
			const filePath = testPath("multiple.jsonl");
			await appendMessage(filePath, { role: "user", content: "Hello" });
			await appendMessage(filePath, { role: "assistant", content: "Hi!" });
			await appendMessage(filePath, { role: "user", content: "How are you?" });

			const messages = await loadSession(filePath);

			expect(messages).toHaveLength(3);
			expect(messages[0]!.role).toBe("user");
			expect(messages[1]!.role).toBe("assistant");
			expect(messages[2]!.role).toBe("user");
		});

		test("skips session_meta line when loading messages", async () => {
			const filePath = testPath("with-meta.jsonl");
			await writeSessionMeta(filePath, {
				type: "session_meta",
				id: "test-uuid",
				cwd: "/tmp",
				model: "test",
				created: "2026-02-18T20:01:46Z",
				heddle_version: "0.1.0",
			});
			await appendMessage(filePath, { role: "user", content: "Hello" });
			await appendMessage(filePath, { role: "assistant", content: "Hi!" });

			const messages = await loadSession(filePath);

			expect(messages).toHaveLength(2);
			expect(messages[0]!.role).toBe("user");
			expect(messages[1]!.role).toBe("assistant");
		});

		test("skips blank lines gracefully", async () => {
			const filePath = testPath("blanks.jsonl");
			await appendMessage(filePath, { role: "user", content: "Hello" });
			// Manually add blank lines
			const { appendFile } = require("node:fs/promises");
			await appendFile(filePath, "\n\n", "utf-8");

			const messages = await loadSession(filePath);
			expect(messages).toHaveLength(1);
		});
	});

	describe("round-trip", () => {
		test("write then read back preserves messages", async () => {
			const filePath = testPath("roundtrip.jsonl");

			await writeSessionMeta(filePath, {
				type: "session_meta",
				id: "rt-uuid",
				cwd: "/tmp",
				model: "test",
				created: "2026-02-18T20:01:46Z",
				heddle_version: "0.1.0",
			});

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
				expect(loaded[i]!.role).toBe(msgs[i]!.role);
			}

			// Verify meta is separate
			const meta = await loadSessionMeta(filePath);
			expect(meta!.id).toBe("rt-uuid");
		});
	});
});
