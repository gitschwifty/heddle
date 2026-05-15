import { describe, expect, test } from "bun:test";
import { formatShellForContext, printShellResult, runShell, type ShellResult } from "../../src/cli/shell.ts";

describe("runShell", () => {
	test("captures stdout and returns exitCode 0", async () => {
		const result = await runShell("echo hello");
		expect(result.stdout.trim()).toBe("hello");
		expect(result.stderr).toBe("");
		expect(result.exitCode).toBe(0);
	});

	test("returns non-zero exitCode on failure", async () => {
		const result = await runShell("exit 1");
		expect(result.exitCode).toBe(1);
	});

	test("captures stderr", async () => {
		const result = await runShell("echo err >&2");
		expect(result.stderr.trim()).toBe("err");
		expect(result.stdout).toBe("");
		expect(result.exitCode).toBe(0);
	});

	test("captures both stdout and stderr", async () => {
		const result = await runShell("echo out && echo err >&2");
		expect(result.stdout.trim()).toBe("out");
		expect(result.stderr.trim()).toBe("err");
		expect(result.exitCode).toBe(0);
	});
});

describe("printShellResult", () => {
	test("writes stdout to process.stdout", () => {
		const chunks: string[] = [];
		const origWrite = process.stdout.write;
		process.stdout.write = ((chunk: string) => {
			chunks.push(chunk);
			return true;
		}) as typeof process.stdout.write;

		const result: ShellResult = { stdout: "hello\n", stderr: "", exitCode: 0 };
		printShellResult(result);

		process.stdout.write = origWrite;
		expect(chunks.join("")).toContain("hello");
	});

	test("writes stderr to process.stderr", () => {
		const chunks: string[] = [];
		const origWrite = process.stderr.write;
		process.stderr.write = ((chunk: string) => {
			chunks.push(chunk);
			return true;
		}) as typeof process.stderr.write;

		const result: ShellResult = { stdout: "", stderr: "oops\n", exitCode: 0 };
		printShellResult(result);

		process.stderr.write = origWrite;
		expect(chunks.join("")).toContain("oops");
	});

	test("prints exit code when non-zero", () => {
		const chunks: string[] = [];
		const origWrite = process.stderr.write;
		process.stderr.write = ((chunk: string) => {
			chunks.push(chunk);
			return true;
		}) as typeof process.stderr.write;

		const result: ShellResult = { stdout: "", stderr: "", exitCode: 42 };
		printShellResult(result);

		process.stderr.write = origWrite;
		expect(chunks.join("")).toContain("Exit code: 42");
	});

	test("does not print exit code when zero", () => {
		const stdoutChunks: string[] = [];
		const stderrChunks: string[] = [];
		const origStdout = process.stdout.write;
		const origStderr = process.stderr.write;
		process.stdout.write = ((chunk: string) => {
			stdoutChunks.push(chunk);
			return true;
		}) as typeof process.stdout.write;
		process.stderr.write = ((chunk: string) => {
			stderrChunks.push(chunk);
			return true;
		}) as typeof process.stderr.write;

		const result: ShellResult = { stdout: "ok\n", stderr: "", exitCode: 0 };
		printShellResult(result);

		process.stdout.write = origStdout;
		process.stderr.write = origStderr;
		const all = stdoutChunks.join("") + stderrChunks.join("");
		expect(all).not.toContain("Exit code");
	});
});

describe("formatShellForContext", () => {
	test("returns user message with command and stdout", () => {
		const result: ShellResult = {
			stdout: "hello world\n",
			stderr: "",
			exitCode: 0,
		};
		const msg = formatShellForContext("echo hello world", result);
		expect(msg.role).toBe("user");
		expect(msg.content).toContain("echo hello world");
		expect(msg.content).toContain("hello world");
		expect(msg.content).toContain("```");
	});

	test("includes stderr section when stderr is non-empty", () => {
		const result: ShellResult = {
			stdout: "out\n",
			stderr: "warning\n",
			exitCode: 0,
		};
		const msg = formatShellForContext("cmd", result);
		expect(msg.content).toContain("stderr");
		expect(msg.content).toContain("warning");
	});

	test("does not include stderr section when stderr is empty", () => {
		const result: ShellResult = {
			stdout: "out\n",
			stderr: "",
			exitCode: 0,
		};
		const msg = formatShellForContext("cmd", result);
		expect(msg.content).not.toContain("stderr");
	});
});
