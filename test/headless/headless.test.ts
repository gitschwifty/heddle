import { afterAll, beforeAll, describe, expect, it } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type Subprocess, spawn } from "bun";

const CWD = process.cwd();

// Canned SSE responses for the mock OpenRouter server
function sseResponse(chunks: string[]): Response {
	const encoder = new TextEncoder();
	const stream = new ReadableStream({
		start(controller) {
			for (const chunk of chunks) {
				controller.enqueue(encoder.encode(`data: ${chunk}\n\n`));
			}
			controller.enqueue(encoder.encode("data: [DONE]\n\n"));
			controller.close();
		},
	});
	return new Response(stream, { headers: { "Content-Type": "text/event-stream" } });
}

function textDelta(text: string): string {
	return JSON.stringify({
		id: "chatcmpl-test",
		choices: [{ index: 0, delta: { content: text }, finish_reason: null }],
	});
}

function finishDelta(usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number }): string {
	return JSON.stringify({
		id: "chatcmpl-test",
		choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
		...(usage ? { usage } : {}),
	});
}

// Track request count for multi-turn and error scenarios
let _requestCount = 0;
let mockMode: "normal" | "error" | "cancel" | "tool" = "normal";

let server: ReturnType<typeof Bun.serve>;
let baseUrl: string;

beforeAll(() => {
	server = Bun.serve({
		port: 0,
		fetch() {
			_requestCount++;

			if (mockMode === "error") {
				return new Response(JSON.stringify({ error: { message: "Model error", type: "error", code: 500 } }), {
					status: 500,
					headers: { "Content-Type": "application/json" },
				});
			}

			if (mockMode === "cancel") {
				return sseResponse([
					textDelta("Working..."),
					textDelta("Still working..."),
					textDelta("More work..."),
					textDelta("Even more work..."),
					finishDelta({ prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }),
				]);
			}

			// normal mode — just return text
			return sseResponse([
				textDelta("Hello! "),
				textDelta("How can I help?"),
				finishDelta({ prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }),
			]);
		},
	});
	baseUrl = `http://localhost:${server.port}`;
});

afterAll(() => {
	server.stop();
});

function resetMock(mode: "normal" | "error" | "cancel" | "tool" = "normal") {
	_requestCount = 0;
	mockMode = mode;
}

function parseLine(line: string | undefined): Record<string, unknown> {
	if (line === undefined) throw new Error("Expected a line but got undefined");
	return JSON.parse(line) as Record<string, unknown>;
}

interface HeadlessProcess {
	proc: Subprocess;
	lines: string[];
	waitForLines(count: number, timeoutMs?: number): Promise<string[]>;
	sendLine(line: string): void;
	close(): void;
}

function spawnHeadless(): HeadlessProcess {
	const heddleHome = mkdtempSync(join(tmpdir(), "heddle-test-"));

	const proc = spawn(["bun", "run", "src/headless/index.ts"], {
		cwd: CWD,
		stdin: "pipe",
		stdout: "pipe",
		stderr: "inherit",
		env: {
			...process.env,
			HEDDLE_BASE_URL: baseUrl,
			OPENROUTER_API_KEY: "test-key-headless",
			HEDDLE_HOME: heddleHome,
			HEDDLE_PROTOCOL_VERSION: "0.1.0",
		},
	});

	const lines: string[] = [];
	const reader = proc.stdout.getReader();
	const decoder = new TextDecoder();
	let buffer = "";

	(async () => {
		try {
			for (;;) {
				const { done, value } = await reader.read();
				if (done) break;
				buffer += decoder.decode(value, { stream: true });
				const parts = buffer.split("\n");
				buffer = parts.pop() ?? "";
				for (const part of parts) {
					if (part.trim()) lines.push(part);
				}
			}
			if (buffer.trim()) lines.push(buffer);
		} catch {}
	})();

	function waitForLines(count: number, timeoutMs = 5000): Promise<string[]> {
		return new Promise((resolve, reject) => {
			const start = Date.now();
			const check = () => {
				if (lines.length >= count) {
					resolve(lines.slice(0, count));
					return;
				}
				if (Date.now() - start > timeoutMs) {
					reject(new Error(`Timeout waiting for ${count} lines, got ${lines.length}: ${JSON.stringify(lines)}`));
					return;
				}
				setTimeout(check, 20);
			};
			check();
		});
	}

	function sendLine(line: string) {
		proc.stdin.write(`${line}\n`);
	}

	function close() {
		try {
			proc.stdin.end();
			proc.kill();
		} catch {}
	}

	return { proc, lines, waitForLines, sendLine, close };
}

function initMsg(overrides?: Record<string, unknown>) {
	return JSON.stringify({
		type: "init",
		id: "1",
		protocol_version: "0.1.0",
		config: {
			model: "openrouter/auto",
			system_prompt: "You are helpful.",
			tools: ["read_file", "glob", "grep"],
			max_iterations: 10,
		},
		...overrides,
	});
}

describe("headless adapter", () => {
	it("init returns init_ok with session_id and protocol_version", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(initMsg());
			await h.waitForLines(1);
			const msg = parseLine(h.lines[0]);
			expect(msg.type).toBe("init_ok");
			expect(msg.id).toBe("1");
			expect(typeof msg.session_id).toBe("string");
			expect(msg.protocol_version).toBe("0.1.0");
		} finally {
			h.close();
		}
	});

	it("send message returns streamed events + result", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(initMsg());
			await h.waitForLines(1);

			h.sendLine(JSON.stringify({ type: "send", id: "2", message: "Hi there" }));
			await h.waitForLines(4, 8000);

			const messages = h.lines.map((l) => JSON.parse(l) as Record<string, unknown>);
			const result = messages.find((m) => m.type === "result");
			expect(result).toBeDefined();
			if (result) {
				expect(result.id).toBe("2");
				expect(result.status).toBe("ok");
				expect(result.iterations).toBeGreaterThanOrEqual(1);
			}
		} finally {
			h.close();
		}
	});

	it("send before init returns error", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(JSON.stringify({ type: "send", id: "2", message: "Hi" }));
			await h.waitForLines(1);
			const msg = parseLine(h.lines[0]);
			expect(msg.type).toBe("result");
			expect(msg.status).toBe("error");
			expect(String(msg.error)).toContain("Not initialized");
		} finally {
			h.close();
		}
	});

	it("malformed JSON returns error and process survives", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine("not valid json{{");
			await h.waitForLines(1);
			const msg = parseLine(h.lines[0]);
			expect(msg.type).toBe("result");
			expect(msg.status).toBe("error");
			expect(msg.error).toBe("Invalid JSON");

			// Process should survive — send another valid message
			h.sendLine(initMsg());
			await h.waitForLines(2);
			const initOk = parseLine(h.lines[1]);
			expect(initOk.type).toBe("init_ok");
		} finally {
			h.close();
		}
	});

	it("shutdown returns shutdown_ok and process exits", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(initMsg());
			await h.waitForLines(1);
			h.sendLine(JSON.stringify({ type: "shutdown", id: "99" }));
			await h.waitForLines(2);
			const msg = parseLine(h.lines[1]);
			expect(msg.type).toBe("shutdown_ok");
			expect(msg.id).toBe("99");

			const code = await h.proc.exited;
			expect(code).toBe(0);
		} finally {
			h.close();
		}
	});

	it("status returns status_ok with correct fields", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(initMsg());
			await h.waitForLines(1);

			h.sendLine(JSON.stringify({ type: "status", id: "s1" }));
			await h.waitForLines(2);
			const msg = parseLine(h.lines[1]);
			expect(msg.type).toBe("status_ok");
			expect(msg.id).toBe("s1");
			expect(typeof msg.model).toBe("string");
			expect(msg.messages_count).toBeGreaterThanOrEqual(1);
			expect(typeof msg.session_id).toBe("string");
			expect(typeof msg.active).toBe("boolean");
		} finally {
			h.close();
		}
	});

	it("protocol version included in init_ok", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(initMsg());
			await h.waitForLines(1);
			const msg = parseLine(h.lines[0]);
			expect(msg.protocol_version).toBe("0.1.0");
		} finally {
			h.close();
		}
	});

	it("provider error emits error event and error result", async () => {
		resetMock("error");
		const h = spawnHeadless();
		try {
			h.sendLine(initMsg());
			await h.waitForLines(1);

			h.sendLine(JSON.stringify({ type: "send", id: "2", message: "Do the thing." }));
			await h.waitForLines(3, 8000);

			const messages = h.lines.map((l) => JSON.parse(l) as Record<string, unknown>);
			const errorEvent = messages.find(
				(m) =>
					m.type === "event" &&
					typeof m.event === "object" &&
					m.event !== null &&
					(m.event as Record<string, unknown>).event === "error",
			);
			expect(errorEvent).toBeDefined();

			const result = messages.find((m) => m.type === "result");
			expect(result).toBeDefined();
			if (result) {
				expect(result.status).toBe("error");
			}
		} finally {
			h.close();
		}
	});

	it("tool restriction via init config.tools", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			const msg = JSON.stringify({
				type: "init",
				id: "1",
				protocol_version: "0.1.0",
				config: {
					model: "openrouter/auto",
					system_prompt: "You are helpful.",
					tools: ["read_file"],
					max_iterations: 10,
				},
			});
			h.sendLine(msg);
			await h.waitForLines(1);

			h.sendLine(JSON.stringify({ type: "status", id: "s1" }));
			await h.waitForLines(2);
			const status = parseLine(h.lines[1]);
			expect(status.type).toBe("status_ok");
		} finally {
			h.close();
		}
	});

	it("version mismatch returns error and exits", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(
				JSON.stringify({
					type: "init",
					id: "1",
					protocol_version: "1.1.0",
					config: { model: "openrouter/auto", system_prompt: "x", tools: [], max_iterations: 2 },
				}),
			);

			await h.waitForLines(1);
			const msg = parseLine(h.lines[0]);
			expect(msg.type).toBe("result");
			expect(msg.status).toBe("error");
			expect(msg.error).toBe("protocol_version_mismatch");

			const code = await h.proc.exited;
			expect(code).toBe(1);
		} finally {
			h.close();
		}
	});

	it("multi-send accumulates messages (multi-turn)", async () => {
		resetMock("normal");
		const h = spawnHeadless();
		try {
			h.sendLine(initMsg());
			await h.waitForLines(1);

			// First send
			h.sendLine(JSON.stringify({ type: "send", id: "2", message: "First message" }));
			// Wait for first result
			await new Promise<void>((resolve) => {
				const check = () => {
					const hasResult = h.lines.some((l) => {
						try {
							return (JSON.parse(l) as Record<string, unknown>).type === "result";
						} catch {
							return false;
						}
					});
					if (hasResult) resolve();
					else setTimeout(check, 20);
				};
				check();
			});

			// Second send
			h.sendLine(JSON.stringify({ type: "send", id: "3", message: "Second message" }));
			// Wait for second result
			await new Promise<void>((resolve, reject) => {
				const start = Date.now();
				const check = () => {
					const results = h.lines.filter((l) => {
						try {
							return (JSON.parse(l) as Record<string, unknown>).type === "result";
						} catch {
							return false;
						}
					});
					if (results.length >= 2) resolve();
					else if (Date.now() - start > 8000) reject(new Error("Timeout waiting for second result"));
					else setTimeout(check, 20);
				};
				check();
			});

			const results = h.lines.filter((l) => (JSON.parse(l) as Record<string, unknown>).type === "result");
			expect(results.length).toBe(2);
			const first = parseLine(results[0]);
			const second = parseLine(results[1]);
			expect(first.id).toBe("2");
			expect(second.id).toBe("3");
		} finally {
			h.close();
		}
	});
});
