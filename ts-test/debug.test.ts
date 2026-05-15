import { afterEach, describe, expect, mock, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// We need to test initDebug() which runs at module load time,
// so we'll test the exported functions after manipulating env vars.

describe("debug utility", () => {
	const origEnv = { ...process.env };
	let debugMod: typeof import("../src/debug.ts");

	afterEach(async () => {
		process.env = { ...origEnv };
		if (debugMod) debugMod.resetDebug();
	});

	test("debug() is silent when HEDDLE_DEBUG is not set", async () => {
		delete process.env.HEDDLE_DEBUG;
		// Re-import to trigger initDebug with clean env
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug(); // reset channels after import

		const spy = mock();
		const origDebug = console.debug;
		console.debug = spy;
		try {
			debugMod.debug("provider", "test message");
			expect(spy).not.toHaveBeenCalled();
		} finally {
			console.debug = origDebug;
		}
	});

	test("HEDDLE_DEBUG=1 enables all channels", async () => {
		process.env.HEDDLE_DEBUG = "1";
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();

		const spy = mock();
		const origDebug = console.debug;
		console.debug = spy;
		try {
			debugMod.debug("provider", "hello");
			expect(spy).toHaveBeenCalledTimes(1);
			expect(spy.mock.calls[0]?.[0]).toBe("[heddle:provider]");

			debugMod.debug("config", "world");
			expect(spy).toHaveBeenCalledTimes(2);
			expect(spy.mock.calls[1]?.[0]).toBe("[heddle:config]");
		} finally {
			console.debug = origDebug;
		}
	});

	test("HEDDLE_DEBUG=true enables all channels", async () => {
		process.env.HEDDLE_DEBUG = "true";
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();

		const spy = mock();
		const origDebug = console.debug;
		console.debug = spy;
		try {
			debugMod.debug("anything", "test");
			expect(spy).toHaveBeenCalledTimes(1);
		} finally {
			console.debug = origDebug;
		}
	});

	test("HEDDLE_DEBUG=provider only enables provider channel", async () => {
		process.env.HEDDLE_DEBUG = "provider";
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();

		const spy = mock();
		const origDebug = console.debug;
		console.debug = spy;
		try {
			debugMod.debug("provider", "yes");
			expect(spy).toHaveBeenCalledTimes(1);

			debugMod.debug("config", "no");
			expect(spy).toHaveBeenCalledTimes(1); // not called again
		} finally {
			console.debug = origDebug;
		}
	});

	test("HEDDLE_DEBUG=provider,config enables both channels", async () => {
		process.env.HEDDLE_DEBUG = "provider,config";
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();

		const spy = mock();
		const origDebug = console.debug;
		console.debug = spy;
		try {
			debugMod.debug("provider", "p");
			debugMod.debug("config", "c");
			debugMod.debug("other", "o");
			expect(spy).toHaveBeenCalledTimes(2);
		} finally {
			console.debug = origDebug;
		}
	});

	test("headless mode uses console.error (stderr)", async () => {
		process.env.HEDDLE_DEBUG = "1";
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();
		debugMod.setHeadless(true);

		const errorSpy = mock();
		const debugSpy = mock();
		const origError = console.error;
		const origDebug = console.debug;
		console.error = errorSpy;
		console.debug = debugSpy;
		try {
			debugMod.debug("provider", "headless test");
			expect(errorSpy).toHaveBeenCalledTimes(1);
			expect(debugSpy).not.toHaveBeenCalled();
			expect(errorSpy.mock.calls[0]?.[0]).toBe("[heddle:provider]");
		} finally {
			console.error = origError;
			console.debug = origDebug;
			debugMod.setHeadless(false);
		}
	});

	test("CLI mode uses console.debug", async () => {
		process.env.HEDDLE_DEBUG = "1";
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();
		debugMod.setHeadless(false);

		const errorSpy = mock();
		const debugSpy = mock();
		const origError = console.error;
		const origDebug = console.debug;
		console.error = errorSpy;
		console.debug = debugSpy;
		try {
			debugMod.debug("provider", "cli test");
			expect(debugSpy).toHaveBeenCalledTimes(1);
			expect(errorSpy).not.toHaveBeenCalled();
		} finally {
			console.error = origError;
			console.debug = origDebug;
		}
	});

	test("HEDDLE_DEBUG_FILE writes to log file instead of console", async () => {
		const dir = mkdtempSync(join(tmpdir(), "heddle-debug-file-"));
		const logPath = join(dir, "debug.log");

		process.env.HEDDLE_DEBUG = "1";
		process.env.HEDDLE_DEBUG_FILE = logPath;
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();

		const debugSpy = mock();
		const origDebug = console.debug;
		console.debug = debugSpy;
		try {
			debugMod.debug("provider", "file log test");
			debugMod.debug("config", "second line", { key: "value" });

			expect(debugSpy).not.toHaveBeenCalled();
			expect(existsSync(logPath)).toBe(true);

			const content = readFileSync(logPath, "utf-8");
			const lines = content.trim().split("\n");
			expect(lines).toHaveLength(2);
			expect(lines[0]).toContain("[heddle:provider]");
			expect(lines[0]).toContain("file log test");
			expect(lines[1]).toContain("[heddle:config]");
			expect(lines[1]).toContain("second line");
			expect(lines[1]).toContain('"key":"value"');
			// Verify timestamp format (ISO 8601)
			expect(lines[0]).toMatch(/^\d{4}-\d{2}-\d{2}T/);
		} finally {
			console.debug = origDebug;
			rmSync(dir, { recursive: true });
		}
	});

	test("clearLogFile() empties the log file", async () => {
		const dir = mkdtempSync(join(tmpdir(), "heddle-debug-clear-"));
		const logPath = join(dir, "debug.log");

		process.env.HEDDLE_DEBUG = "1";
		process.env.HEDDLE_DEBUG_FILE = logPath;
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();

		try {
			debugMod.debug("provider", "before clear");
			expect(readFileSync(logPath, "utf-8").trim()).not.toBe("");

			debugMod.clearLogFile();
			expect(readFileSync(logPath, "utf-8")).toBe("");

			debugMod.debug("provider", "after clear");
			const content = readFileSync(logPath, "utf-8").trim();
			expect(content.split("\n")).toHaveLength(1);
			expect(content).toContain("after clear");
		} finally {
			rmSync(dir, { recursive: true });
		}
	});

	test("file logging includes JSON-formatted objects", async () => {
		const dir = mkdtempSync(join(tmpdir(), "heddle-debug-json-"));
		const logPath = join(dir, "debug.log");

		process.env.HEDDLE_DEBUG = "1";
		process.env.HEDDLE_DEBUG_FILE = logPath;
		debugMod = await import("../src/debug.ts");
		debugMod.resetDebug();

		try {
			debugMod.debug("provider", "request:", { model: "test", messages: [{ role: "user" }] });

			const content = readFileSync(logPath, "utf-8");
			expect(content).toContain('"model":"test"');
			expect(content).toContain("[heddle:provider]");
		} finally {
			rmSync(dir, { recursive: true });
		}
	});
});
