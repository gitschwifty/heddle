import { describe, expect, test } from "bun:test";
import { cascadingMatch, findClosestMatch } from "../../src/tools/fuzzy-match.ts";

describe("cascadingMatch", () => {
	test("level 0: exact match returns level 0 with correct position", () => {
		const content = "hello foo bar world";
		const search = "foo bar";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBe(0);
		expect(result?.startIndex).toBe(6);
		expect(result?.matchedText).toBe("foo bar");
	});

	test("level 1: extra spaces between tokens", () => {
		const content = "hello foo  bar world";
		const search = "foo bar";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBe(1);
		expect(result?.matchedText).toBe("foo  bar");
		expect(result?.startIndex).toBe(6);
	});

	test("level 1: tabs vs spaces", () => {
		const content = "function\tfoo(\tbar\t)";
		const search = "function foo( bar )";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBe(1);
		expect(result?.matchedText).toBe("function\tfoo(\tbar\t)");
	});

	test("level 1: trailing whitespace difference", () => {
		const content = "foo bar  \nbaz";
		const search = "foo bar\nbaz";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBe(1);
		expect(result?.matchedText).toBe("foo bar  \nbaz");
	});

	test("level 2: different indentation — 2-space vs tab", () => {
		const content = "\tif (true) {\n\t\treturn 1;\n\t}";
		const search = "  if (true) {\n    return 1;\n  }";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBe(2);
		// matchedText should be the original tab-indented version
		expect(result?.matchedText).toBe("\tif (true) {\n\t\treturn 1;\n\t}");
	});

	test("level 2: matchedText preserves original indentation", () => {
		const content = "header\n    foo()\n    bar()\nfooter";
		const search = "  foo()\n  bar()";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBe(2);
		expect(result?.matchedText).toBe("    foo()\n    bar()");
		// Verify replacement works correctly
		const start = result!.startIndex;
		const end = start + result!.matchedText.length;
		const replaced = `${content.slice(0, start)}REPLACED${content.slice(end)}`;
		expect(replaced).toBe("header\nREPLACED\nfooter");
	});

	test("level 3: trailing whitespace differences per line", () => {
		const content = "  foo()  \n  bar()  ";
		const search = "  foo()\n  bar()";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBeLessThanOrEqual(3);
		expect(result?.matchedText).toBe("  foo()  \n  bar()  ");
	});

	test("returns null when all levels fail", () => {
		const content = "completely different content here";
		const search = "nothing matches this at all xyz123";
		const result = cascadingMatch(content, search);

		expect(result).toBeNull();
	});

	test("level 0: multiline exact match", () => {
		const content = "line1\nline2\nline3\nline4";
		const search = "line2\nline3";
		const result = cascadingMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.level).toBe(0);
		expect(result?.startIndex).toBe(6);
	});
});

describe("findClosestMatch", () => {
	test("returns correct line number and snippet", () => {
		const content = "alpha\nbeta\ngamma\ndelta\nepsilon";
		const search = "gamm";
		const result = findClosestMatch(content, search);

		expect(result).not.toBeNull();
		expect(result?.line).toBe(3); // 1-indexed, "gamma" is line 3
		expect(result?.snippet).toContain("gamma");
	});

	test("returns null on totally unrelated content", () => {
		const content = "aaa\nbbb\nccc";
		const search = "xyz123completely_unrelated_token";
		const result = findClosestMatch(content, search);

		expect(result).toBeNull();
	});
});
