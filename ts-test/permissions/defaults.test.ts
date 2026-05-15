import { describe, expect, test } from "bun:test";
import { DEFAULT_DENY_RULES, generateDefaultPermissionsToml } from "../../src/permissions/defaults.ts";
import { parseRule } from "../../src/permissions/rules.ts";

describe("DEFAULT_DENY_RULES", () => {
	test("contains .env write protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Write(.env*)");
	});

	test("contains .pem write protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Write(*.pem)");
	});

	test("contains .key write protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Write(*.key)");
	});

	test("contains credentials write protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Write(*credentials*)");
	});

	test("contains rm command protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Bash(rm *)");
		expect(DEFAULT_DENY_RULES).toContain("Bash(rm)");
	});

	test("contains sudo protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Bash(sudo *)");
	});

	test("contains chmod protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Bash(chmod *)");
	});

	test("contains config self-protection", () => {
		expect(DEFAULT_DENY_RULES).toContain("Write(.heddle/config.toml)");
	});

	test("all rules parse successfully", () => {
		for (const rule of DEFAULT_DENY_RULES) {
			const parsed = parseRule(rule);
			expect(parsed).not.toBeNull();
		}
	});
});

describe("generateDefaultPermissionsToml", () => {
	test("produces valid TOML fragment", () => {
		const toml = generateDefaultPermissionsToml();
		expect(toml).toContain("[permissions]");
		expect(toml).toContain("deny = [");
		expect(toml).toContain("Write(.env*)");
		expect(toml).toContain("Bash(rm *)");
	});

	test("includes all default deny rules", () => {
		const toml = generateDefaultPermissionsToml();
		for (const rule of DEFAULT_DENY_RULES) {
			expect(toml).toContain(rule);
		}
	});
});
