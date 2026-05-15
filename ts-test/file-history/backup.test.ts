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

			await backupFile(join(dir, "nonexistent.txt"), dir);

			const { existsSync } = await import("node:fs");
			expect(existsSync(join(dir, "projects"))).toBe(false);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("creates v1.bak for first backup", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			const filePath = join(dir, "source.txt");
			await writeFile(filePath, "hello world");

			await backupFile(filePath, dir);

			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const baseDir = getFileHistoryDir(dir);
			// Find the UUID dir
			const metaRaw = JSON.parse(await readFile(join(baseDir, "meta.json"), "utf-8"));
			const uuids = Object.keys(metaRaw);
			expect(uuids.length).toBe(1);

			const uuid = uuids[0] as string;
			const files = await readdir(join(baseDir, uuid));
			expect(files).toContain("v1.bak");

			const content = await readFile(join(baseDir, uuid, "v1.bak"), "utf-8");
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
			const baseDir = getFileHistoryDir(dir);
			const metaRaw = JSON.parse(await readFile(join(baseDir, "meta.json"), "utf-8"));
			const uuid = Object.keys(metaRaw)[0] as string;
			const files = await readdir(join(baseDir, uuid));
			const bakFiles = files.filter((f: string) => f.endsWith(".bak"));
			expect(bakFiles.length).toBe(1);

			delete process.env.HEDDLE_HOME;
		});
	});

	test("creates v2.bak when content changes", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			const filePath = join(dir, "changing.txt");
			await writeFile(filePath, "version 1");
			await backupFile(filePath, dir);

			await writeFile(filePath, "version 2");
			await backupFile(filePath, dir);

			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const baseDir = getFileHistoryDir(dir);
			const metaRaw = JSON.parse(await readFile(join(baseDir, "meta.json"), "utf-8"));
			const uuid = Object.keys(metaRaw)[0] as string;
			const files = await readdir(join(baseDir, uuid));
			const bakFiles = files.filter((f: string) => f.endsWith(".bak")).sort();
			expect(bakFiles).toEqual(["v1.bak", "v2.bak"]);

			const v1 = await readFile(join(baseDir, uuid, "v1.bak"), "utf-8");
			const v2 = await readFile(join(baseDir, uuid, "v2.bak"), "utf-8");
			expect(v1).toBe("version 1");
			expect(v2).toBe("version 2");

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

			await backupFile(filePath, dir);

			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const baseDir = getFileHistoryDir(dir);
			const metaRaw = JSON.parse(await readFile(join(baseDir, "meta.json"), "utf-8"));
			const uuid = Object.keys(metaRaw)[0] as string;
			const files = await readdir(join(baseDir, uuid));
			expect(files).toContain("v1.bak");

			delete process.env.HEDDLE_HOME;
		});
	});

	test("same file across calls uses same UUID", async () => {
		await withTmpDir(async (dir) => {
			process.env.HEDDLE_HOME = dir;
			const { backupFile } = await import("../../src/file-history/backup.ts");

			const filePath = join(dir, "consistent.txt");
			await writeFile(filePath, "v1");
			await backupFile(filePath, dir);

			await writeFile(filePath, "v2");
			await backupFile(filePath, dir);

			await writeFile(filePath, "v3");
			await backupFile(filePath, dir);

			const { getFileHistoryDir } = await import("../../src/config/paths.ts");
			const baseDir = getFileHistoryDir(dir);
			const metaRaw = JSON.parse(await readFile(join(baseDir, "meta.json"), "utf-8"));
			expect(Object.keys(metaRaw).length).toBe(1);
			const uuid = Object.keys(metaRaw)[0] as string;
			expect(metaRaw[uuid].versions).toBe(3);

			delete process.env.HEDDLE_HOME;
		});
	});
});
