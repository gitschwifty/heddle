import { describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { findSession, listSessions } from "../../src/session/list.ts";

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-list-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

function sessionLine(overrides: Record<string, unknown> = {}): string {
	return JSON.stringify({
		type: "session_meta",
		id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
		cwd: "/tmp/test",
		model: "test-model",
		created: "2026-01-15T10:00:00.000Z",
		heddle_version: "0.1.0",
		...overrides,
	});
}

function messageLine(role: string, content: string): string {
	return JSON.stringify({ role, content, timestamp: new Date().toISOString() });
}

describe("listSessions", () => {
	test("returns empty array for empty directory", async () => {
		await withTmpDir(async (dir) => {
			const sessions = await listSessions(dir);
			expect(sessions).toEqual([]);
		});
	});

	test("parses session metas and counts messages", async () => {
		await withTmpDir(async (dir) => {
			const lines = [
				sessionLine({ id: "id-1", model: "gpt-4" }),
				messageLine("system", "You are helpful"),
				messageLine("user", "Hello there"),
				messageLine("assistant", "Hi!"),
			].join("\n");
			await writeFile(join(dir, "id-1.jsonl"), `${lines}\n`);

			const sessions = await listSessions(dir);
			expect(sessions).toHaveLength(1);
			expect(sessions[0]!.id).toBe("id-1");
			expect(sessions[0]!.model).toBe("gpt-4");
			expect(sessions[0]!.messageCount).toBe(3);
			expect(sessions[0]!.firstUserMessage).toBe("Hello there");
		});
	});

	test("sorts by created descending", async () => {
		await withTmpDir(async (dir) => {
			await writeFile(join(dir, "old.jsonl"), `${sessionLine({ id: "old", created: "2026-01-01T00:00:00.000Z" })}\n`);
			await writeFile(join(dir, "new.jsonl"), `${sessionLine({ id: "new", created: "2026-03-01T00:00:00.000Z" })}\n`);
			await writeFile(join(dir, "mid.jsonl"), `${sessionLine({ id: "mid", created: "2026-02-01T00:00:00.000Z" })}\n`);

			const sessions = await listSessions(dir);
			expect(sessions.map((s) => s.id)).toEqual(["new", "mid", "old"]);
		});
	});

	test("truncates firstUserMessage to 100 chars", async () => {
		await withTmpDir(async (dir) => {
			const longMsg = "x".repeat(200);
			const lines = [sessionLine({ id: "trunc" }), messageLine("user", longMsg)].join("\n");
			await writeFile(join(dir, "trunc.jsonl"), `${lines}\n`);

			const sessions = await listSessions(dir);
			expect(sessions[0]!.firstUserMessage?.length).toBeLessThanOrEqual(100);
		});
	});

	test("skips files without valid session_meta", async () => {
		await withTmpDir(async (dir) => {
			await writeFile(join(dir, "bad.jsonl"), '{"not":"session_meta"}\n');
			await writeFile(join(dir, "good.jsonl"), `${sessionLine({ id: "good" })}\n`);

			const sessions = await listSessions(dir);
			expect(sessions).toHaveLength(1);
			expect(sessions[0]!.id).toBe("good");
		});
	});

	test("reads forkedFrom field", async () => {
		await withTmpDir(async (dir) => {
			await writeFile(join(dir, "forked.jsonl"), `${sessionLine({ id: "forked", forked_from: "parent-id" })}\n`);

			const sessions = await listSessions(dir);
			expect(sessions[0]!.forkedFrom).toBe("parent-id");
		});
	});

	test("reads name from session_name marker", async () => {
		await withTmpDir(async (dir) => {
			const lines = [
				sessionLine({ id: "named" }),
				JSON.stringify({ type: "session_name", name: "my session", timestamp: "2026-01-15T10:00:00.000Z" }),
			].join("\n");
			await writeFile(join(dir, "named.jsonl"), `${lines}\n`);

			const sessions = await listSessions(dir);
			expect(sessions[0]!.name).toBe("my session");
		});
	});
});

describe("findSession", () => {
	test("returns most recent session file when target is empty", async () => {
		await withTmpDir(async (dir) => {
			await writeFile(
				join(dir, "old.jsonl"),
				`${sessionLine({ id: "old-id", created: "2026-01-01T00:00:00.000Z" })}\n`,
			);
			await writeFile(
				join(dir, "new.jsonl"),
				`${sessionLine({ id: "new-id", created: "2026-03-01T00:00:00.000Z" })}\n`,
			);

			const result = await findSession("", dir);
			expect(result).toContain("new.jsonl");
		});
	});

	test("returns null when target is empty and no sessions exist", async () => {
		await withTmpDir(async (dir) => {
			const result = await findSession("", dir);
			expect(result).toBeNull();
		});
	});

	test("finds session by UUID", async () => {
		await withTmpDir(async (dir) => {
			const id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
			await writeFile(join(dir, "test.jsonl"), `${sessionLine({ id })}\n`);

			const result = await findSession(id, dir);
			expect(result).toContain("test.jsonl");
		});
	});

	test("finds session by partial UUID prefix", async () => {
		await withTmpDir(async (dir) => {
			const id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
			await writeFile(join(dir, "test.jsonl"), `${sessionLine({ id })}\n`);

			const result = await findSession("aaaaaaaa", dir);
			expect(result).toContain("test.jsonl");
		});
	});

	test("finds session by name", async () => {
		await withTmpDir(async (dir) => {
			const lines = [sessionLine({ id: "named-id", name: "my-session" })].join("\n");
			await writeFile(join(dir, "named.jsonl"), `${lines}\n`);

			const result = await findSession("my-session", dir);
			expect(result).toContain("named.jsonl");
		});
	});

	test("returns null for nonexistent session", async () => {
		await withTmpDir(async (dir) => {
			await writeFile(join(dir, "test.jsonl"), `${sessionLine()}\n`);

			const result = await findSession("nonexistent-id", dir);
			expect(result).toBeNull();
		});
	});
});
