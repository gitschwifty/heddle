import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createGrepTool } from "../../src/tools/grep.ts";

describe("grep tool", () => {
	const tool = createGrepTool();
	let dir: string;

	beforeAll(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-grep-"));
		// Each test uses distinct filenames
		await writeFile(join(dir, "match.txt"), "hello world\nfoo bar\nbaz\n");
		await writeFile(join(dir, "code.ts"), "const x = 1;\n");
		await writeFile(join(dir, "notes.txt"), "const y = 2;\n");
		await writeFile(join(dir, "nomatch.txt"), "nothing interesting here\n");
		await writeFile(join(dir, "regex.txt"), "hello\n");
		await writeFile(join(dir, "globtest.txt"), "hello world\n");
	});

	afterAll(async () => {
		await rm(dir, { recursive: true });
	});

	test("returns matching lines with file paths", async () => {
		const result = await tool.execute({ pattern: "foo", path: dir });
		expect(result).toContain("foo bar");
		expect(result).toContain("match.txt");
	});

	test("respects glob filter", async () => {
		const result = await tool.execute({ pattern: "const", path: dir, glob: "*.ts" });
		expect(result).toContain("code.ts");
		expect(result).not.toContain("notes.txt");
	});

	test("returns no-match message when pattern not found", async () => {
		const result = await tool.execute({ pattern: "zzz_not_here", path: dir });
		expect(result).toContain("No matches found");
	});

	test("returns error for invalid regex pattern", async () => {
		const result = await tool.execute({ pattern: "[invalid", path: dir });
		// grep exit code 2 for invalid regex — hits the exitCode > 1 branch
		expect(result).toContain("Error");
	});

	test("returns no-match when glob filter excludes all files", async () => {
		const result = await tool.execute({
			pattern: "hello",
			path: dir,
			glob: "*.py",
		});
		expect(result).toContain("No matches found");
	});

	test("returns error when path does not exist", async () => {
		const result = await tool.execute({ pattern: "test", path: "/tmp/heddle-nonexistent-path-xyz" });
		// grep exits with code 2 for nonexistent paths
		expect(result).toContain("Error");
	});
});
