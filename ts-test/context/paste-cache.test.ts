import { describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PasteCache } from "../../src/context/paste-cache.ts";

async function withTmpDir(fn: (dir: string) => Promise<void>): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "heddle-paste-cache-"));
	try {
		await fn(dir);
	} finally {
		await rm(dir, { recursive: true });
	}
}

describe("PasteCache", () => {
	test("first resolve reads from disk and caches", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "hello.txt");
			await writeFile(filePath, "hello world");

			const cache = new PasteCache();
			const result = await cache.resolve(filePath);

			expect(result.path).toBe(filePath);
			expect(result.content).toBe("hello world");
			expect(result.lines).toBe(1);
			expect(result.hash).toBeString();
			expect(result.timestamp).toBeNumber();
			expect(cache.size).toBe(1);
		});
	});

	test("second resolve returns cached entry without re-read if unchanged", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "cached.txt");
			await writeFile(filePath, "cached content");

			const cache = new PasteCache();
			const first = await cache.resolve(filePath);
			const second = await cache.resolve(filePath);

			// Same object reference means it was returned from cache
			expect(second.hash).toBe(first.hash);
			expect(second.content).toBe(first.content);
			expect(second.timestamp).toBe(first.timestamp);
		});
	});

	test("file change detected via hash mismatch triggers re-read", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "changing.txt");
			await writeFile(filePath, "version 1");

			const cache = new PasteCache();
			const first = await cache.resolve(filePath);
			expect(first.content).toBe("version 1");

			// Modify the file
			await writeFile(filePath, "version 2");

			const second = await cache.resolve(filePath);
			expect(second.content).toBe("version 2");
			expect(second.hash).not.toBe(first.hash);
			expect(cache.size).toBe(1);
		});
	});

	test("large files get paste IDs assigned", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "large.txt");
			// Use a small threshold for testing
			const threshold = 100;
			const largeContent = "x".repeat(threshold + 1);
			await writeFile(filePath, largeContent);

			const cache = new PasteCache(threshold);
			const result = await cache.resolve(filePath);

			expect(result.pasteId).toBeString();
			expect(result.pasteId!.length).toBe(6);
		});
	});

	test("small files do not get paste IDs", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "small.txt");
			await writeFile(filePath, "tiny");

			const cache = new PasteCache(100);
			const result = await cache.resolve(filePath);

			expect(result.pasteId).toBeUndefined();
		});
	});

	test("getByPasteId returns correct entry", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "lookup.txt");
			const content = "x".repeat(200);
			await writeFile(filePath, content);

			const cache = new PasteCache(100);
			const result = await cache.resolve(filePath);
			expect(result.pasteId).toBeString();

			const looked = cache.getByPasteId(result.pasteId!);
			expect(looked).not.toBeNull();
			expect(looked!.path).toBe(filePath);
			expect(looked!.content).toBe(content);
		});
	});

	test("getByPasteId returns null for unknown ID", () => {
		const cache = new PasteCache();
		expect(cache.getByPasteId("nope00")).toBeNull();
	});

	test("list returns all cached entries", async () => {
		await withTmpDir(async (dir) => {
			const fileA = join(dir, "a.txt");
			const fileB = join(dir, "b.txt");
			await writeFile(fileA, "aaa");
			await writeFile(fileB, "bbb");

			const cache = new PasteCache();
			await cache.resolve(fileA);
			await cache.resolve(fileB);

			const entries = cache.list();
			expect(entries).toHaveLength(2);
			const paths = entries.map((e) => e.path);
			expect(paths).toContain(fileA);
			expect(paths).toContain(fileB);
		});
	});

	test("clear empties the cache", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "clearme.txt");
			await writeFile(filePath, "gone soon");

			const cache = new PasteCache();
			await cache.resolve(filePath);
			expect(cache.size).toBe(1);

			cache.clear();
			expect(cache.size).toBe(0);
			expect(cache.list()).toHaveLength(0);
		});
	});

	test("clear also removes paste ID mappings", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "clearids.txt");
			await writeFile(filePath, "x".repeat(200));

			const cache = new PasteCache(100);
			const result = await cache.resolve(filePath);
			const pasteId = result.pasteId!;
			expect(cache.getByPasteId(pasteId)).not.toBeNull();

			cache.clear();
			expect(cache.getByPasteId(pasteId)).toBeNull();
		});
	});

	test("nonexistent file throws an error", async () => {
		const cache = new PasteCache();
		await expect(cache.resolve("/no/such/file.txt")).rejects.toThrow();
	});

	test("lines counts newlines correctly", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "multiline.txt");
			await writeFile(filePath, "line1\nline2\nline3\n");

			const cache = new PasteCache();
			const result = await cache.resolve(filePath);
			expect(result.lines).toBe(4); // split on \n gives 4 elements
		});
	});

	test("paste ID is stable across re-resolves of unchanged large file", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "stable.txt");
			await writeFile(filePath, "x".repeat(200));

			const cache = new PasteCache(100);
			const first = await cache.resolve(filePath);
			const second = await cache.resolve(filePath);

			expect(second.pasteId).toBe(first.pasteId);
		});
	});

	test("paste ID changes when large file content changes", async () => {
		await withTmpDir(async (dir) => {
			const filePath = join(dir, "changing-large.txt");
			await writeFile(filePath, "x".repeat(200));

			const cache = new PasteCache(100);
			const first = await cache.resolve(filePath);
			const firstId = first.pasteId;

			await writeFile(filePath, "y".repeat(200));
			const second = await cache.resolve(filePath);

			// New content gets a new paste ID
			expect(second.pasteId).toBeString();
			expect(second.pasteId).not.toBe(firstId);
		});
	});
});
