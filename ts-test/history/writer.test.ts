import { describe, expect, test } from "bun:test";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { appendHistoryEntry, type HistoryEntry } from "../../src/history/writer.ts";

async function withHistoryDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-history-writer-"));
	const prev = process.env.HEDDLE_HOME;
	process.env.HEDDLE_HOME = dir;
	try {
		await fn(dir);
	} finally {
		if (prev === undefined) {
			delete process.env.HEDDLE_HOME;
		} else {
			process.env.HEDDLE_HOME = prev;
		}
		await rm(dir, { recursive: true });
	}
}

describe("history writer", () => {
	test("appends a single entry as JSONL", async () => {
		await withHistoryDir(async (dir) => {
			const entry: HistoryEntry = {
				timestamp: "2026-03-29T12:00:00.000Z",
				session_id: "test-session-1",
				project: "/tmp/project",
				message_preview: "hello world",
				content_type: "text",
			};
			await appendHistoryEntry(entry);

			const historyPath = join(dir, "history.jsonl");
			const content = await readFile(historyPath, "utf-8");
			const lines = content.trim().split("\n");
			expect(lines).toHaveLength(1);

			const parsed = JSON.parse(lines[0]!);
			expect(parsed.timestamp).toBe("2026-03-29T12:00:00.000Z");
			expect(parsed.session_id).toBe("test-session-1");
			expect(parsed.project).toBe("/tmp/project");
			expect(parsed.message_preview).toBe("hello world");
			expect(parsed.content_type).toBe("text");
		});
	});

	test("appends multiple entries as separate lines", async () => {
		await withHistoryDir(async (dir) => {
			await appendHistoryEntry({
				timestamp: "2026-03-29T12:00:00.000Z",
				session_id: "s1",
				project: "/tmp/project",
				message_preview: "first",
				content_type: "text",
			});
			await appendHistoryEntry({
				timestamp: "2026-03-29T12:01:00.000Z",
				session_id: "s1",
				project: "/tmp/project",
				message_preview: "second",
				content_type: "mention",
			});

			const historyPath = join(dir, "history.jsonl");
			const content = await readFile(historyPath, "utf-8");
			const lines = content.trim().split("\n");
			expect(lines).toHaveLength(2);
			expect(JSON.parse(lines[1]!).content_type).toBe("mention");
		});
	});

	test("handles shell content type", async () => {
		await withHistoryDir(async (dir) => {
			await appendHistoryEntry({
				timestamp: "2026-03-29T12:02:00.000Z",
				session_id: "s3",
				project: "/tmp/project",
				message_preview: "run ls",
				content_type: "shell",
			});

			const historyPath = join(dir, "history.jsonl");
			const content = await readFile(historyPath, "utf-8");
			const lines = content.trim().split("\n");
			expect(JSON.parse(lines[0]!).content_type).toBe("shell");
		});
	});
});
