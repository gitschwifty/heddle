import { describe, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { forkSession } from "../../src/session/fork.ts";

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-fork-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

function sessionLine(overrides: Record<string, unknown> = {}): string {
	return JSON.stringify({
		type: "session_meta",
		id: "original-id",
		cwd: "/tmp/test",
		model: "test-model",
		created: "2026-01-15T10:00:00.000Z",
		heddle_version: "0.1.0",
		...overrides,
	});
}

function messageLine(role: string, content: string): string {
	return JSON.stringify({ role, content, timestamp: "2026-01-15T10:00:01.000Z" });
}

function parseLines(content: string): Record<string, unknown>[] {
	return content
		.trim()
		.split("\n")
		.filter((l) => l.trim())
		.map((l) => JSON.parse(l));
}

describe("forkSession", () => {
	test("creates new file with forked_from in meta", async () => {
		await withTmpDir(async (dir) => {
			const sourceFile = join(dir, "original.jsonl");
			const lines = [sessionLine(), messageLine("user", "hello")].join("\n");
			await writeFile(sourceFile, `${lines}\n`);

			const result = await forkSession(sourceFile);
			expect(existsSync(result.sessionFile)).toBe(true);
			expect(result.sessionId).not.toBe("original-id");

			const content = await readFile(result.sessionFile, "utf-8");
			const parsed = parseLines(content);
			const meta = parsed[0]!;
			expect(meta.type).toBe("session_meta");
			expect(meta.forked_from).toBe("original-id");
			expect(meta.id).toBe(result.sessionId);
		});
	});

	test("copies all messages from source", async () => {
		await withTmpDir(async (dir) => {
			const sourceFile = join(dir, "source.jsonl");
			const lines = [
				sessionLine(),
				messageLine("system", "system prompt"),
				messageLine("user", "hello"),
				messageLine("assistant", "hi there"),
			].join("\n");
			await writeFile(sourceFile, `${lines}\n`);

			const result = await forkSession(sourceFile);
			const content = await readFile(result.sessionFile, "utf-8");
			const parsed = parseLines(content);

			// meta + 3 messages
			expect(parsed).toHaveLength(4);
			expect(parsed[1]!.role).toBe("system");
			expect(parsed[2]!.role).toBe("user");
			expect(parsed[3]!.role).toBe("assistant");
		});
	});

	test("truncates messages with upToMessage option", async () => {
		await withTmpDir(async (dir) => {
			const sourceFile = join(dir, "trunc.jsonl");
			const lines = [
				sessionLine(),
				messageLine("system", "system prompt"),
				messageLine("user", "first"),
				messageLine("assistant", "response 1"),
				messageLine("user", "second"),
				messageLine("assistant", "response 2"),
			].join("\n");
			await writeFile(sourceFile, `${lines}\n`);

			const result = await forkSession(sourceFile, { upToMessage: 2 });
			const content = await readFile(result.sessionFile, "utf-8");
			const parsed = parseLines(content);

			// meta + 2 messages
			expect(parsed).toHaveLength(3);
			expect(parsed[1]!.role).toBe("system");
			expect(parsed[2]!.role).toBe("user");
		});
	});

	test("preserves original session unchanged", async () => {
		await withTmpDir(async (dir) => {
			const sourceFile = join(dir, "preserve.jsonl");
			const originalContent = `${[sessionLine(), messageLine("user", "hello")].join("\n")}\n`;
			await writeFile(sourceFile, originalContent);

			await forkSession(sourceFile);

			const afterFork = await readFile(sourceFile, "utf-8");
			expect(afterFork).toBe(originalContent);
		});
	});

	test("forked file is in same directory as source", async () => {
		await withTmpDir(async (dir) => {
			const sourceFile = join(dir, "source.jsonl");
			await writeFile(sourceFile, `${sessionLine()}\n`);

			const result = await forkSession(sourceFile);
			const resultDir = result.sessionFile.substring(0, result.sessionFile.lastIndexOf("/"));
			expect(resultDir).toBe(dir);
		});
	});
});
