import { describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { SessionMetrics } from "../../src/usage/collector.ts";
import { aggregateUsage, readUsageRecord } from "../../src/usage/reader.ts";
import type { UsageRecord } from "../../src/usage/writer.ts";

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
	const dir = await mkdtemp(join(tmpdir(), "heddle-reader-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

async function writeRecord(dir: string, record: UsageRecord): Promise<void> {
	const usageDir = join(dir, "usage");
	await mkdir(usageDir, { recursive: true });
	await writeFile(join(usageDir, `${record.session_id}.json`), JSON.stringify(record), "utf-8");
}

describe("readUsageRecord", () => {
	test("reads a single usage record by session id", async () => {
		await withTmpDir(async (dir) => {
			const record = makeRecord({ session_id: "abc-123" });
			await writeRecord(dir, record);

			const result = await readUsageRecord("abc-123", dir);
			expect(result).not.toBeNull();
			expect(result!.session_id).toBe("abc-123");
			expect(result!.project).toBe("/tmp/test-project");
			expect(result!.duration_ms).toBe(300000);
		});
	});

	test("returns null for non-existent session id", async () => {
		await withTmpDir(async (dir) => {
			const result = await readUsageRecord("does-not-exist", dir);
			expect(result).toBeNull();
		});
	});

	test("returns null when usage directory does not exist", async () => {
		await withTmpDir(async (dir) => {
			const result = await readUsageRecord("anything", dir);
			expect(result).toBeNull();
		});
	});

	test("reads record with full metrics", async () => {
		await withTmpDir(async (dir) => {
			const record = makeRecord({
				session_id: "full-metrics",
				metrics: makeMetrics({
					messageCount: { user: 10, assistant: 9 },
					toolCalls: { read: 5, write: 3, glob: 2 },
					errors: { tool: 1, provider: 2 },
					tokens: { input: 5000, output: 2000 },
					turns: 10,
				}),
				cost_usd: 0.15,
			});
			await writeRecord(dir, record);

			const result = await readUsageRecord("full-metrics", dir);
			expect(result).not.toBeNull();
			expect(result!.metrics.toolCalls).toEqual({ read: 5, write: 3, glob: 2 });
			expect(result!.cost_usd).toBe(0.15);
		});
	});
});

describe("aggregateUsage", () => {
	test("aggregates across multiple records", async () => {
		await withTmpDir(async (dir) => {
			await writeRecord(
				dir,
				makeRecord({
					session_id: "s1",
					metrics: makeMetrics({
						tokens: { input: 1000, output: 500 },
						toolCalls: { read: 3, write: 1 },
					}),
					cost_usd: 0.01,
				}),
			);
			await writeRecord(
				dir,
				makeRecord({
					session_id: "s2",
					metrics: makeMetrics({
						tokens: { input: 2000, output: 1000 },
						toolCalls: { read: 2, edit: 4 },
					}),
					cost_usd: 0.02,
				}),
			);

			const agg = await aggregateUsage(dir);
			expect(agg.totalSessions).toBe(2);
			expect(agg.totalTokens).toEqual({ input: 3000, output: 1500 });
			expect(agg.totalCost).toBeCloseTo(0.03);
			expect(agg.toolCalls).toEqual({ read: 5, write: 1, edit: 4 });
		});
	});

	test("returns zeros for empty usage directory", async () => {
		await withTmpDir(async (dir) => {
			await mkdir(join(dir, "usage"), { recursive: true });
			const agg = await aggregateUsage(dir);
			expect(agg.totalSessions).toBe(0);
			expect(agg.totalTokens).toEqual({ input: 0, output: 0 });
			expect(agg.totalCost).toBe(0);
			expect(agg.toolCalls).toEqual({});
		});
	});

	test("returns zeros when usage directory does not exist", async () => {
		await withTmpDir(async (dir) => {
			const agg = await aggregateUsage(dir);
			expect(agg.totalSessions).toBe(0);
			expect(agg.totalTokens).toEqual({ input: 0, output: 0 });
			expect(agg.totalCost).toBe(0);
			expect(agg.toolCalls).toEqual({});
		});
	});

	test("handles records without cost_usd", async () => {
		await withTmpDir(async (dir) => {
			await writeRecord(
				dir,
				makeRecord({
					session_id: "no-cost",
					metrics: makeMetrics({ tokens: { input: 500, output: 200 } }),
					// no cost_usd
				}),
			);

			const agg = await aggregateUsage(dir);
			expect(agg.totalSessions).toBe(1);
			expect(agg.totalTokens).toEqual({ input: 500, output: 200 });
			expect(agg.totalCost).toBe(0);
		});
	});

	test("skips non-json files in usage directory", async () => {
		await withTmpDir(async (dir) => {
			const usageDir = join(dir, "usage");
			await mkdir(usageDir, { recursive: true });
			await writeFile(join(usageDir, "README.md"), "# Usage data", "utf-8");
			await writeRecord(
				dir,
				makeRecord({
					session_id: "valid",
					metrics: makeMetrics({ tokens: { input: 100, output: 50 } }),
					cost_usd: 0.005,
				}),
			);

			const agg = await aggregateUsage(dir);
			expect(agg.totalSessions).toBe(1);
			expect(agg.totalTokens).toEqual({ input: 100, output: 50 });
		});
	});

	test("skips malformed json files gracefully", async () => {
		await withTmpDir(async (dir) => {
			const usageDir = join(dir, "usage");
			await mkdir(usageDir, { recursive: true });
			await writeFile(join(usageDir, "corrupt.json"), "not valid json{{{", "utf-8");
			await writeRecord(
				dir,
				makeRecord({
					session_id: "good",
					metrics: makeMetrics({ tokens: { input: 100, output: 50 } }),
					cost_usd: 0.01,
				}),
			);

			const agg = await aggregateUsage(dir);
			expect(agg.totalSessions).toBe(1);
			expect(agg.totalCost).toBeCloseTo(0.01);
		});
	});
});
