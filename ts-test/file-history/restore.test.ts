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
	test("lists backups sorted newest-first (highest version first)", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const { listBackups } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "test.txt");
			const baseDir = getFileHistoryDir(dir);
			const meta = new FileHistoryMeta(baseDir);
			const entry = await meta.getOrCreate(filePath);
			const uuidDir = join(baseDir, entry.uuid);
			await mkdir(uuidDir, { recursive: true });

			await writeFile(join(uuidDir, "v1.bak"), "old");
			await meta.incrementVersion(entry.uuid);
			await writeFile(join(uuidDir, "v2.bak"), "middle");
			await meta.incrementVersion(entry.uuid);
			await writeFile(join(uuidDir, "v3.bak"), "newest");
			await meta.incrementVersion(entry.uuid);

			const backups = await listBackups(filePath, dir);
			expect(backups.length).toBe(3);
			expect(backups[0]!.version).toBe(3);
			expect(backups[1]!.version).toBe(2);
			expect(backups[2]!.version).toBe(1);
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
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const { listBackups } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "sized.txt");
			const baseDir = getFileHistoryDir(dir);
			const meta = new FileHistoryMeta(baseDir);
			const entry = await meta.getOrCreate(filePath);
			const uuidDir = join(baseDir, entry.uuid);
			await mkdir(uuidDir, { recursive: true });

			await writeFile(join(uuidDir, "v1.bak"), "twelve chars");
			await meta.incrementVersion(entry.uuid);

			const backups = await listBackups(filePath, dir);
			expect(backups.length).toBe(1);
			expect(backups[0]!.size).toBe(12);
		});
	});
});

describe("restoreBackup", () => {
	test("restores backup to original path", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const { restoreBackup } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "restore-me.txt");
			const baseDir = getFileHistoryDir(dir);
			const meta = new FileHistoryMeta(baseDir);
			const entry = await meta.getOrCreate(filePath);
			const uuidDir = join(baseDir, entry.uuid);
			await mkdir(uuidDir, { recursive: true });

			await writeFile(join(uuidDir, "v1.bak"), "restored content");
			await meta.incrementVersion(entry.uuid);
			await writeFile(filePath, "current content");

			const result = await restoreBackup(filePath, 1, dir);
			expect(result).toContain("Restored");

			const content = await readFile(filePath, "utf-8");
			expect(content).toBe("restored content");
		});
	});

	test("returns error for nonexistent version", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { restoreBackup } = await import("../../src/file-history/restore.ts");

			const filePath = join(dir, "missing.txt");
			const result = await restoreBackup(filePath, 99, dir);
			expect(result).toMatch(/not found|error/i);
		});
	});
});
