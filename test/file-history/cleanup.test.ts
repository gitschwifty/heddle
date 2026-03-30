import { describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-cleanup-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

describe("runFileHistoryCleanup", () => {
	test("removes files older than maxAge", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			// Create a fake project backup dir
			const filePath = "/fake/file.txt";
			const histDir = getFileHistoryDir(dir, filePath);
			await mkdir(histDir, { recursive: true });

			// Old backup (timestamp way in the past)
			const oldTs = Date.now() - 31 * 24 * 60 * 60 * 1000; // 31 days ago
			await writeFile(join(histDir, `${oldTs}.bak`), "old data");

			// Recent backup
			const recentTs = Date.now() - 1000; // 1 second ago
			await writeFile(join(histDir, `${recentTs}.bak`), "recent data");

			const stats = await runFileHistoryCleanup({
				maxAge: 30 * 24 * 60 * 60 * 1000, // 30 days
				maxSize: 100 * 1024 * 1024,
				projectPath: dir,
			});

			expect(stats.filesRemoved).toBe(1);
			expect(stats.bytesFreed).toBeGreaterThan(0);

			// Recent backup should still exist
			const { existsSync } = await import("node:fs");
			expect(existsSync(join(histDir, `${recentTs}.bak`))).toBe(true);
			expect(existsSync(join(histDir, `${oldTs}.bak`))).toBe(false);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("respects maxSize limit", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			const filePath = "/fake/bigfile.txt";
			const histDir = getFileHistoryDir(dir, filePath);
			await mkdir(histDir, { recursive: true });

			// Create several backups, all recent
			const now = Date.now();
			const bigContent = "x".repeat(1000);
			await writeFile(join(histDir, `${now - 3000}.bak`), bigContent);
			await writeFile(join(histDir, `${now - 2000}.bak`), bigContent);
			await writeFile(join(histDir, `${now - 1000}.bak`), bigContent);

			// maxSize = 2500 bytes, so we have 3000 bytes total — should remove oldest
			const stats = await runFileHistoryCleanup({
				maxAge: 30 * 24 * 60 * 60 * 1000,
				maxSize: 2500,
				projectPath: dir,
			});

			expect(stats.filesRemoved).toBeGreaterThanOrEqual(1);
			expect(stats.bytesFreed).toBeGreaterThanOrEqual(1000);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("returns correct stats", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			// No backups at all — should be a no-op
			const stats = await runFileHistoryCleanup({
				maxAge: 30 * 24 * 60 * 60 * 1000,
				maxSize: 100 * 1024 * 1024,
				projectPath: dir,
			});

			expect(stats.filesRemoved).toBe(0);
			expect(stats.bytesFreed).toBe(0);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("no-op on empty directory", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			// Create file-history dir but leave it empty
			const baseDir = getFileHistoryDir(dir);
			await mkdir(baseDir, { recursive: true });

			const stats = await runFileHistoryCleanup({
				maxAge: 30 * 24 * 60 * 60 * 1000,
				maxSize: 100 * 1024 * 1024,
				projectPath: dir,
			});

			expect(stats.filesRemoved).toBe(0);
			expect(stats.bytesFreed).toBe(0);

			delete process.env.HEDDLE_HOME;
		});
	});
});
