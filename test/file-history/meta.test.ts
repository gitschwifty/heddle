import { describe, expect, test } from "bun:test";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-meta-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

describe("FileHistoryMeta", () => {
	test("getOrCreate returns new UUID for unknown path", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const meta = new FileHistoryMeta(dir);
			const entry = await meta.getOrCreate("/some/file.ts");
			expect(entry.uuid).toMatch(/^[0-9a-f-]{36}$/);
			expect(entry.path).toBe("/some/file.ts");
			expect(entry.versions).toBe(0);
		});
	});

	test("getOrCreate returns same UUID for same path", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const meta = new FileHistoryMeta(dir);
			const first = await meta.getOrCreate("/some/file.ts");
			const second = await meta.getOrCreate("/some/file.ts");
			expect(first.uuid).toBe(second.uuid);
		});
	});

	test("incrementVersion bumps version count", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const meta = new FileHistoryMeta(dir);
			const entry = await meta.getOrCreate("/some/file.ts");
			expect(entry.versions).toBe(0);

			await meta.incrementVersion(entry.uuid);
			const updated = await meta.getOrCreate("/some/file.ts");
			expect(updated.versions).toBe(1);

			await meta.incrementVersion(entry.uuid);
			const again = await meta.getOrCreate("/some/file.ts");
			expect(again.versions).toBe(2);
		});
	});

	test("findByPath returns null for unknown path", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const meta = new FileHistoryMeta(dir);
			const result = await meta.findByPath("/nonexistent.ts");
			expect(result).toBeNull();
		});
	});

	test("persists to meta.json on disk", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const meta = new FileHistoryMeta(dir);
			const entry = await meta.getOrCreate("/persisted.ts");
			await meta.incrementVersion(entry.uuid);

			// Read meta.json directly
			const raw = JSON.parse(await readFile(join(dir, "meta.json"), "utf-8"));
			expect(raw[entry.uuid]).toBeDefined();
			expect(raw[entry.uuid].path).toBe("/persisted.ts");
			expect(raw[entry.uuid].versions).toBe(1);
		});
	});

	test("loads existing meta.json on construction", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");

			// First instance creates entry
			const meta1 = new FileHistoryMeta(dir);
			const entry = await meta1.getOrCreate("/reload.ts");
			await meta1.incrementVersion(entry.uuid);

			// Second instance should load from disk
			const meta2 = new FileHistoryMeta(dir);
			const found = await meta2.findByPath("/reload.ts");
			expect(found).not.toBeNull();
			expect(found!.uuid).toBe(entry.uuid);
			expect(found!.versions).toBe(1);
		});
	});

	test("tracks previous paths when file is re-registered", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const meta = new FileHistoryMeta(dir);

			// Create entry for old path
			const old = await meta.getOrCreate("/old/path.ts");
			await meta.incrementVersion(old.uuid);

			// Register new path, referencing old uuid
			const moved = await meta.getOrCreate("/new/path.ts", old.uuid);
			expect(moved.uuid).not.toBe(old.uuid); // new UUID
			expect(moved.previousPaths).toContain("/old/path.ts");
		});
	});

	test("multiple files get distinct UUIDs", async () => {
		await withTmpDir(async (dir) => {
			const { FileHistoryMeta } = await import("../../src/file-history/meta.ts");
			const meta = new FileHistoryMeta(dir);

			const a = await meta.getOrCreate("/a.ts");
			const b = await meta.getOrCreate("/b.ts");
			expect(a.uuid).not.toBe(b.uuid);
		});
	});
});
