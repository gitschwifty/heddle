import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { rmSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createWriteTool } from "../../src/tools/write.ts";

describe("write_file (negative)", () => {
	let dir: string;
	const tool = createWriteTool();

	beforeEach(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-write-"));
	});

	afterEach(() => {
		rmSync(dir, { recursive: true, force: true });
	});

	test("returns error when writing to /dev/null/impossible (invalid nested path)", async () => {
		const result = await tool.execute({
			file_path: "/dev/null/impossible/file.txt",
			content: "hello",
		});
		expect(result).toContain("Error");
	});

	test("returns error for empty path", async () => {
		const result = await tool.execute({ file_path: "", content: "hello" });
		expect(result).toContain("Error");
	});
});
