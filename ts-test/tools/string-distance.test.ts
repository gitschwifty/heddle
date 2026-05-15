import { describe, expect, test } from "bun:test";
import { findClosest, levenshtein } from "../../src/tools/string-distance.ts";

describe("levenshtein", () => {
	test("identical strings → distance 0", () => {
		expect(levenshtein("hello", "hello")).toBe(0);
	});

	test("single char difference → distance 1", () => {
		expect(levenshtein("cat", "bat")).toBe(1);
		expect(levenshtein("cat", "ca")).toBe(1);
		expect(levenshtein("cat", "cats")).toBe(1);
	});

	test("completely different strings → correct distance", () => {
		expect(levenshtein("abc", "xyz")).toBe(3);
		expect(levenshtein("", "hello")).toBe(5);
		expect(levenshtein("hello", "")).toBe(5);
	});

	test("case sensitive matching", () => {
		expect(levenshtein("Hello", "hello")).toBe(1);
		expect(levenshtein("ABC", "abc")).toBe(3);
	});
});

describe("findClosest", () => {
	test("returns closest match within maxDistance", () => {
		const candidates = ["read_file", "write_file", "edit_file"];
		expect(findClosest("reed_file", candidates)).toBe("read_file");
		expect(findClosest("writ_file", candidates)).toBe("write_file");
	});

	test("returns null when nothing within maxDistance", () => {
		const candidates = ["read_file", "write_file"];
		expect(findClosest("completely_different_tool_name", candidates)).toBeNull();
	});
});
