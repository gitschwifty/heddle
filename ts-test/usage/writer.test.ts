import { describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { SessionMetrics } from "../../src/usage/collector.ts";
import type { UsageRecord } from "../../src/usage/writer.ts";
import { writeUsageRecord } from "../../src/usage/writer.ts";

function makeMetrics(overrides: Partial<SessionMetrics> = {}): SessionMetrics {
	return {
		messageCount: { user: 0, assistant: 0 },
		toolCalls: {},
		errors: { tool: 0, provider: 0 },
		tokens: { input: 0, output: 0 },
		turns: 0,
		...overrides,
	};
}

function makeRecord(overrides: Partial<UsageRecord> = {}): UsageRecord {
	return {
		session_id: "test-session-001",
		project: "/tmp/test-project",
		created: "2026-01-15T10:00:00.000Z",
		ended: "2026-01-15T10:05:00.000Z",
		duration_ms: 300000,
		metrics: makeMetrics(),
		...overrides,
	};
}

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-writer-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

describe("writeUsageRecord", () => {
	test("writes usage record to correct path", async () => {
		await withTmpDir(async (dir) => {
			const record = makeRecord({ session_id: "abc-123" });
			await writeUsageRecord(record, dir);

			const filePath = join(dir, "usage", "abc-123.json");
			const content = await readFile(filePath, "utf-8");
			const parsed = JSON.parse(content);
			expect(parsed.session_id).toBe("abc-123");
			expect(parsed.project).toBe("/tmp/test-project");
			expect(parsed.duration_ms).toBe(300000);
		});
	});

	test("creates usage directory if it does not exist", async () => {
		await withTmpDir(async (dir) => {
			const record = makeRecord();
			await writeUsageRecord(record, dir);

			const filePath = join(dir, "usage", "test-session-001.json");
			const content = await readFile(filePath, "utf-8");
			expect(JSON.parse(content).session_id).toBe("test-session-001");
		});
	});

	test("writes metrics data correctly", async () => {
		await withTmpDir(async (dir) => {
			const record = makeRecord({
				metrics: makeMetrics({
					messageCount: { user: 5, assistant: 4 },
					toolCalls: { read: 3, write: 1 },
					errors: { tool: 1, provider: 0 },
					tokens: { input: 1000, output: 500 },
					turns: 5,
				}),
			});
			await writeUsageRecord(record, dir);

			const filePath = join(dir, "usage", "test-session-001.json");
			const parsed = JSON.parse(await readFile(filePath, "utf-8"));
			expect(parsed.metrics.messageCount).toEqual({ user: 5, assistant: 4 });
			expect(parsed.metrics.toolCalls).toEqual({ read: 3, write: 1 });
			expect(parsed.metrics.tokens).toEqual({ input: 1000, output: 500 });
			expect(parsed.metrics.turns).toBe(5);
		});
	});

	test("includes cost_usd when provided", async () => {
		await withTmpDir(async (dir) => {
			const record = makeRecord({ cost_usd: 0.042 });
			await writeUsageRecord(record, dir);

			const filePath = join(dir, "usage", "test-session-001.json");
			const parsed = JSON.parse(await readFile(filePath, "utf-8"));
			expect(parsed.cost_usd).toBe(0.042);
		});
	});

	test("omits cost_usd when undefined", async () => {
		await withTmpDir(async (dir) => {
			const record = makeRecord();
			await writeUsageRecord(record, dir);

			const filePath = join(dir, "usage", "test-session-001.json");
			const parsed = JSON.parse(await readFile(filePath, "utf-8"));
			expect(parsed.cost_usd).toBeUndefined();
		});
	});

	test("writes to usage subdirectory under project dir", async () => {
		await withTmpDir(async (dir) => {
			// Pre-create the project dir structure
			const projectDir = join(dir, "projects", "my-project");
			await mkdir(projectDir, { recursive: true });

			const record = makeRecord({ session_id: "nested-test" });
			await writeUsageRecord(record, projectDir);

			const filePath = join(projectDir, "usage", "nested-test.json");
			const content = await readFile(filePath, "utf-8");
			expect(JSON.parse(content).session_id).toBe("nested-test");
		});
	});
});
