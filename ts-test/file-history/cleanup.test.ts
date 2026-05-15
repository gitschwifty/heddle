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
	test("removes old version files beyond maxVersions", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			const baseDir = getFileHistoryDir(dir);
			const uuidDir = join(baseDir, "fake-uuid-1");
			await mkdir(uuidDir, { recursive: true });

			// Create 5 versions
			for (let i = 1; i <= 5; i++) {
				await writeFile(join(uuidDir, `v${i}.bak`), `content ${i}`);
			}

			const stats = await runFileHistoryCleanup({
				maxSize: 100 * 1024 * 1024,
				projectPath: dir,
			});

			// Should not remove anything by default (no maxAge or size pressure)
			expect(stats.filesRemoved).toBe(0);
		});
	});

	test("respects maxSize limit by removing oldest versions first", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			const baseDir = getFileHistoryDir(dir);
			const uuidDir = join(baseDir, "fake-uuid-2");
			await mkdir(uuidDir, { recursive: true });

			const bigContent = "x".repeat(1000);
			await writeFile(join(uuidDir, "v1.bak"), bigContent);
			await writeFile(join(uuidDir, "v2.bak"), bigContent);
			await writeFile(join(uuidDir, "v3.bak"), bigContent);

			// maxSize = 2500 bytes, total = 3000 → should remove oldest
			const stats = await runFileHistoryCleanup({
				maxSize: 2500,
				projectPath: dir,
			});

			expect(stats.filesRemoved).toBeGreaterThanOrEqual(1);
			expect(stats.bytesFreed).toBeGreaterThanOrEqual(1000);

			// v3 (newest) should survive
			const { existsSync } = await import("node:fs");
			expect(existsSync(join(uuidDir, "v3.bak"))).toBe(true);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("returns correct stats on empty directory", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			const stats = await runFileHistoryCleanup({
				maxSize: 100 * 1024 * 1024,
				projectPath: dir,
			});

			expect(stats.filesRemoved).toBe(0);
			expect(stats.bytesFreed).toBe(0);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("no-op when base dir doesn't exist", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { runFileHistoryCleanup } = await import("../../src/file-history/cleanup.ts");

			const stats = await runFileHistoryCleanup({
				maxSize: 100 * 1024 * 1024,
				projectPath: join(dir, "nonexistent"),
			});

			expect(stats.filesRemoved).toBe(0);
			expect(stats.bytesFreed).toBe(0);

			delete process.env.HEDDLE_HOME;
		});
	});
});
