import { describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-backup-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

describe("backupFile", () => {
	test("skips backup if file doesn't exist (new file)", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			// File doesn't exist — should return early with no error
			await backupFile(join(dir, "nonexistent.txt"), dir);

			// Backup dir should not have been created
			const { existsSync } = await import("node:fs");
			expect(existsSync(join(dir, "projects"))).toBe(false);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("creates backup with timestamp filename", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			const filePath = join(dir, "source.txt");
			await writeFile(filePath, "hello world");

			await backupFile(filePath, dir);

			// Find the backup directory
			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const histDir = getFileHistoryDir(dir, filePath);
			const files = await readdir(histDir);
			expect(files.length).toBe(1);
			expect(files[0]).toMatch(/^\d+\.bak$/);

			const content = await readFile(join(histDir, files[0] as string), "utf-8");
			expect(content).toBe("hello world");

			delete process.env.HEDDLE_HOME;
		});
	});

	test("deduplicates identical content", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			const filePath = join(dir, "dup.txt");
			await writeFile(filePath, "same content");

			await backupFile(filePath, dir);
			await backupFile(filePath, dir);

			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const histDir = getFileHistoryDir(dir, filePath);
			const files = await readdir(histDir);
			// Should only have 1 backup since content is identical
			expect(files.length).toBe(1);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("creates new backup when content changes", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			const filePath = join(dir, "changing.txt");
			await writeFile(filePath, "version 1");
			await backupFile(filePath, dir);

			await writeFile(filePath, "version 2");
			await backupFile(filePath, dir);

			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const histDir = getFileHistoryDir(dir, filePath);
			const files = await readdir(histDir);
			expect(files.length).toBe(2);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("creates parent dirs as needed", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			const filePath = join(dir, "deep", "nested", "file.txt");
			await mkdir(join(dir, "deep", "nested"), { recursive: true });
			await writeFile(filePath, "nested content");

			// Should not throw — creates backup dirs automatically
			await backupFile(filePath, dir);

			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const histDir = getFileHistoryDir(dir, filePath);
			const files = await readdir(histDir);
			expect(files.length).toBe(1);

			delete process.env.HEDDLE_HOME;
		});
	});
});
