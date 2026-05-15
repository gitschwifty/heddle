import { beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildMentionMessage, resolveMentions } from "../../src/cli/mentions.ts";

describe("resolveMentions", () => {
	let dir: string;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-mentions-"));
		mkdirSync(join(dir, "src"));
		writeFileSync(join(dir, "src/index.ts"), "console.log('hello');");
		writeFileSync(join(dir, "config.toml"), 'model = "test"');
		writeFileSync(join(dir, "a.ts"), "const a = 1;");
		writeFileSync(join(dir, "b.ts"), "const b = 2;");
	});

	test("basic file mention injects content and cleans input", async () => {
		const result = await resolveMentions("look at @src/index.ts", dir);
		expect(result.cleanedInput).toBe("look at src/index.ts");
		expect(result.injectedFiles).toHaveLength(1);
		const file = result.injectedFiles[0]!;
		expect(file.path).toBe(join(dir, "src/index.ts"));
		expect(file.content).toBe("console.log('hello');");
		expect(file.lines).toBe(1);
		expect(result.errors).toHaveLength(0);
	});

	test("directory mention injects listing", async () => {
		const result = await resolveMentions("check @src/", dir);
		expect(result.cleanedInput).toBe("check src/");
		expect(result.injectedFiles).toHaveLength(1);
		expect(result.injectedFiles[0]!.content).toContain("index.ts");
		expect(result.errors).toHaveLength(0);
	});

	test("multiple mentions both injected", async () => {
		const result = await resolveMentions("compare @a.ts and @b.ts", dir);
		expect(result.cleanedInput).toBe("compare a.ts and b.ts");
		expect(result.injectedFiles).toHaveLength(2);
		expect(result.errors).toHaveLength(0);
	});

	test("duplicate path only injected once", async () => {
		const result = await resolveMentions("@a.ts @a.ts", dir);
		expect(result.injectedFiles).toHaveLength(1);
	});

	test("non-existent path populates errors", async () => {
		const result = await resolveMentions("@missing.ts", dir);
		expect(result.injectedFiles).toHaveLength(0);
		expect(result.errors).toHaveLength(1);
		expect(result.errors[0]).toContain("Not found");
		expect(result.errors[0]).toContain("missing.ts");
	});

	test("no mentions returns input unchanged", async () => {
		const result = await resolveMentions("just regular text", dir);
		expect(result.cleanedInput).toBe("just regular text");
		expect(result.injectedFiles).toHaveLength(0);
		expect(result.errors).toHaveLength(0);
	});

	test("non-path @ (no / or .) is not treated as mention", async () => {
		const result = await resolveMentions("hello @username", dir);
		expect(result.cleanedInput).toBe("hello @username");
		expect(result.injectedFiles).toHaveLength(0);
		expect(result.errors).toHaveLength(0);
	});

	test("path with dot is treated as mention", async () => {
		const result = await resolveMentions("@config.toml", dir);
		expect(result.injectedFiles).toHaveLength(1);
		expect(result.injectedFiles[0]!.content).toContain("model");
	});

	test("path with slash is treated as mention", async () => {
		const result = await resolveMentions("@src/index.ts", dir);
		expect(result.injectedFiles).toHaveLength(1);
	});

	test("mixed valid and invalid paths", async () => {
		const result = await resolveMentions("@a.ts @fake.ts", dir);
		expect(result.injectedFiles).toHaveLength(1);
		expect(result.errors).toHaveLength(1);
	});
});

describe("buildMentionMessage", () => {
	test("single file produces correct format", () => {
		const result = buildMentionMessage("look at src/index.ts", [
			{ path: "/tmp/src/index.ts", content: "console.log('hello');", lines: 1 },
		]);
		expect(result).toContain("look at src/index.ts");
		expect(result).toContain("---");
		expect(result).toContain("Referenced files:");
		expect(result).toContain("`/tmp/src/index.ts`:");
		expect(result).toContain("```ts");
		expect(result).toContain("console.log('hello');");
	});

	test("multiple files all included", () => {
		const result = buildMentionMessage("compare", [
			{ path: "/tmp/a.ts", content: "const a = 1;", lines: 1 },
			{ path: "/tmp/b.md", content: "# Hello", lines: 1 },
		]);
		expect(result).toContain("`/tmp/a.ts`:");
		expect(result).toContain("```ts");
		expect(result).toContain("`/tmp/b.md`:");
		expect(result).toContain("```md");
	});

	test("file extension detection", () => {
		const result = buildMentionMessage("test", [{ path: "/tmp/app.js", content: "x", lines: 1 }]);
		expect(result).toContain("```js");
	});

	test("no extension uses empty fence", () => {
		const result = buildMentionMessage("test", [{ path: "/tmp/Makefile", content: "all:", lines: 1 }]);
		expect(result).toContain("```\n");
	});
});
