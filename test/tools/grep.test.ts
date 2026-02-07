import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { rmSync } from "node:fs";
import { createGrepTool } from "../../src/tools/grep.ts";

describe("grep tool (negative)", () => {
	let dir: string;
	const tool = createGrepTool();

	beforeEach(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-grep-"));
	});

	afterEach(() => {
		rmSync(dir, { recursive: true, force: true });
	});

	test("returns no-match message when pattern not found", async () => {
		await writeFile(join(dir, "file.txt"), "hello world\nfoo bar\n");
		const result = await tool.execute({ pattern: "zzz_not_here", path: dir });
		expect(result).toContain("No matches found");
	});

	test("returns no-match in empty directory", async () => {
		const result = await tool.execute({ pattern: "anything", path: dir });
		expect(result).toContain("No matches found");
	});

	test("returns error for invalid regex pattern", async () => {
		await writeFile(join(dir, "file.txt"), "hello\n");
		const result = await tool.execute({ pattern: "[invalid", path: dir });
		expect(result).toMatch(/Error|No matches/);
	});

	test("returns no-match when glob filter excludes all files", async () => {
		await writeFile(join(dir, "file.txt"), "hello world\n");
		const result = await tool.execute({
			pattern: "hello",
			path: dir,
			glob: "*.py",
		});
		expect(result).toContain("No matches found");
	});
});
