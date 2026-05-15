import { afterEach, describe, expect, test } from "bun:test";
import { existsSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { createTestSandbox } from "./sandbox.ts";

describe("createTestSandbox", () => {
	const origEnv = { ...process.env };
	const origCwd = process.cwd();

	afterEach(() => {
		// Safety net — restore even if a test forgets cleanup
		process.env = { ...origEnv };
		process.chdir(origCwd);
	});

	test("creates sandbox directories in tmpdir", () => {
		const sandbox = createTestSandbox("dirs");
		try {
			expect(sandbox.root.startsWith(realpathSync(tmpdir()))).toBe(true);
			expect(existsSync(sandbox.heddleHome)).toBe(true);
			expect(existsSync(sandbox.project)).toBe(true);
		} finally {
			sandbox.cleanup();
		}
	});

	test("sets HEDDLE_HOME to sandbox heddleHome", () => {
		const sandbox = createTestSandbox("env");
		try {
			expect(process.env.HEDDLE_HOME).toBe(sandbox.heddleHome);
		} finally {
			sandbox.cleanup();
		}
	});

	test("changes cwd to sandbox project dir", () => {
		const sandbox = createTestSandbox("cwd");
		try {
			expect(process.cwd()).toBe(sandbox.project);
		} finally {
			sandbox.cleanup();
		}
	});

	test("cleanup restores env", () => {
		const sandbox = createTestSandbox("restore-env");
		sandbox.cleanup();
		expect(process.env.HEDDLE_HOME).toBe(origEnv.HEDDLE_HOME);
	});

	test("cleanup restores cwd", () => {
		const sandbox = createTestSandbox("restore-cwd");
		sandbox.cleanup();
		expect(process.cwd()).toBe(origCwd);
	});

	test("cleanup removes sandbox files", () => {
		const sandbox = createTestSandbox("remove");
		const root = sandbox.root;
		sandbox.cleanup();
		expect(existsSync(root)).toBe(false);
	});

	test("unique dirs per call (concurrent safety)", () => {
		const a = createTestSandbox("unique");
		const b = createTestSandbox("unique");
		try {
			expect(a.root).not.toBe(b.root);
		} finally {
			b.cleanup();
			a.cleanup();
		}
	});
});
