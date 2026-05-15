import { beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createMentionCompleter } from "../../src/cli/completer.ts";

describe("createMentionCompleter", () => {
	let dir: string;
	let completer: ReturnType<typeof createMentionCompleter>;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-completer-"));
		mkdirSync(join(dir, "src"));
		writeFileSync(join(dir, "src/cli.ts"), "");
		writeFileSync(join(dir, "src/config.ts"), "");
		writeFileSync(join(dir, "package.json"), "{}");
		writeFileSync(join(dir, "pants.toml"), "");
		completer = createMentionCompleter(dir);
	});

	test("@src/ completes with entries in src/ dir", () => {
		const [completions, substring] = completer("@src/");
		expect(completions.length).toBeGreaterThan(0);
		for (const c of completions) {
			expect(c).toStartWith("@src/");
		}
		expect(substring).toBe("@src/");
	});

	test("@src/cl filters to entries starting with cl", () => {
		const [completions, substring] = completer("@src/cl");
		expect(completions).toContain("@src/cli.ts");
		expect(completions).not.toContain("@src/config.ts");
		expect(substring).toBe("@src/cl");
	});

	test("@pa completes from cwd entries starting with pa", () => {
		const [completions, substring] = completer("@pa");
		expect(completions).toContain("@package.json");
		expect(completions).toContain("@pants.toml");
		expect(substring).toBe("@pa");
	});

	test("directories get / suffix", () => {
		const [completions] = completer("@sr");
		expect(completions).toContain("@src/");
	});

	test("non-@ word returns empty completions", () => {
		const [completions, substring] = completer("hello");
		expect(completions).toHaveLength(0);
		expect(substring).toBe("hello");
	});

	test("@nonexistent/ returns empty completions", () => {
		const [completions] = completer("@nonexistent/");
		expect(completions).toHaveLength(0);
	});

	test("multiple words, only last word triggers completion", () => {
		const [completions, substring] = completer("look at @src/");
		expect(completions.length).toBeGreaterThan(0);
		expect(substring).toBe("@src/");
	});
});
