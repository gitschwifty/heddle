import { describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-restore-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

describe("listBackups", () => {
	test("lists backups sorted newest-first", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { listBackups } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "test.txt");
			const histDir = getFileHistoryDir(dir, filePath);
			await mkdir(histDir, { recursive: true });

			// Create backups with known timestamps
			await writeFile(join(histDir, "1000.bak"), "old");
			await writeFile(join(histDir, "3000.bak"), "newest");
			await writeFile(join(histDir, "2000.bak"), "middle");

			const backups = await listBackups(filePath, dir);
			expect(backups.length).toBe(3);
			expect(backups[0]!.timestamp).toBe(3000);
			expect(backups[1]!.timestamp).toBe(2000);
			expect(backups[2]!.timestamp).toBe(1000);
		});
	});

	test("returns empty array for nonexistent file", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { listBackups } = await import("../../src/file-history/restore.ts");

			const backups = await listBackups(join(dir, "nope.txt"), dir);
			expect(backups).toEqual([]);
		});
	});

	test("backup entries include size", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { listBackups } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "sized.txt");
			const histDir = getFileHistoryDir(dir, filePath);
			await mkdir(histDir, { recursive: true });

			await writeFile(join(histDir, "5000.bak"), "twelve chars");

			const backups = await listBackups(filePath, dir);
			expect(backups.length).toBe(1);
			expect(backups[0]!.size).toBe(12);
			expect(backups[0]!.path).toContain("5000.bak");
		});
	});
});

describe("restoreBackup", () => {
	test("restores backup to original path", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { restoreBackup } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "restore-me.txt");
			const histDir = getFileHistoryDir(dir, filePath);
			await mkdir(histDir, { recursive: true });

			await writeFile(join(histDir, "9999.bak"), "restored content");
			await writeFile(filePath, "current content");

			const result = await restoreBackup(filePath, 9999, dir);
			expect(result).toContain("Restored");

			const content = await readFile(filePath, "utf-8");
			expect(content).toBe("restored content");
		});
	});

	test("returns error for nonexistent timestamp", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { restoreBackup } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "missing.txt");
			const result = await restoreBackup(filePath, 12345, dir);
			expect(result).toMatch(/not found|error/i);
		});
	});
});
