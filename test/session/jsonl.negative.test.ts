import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { loadSession } from "../../src/session/jsonl.ts";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";

const TEST_DIR = join(import.meta.dir, ".tmp-session-neg-test");

function testPath(name: string): string {
	return join(TEST_DIR, name);
}

describe("JSONL session (negative)", () => {
	beforeEach(() => {
		mkdirSync(TEST_DIR, { recursive: true });
	});

	afterEach(() => {
		if (existsSync(TEST_DIR)) {
			rmSync(TEST_DIR, { recursive: true, force: true });
		}
	});

	test("loadSession throws on malformed JSON line", async () => {
		const filePath = testPath("bad.jsonl");
		await Bun.write(filePath, '{"role":"user","content":"ok"}\nnot valid json\n');

		expect(loadSession(filePath)).rejects.toThrow();
	});

	test("loadSession returns empty for file with only whitespace", async () => {
		const filePath = testPath("whitespace.jsonl");
		await Bun.write(filePath, "   \n  \n\n  ");

		const messages = await loadSession(filePath);
		expect(messages).toEqual([]);
	});

	test("loadSession returns empty for file with only newlines", async () => {
		const filePath = testPath("newlines.jsonl");
		await Bun.write(filePath, "\n\n\n\n");

		const messages = await loadSession(filePath);
		expect(messages).toEqual([]);
	});
});
