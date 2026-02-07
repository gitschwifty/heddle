import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createEditTool } from "../../src/tools/edit.ts";

describe("edit tool", () => {
	let dir: string;

	beforeEach(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-edit-"));
	});

	afterEach(async () => {
		await rm(dir, { recursive: true });
	});

	test("exact match replacement", async () => {
		const filePath = join(dir, "test.txt");
		await writeFile(filePath, "hello world\nfoo bar\nbaz");

		const tool = createEditTool();
		const result = await tool.execute({
			file_path: filePath,
			old_string: "foo bar",
			new_string: "FOO BAR",
		});

		expect(result).toContain("Applied edit");
		const content = await Bun.file(filePath).text();
		expect(content).toBe("hello world\nFOO BAR\nbaz");
	});

	test("replace_all replaces every occurrence", async () => {
		const filePath = join(dir, "test.txt");
		await writeFile(filePath, "aaa bbb aaa ccc aaa");

		const tool = createEditTool();
		const result = await tool.execute({
			file_path: filePath,
			old_string: "aaa",
			new_string: "ZZZ",
			replace_all: true,
		});

		expect(result).toContain("Replaced 3 occurrences");
		const content = await Bun.file(filePath).text();
		expect(content).toBe("ZZZ bbb ZZZ ccc ZZZ");
	});

	test("fails when old_string is not unique (without replace_all)", async () => {
		const filePath = join(dir, "test.txt");
		await writeFile(filePath, "aaa bbb aaa");

		const tool = createEditTool();
		const result = await tool.execute({
			file_path: filePath,
			old_string: "aaa",
			new_string: "ZZZ",
		});

		expect(result).toContain("not unique");
		// File should be unchanged
		const content = await Bun.file(filePath).text();
		expect(content).toBe("aaa bbb aaa");
	});

	test("fails when old_string is not found", async () => {
		const filePath = join(dir, "test.txt");
		await writeFile(filePath, "hello world");

		const tool = createEditTool();
		const result = await tool.execute({
			file_path: filePath,
			old_string: "nonexistent",
			new_string: "replacement",
		});

		expect(result).toContain("not found");
	});

	test("fails when file does not exist", async () => {
		const tool = createEditTool();
		const result = await tool.execute({
			file_path: join(dir, "nonexistent.txt"),
			old_string: "foo",
			new_string: "bar",
		});

		expect(result).toContain("not found");
	});

	test("multiline replacement", async () => {
		const filePath = join(dir, "test.txt");
		await writeFile(filePath, "line1\nline2\nline3\nline4");

		const tool = createEditTool();
		const result = await tool.execute({
			file_path: filePath,
			old_string: "line2\nline3",
			new_string: "REPLACED",
		});

		expect(result).toContain("Applied edit");
		const content = await Bun.file(filePath).text();
		expect(content).toBe("line1\nREPLACED\nline4");
	});
});
