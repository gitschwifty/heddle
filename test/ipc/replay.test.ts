import { describe, expect, it } from "bun:test";
import { spawn } from "node:child_process";
import fs, { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import path, { join } from "node:path";
import readline from "node:readline";
import { validateIpcMessage } from "../../src/ipc/schema";

const FIXTURES_DIR = path.resolve(process.cwd(), "test/ipc/fixtures");

const IGNORE_PATHS: string[] = [
	"session_id",
	"timestamp",
	"usage.prompt_tokens",
	"usage.completion_tokens",
	"usage.total_tokens",
	"event.result_preview",
	"event.details",
	"event.provider",
	"task_id",
	"worker_id",
	"model_latency_ms",
	"tool_latency_ms",
	"total_latency_ms",
];

function deletePath(obj: Record<string, unknown>, pathStr: string) {
	const parts = pathStr.split(".");
	let cur: Record<string, unknown> = obj;
	for (let i = 0; i < parts.length - 1; i++) {
		const key = parts[i];
		if (!cur || typeof cur !== "object" || key === undefined) return;
		cur = cur[key] as Record<string, unknown>;
	}
	const lastKey = parts[parts.length - 1];
	if (cur && typeof cur === "object" && lastKey !== undefined) delete cur[lastKey];
}

function stripIgnored(obj: unknown): unknown {
	const clone = JSON.parse(JSON.stringify(obj));
	for (const p of IGNORE_PATHS) deletePath(clone, p);
	return clone;
}

// SSE mock helpers
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

function finishDelta(usage?: Record<string, number>): string {
	return JSON.stringify({
		id: "chatcmpl-test",
		choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
		...(usage ? { usage } : {}),
	});
}

function toolCallChunks(name: string, args: string): string[] {
	return [
		JSON.stringify({
			id: "chatcmpl-test",
			choices: [
				{
					index: 0,
					delta: { tool_calls: [{ index: 0, id: "call_0", type: "function", function: { name, arguments: "" } }] },
					finish_reason: null,
				},
			],
		}),
		JSON.stringify({
			id: "chatcmpl-test",
			choices: [
				{
					index: 0,
					delta: { tool_calls: [{ index: 0, function: { arguments: args } }] },
					finish_reason: null,
				},
			],
		}),
		JSON.stringify({
			id: "chatcmpl-test",
			choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }],
		}),
	];
}

function createMockServer(mode: "normal" | "error" | "cancel" | "heartbeat"): ReturnType<typeof Bun.serve> {
	let requestCount = 0;
	return Bun.serve({
		port: 0,
		async fetch() {
			requestCount++;

			if (mode === "error") {
				return new Response(JSON.stringify({ error: { message: "Model error", type: "error", code: 500 } }), {
					status: 500,
					headers: { "Content-Type": "application/json" },
				});
			}

			if (mode === "cancel") {
				return sseResponse([
					textDelta("Working..."),
					textDelta("Still working..."),
					textDelta("More work..."),
					finishDelta({ prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }),
				]);
			}

			if (mode === "heartbeat") {
				// Delay to ensure heartbeat fires at least once
				await new Promise((r) => setTimeout(r, 250));
				if (requestCount === 1) {
					return sseResponse(toolCallChunks("read_file", '{"path":"src/main.rs"}'));
				}
				return sseResponse([
					textDelta("The codebase has..."),
					finishDelta({ prompt_tokens: 120, completion_tokens: 30, total_tokens: 150 }),
				]);
			}

			// normal: first request returns glob tool call, second returns text
			if (requestCount === 1) {
				return sseResponse(toolCallChunks("glob", '{"pattern":"*"}'));
			}
			return sseResponse([
				textDelta("Here are the files..."),
				finishDelta({ prompt_tokens: 42, completion_tokens: 15, total_tokens: 57 }),
			]);
		},
	});
}

async function runFixtureStrict(name: string): Promise<void> {
	const inPath = path.join(FIXTURES_DIR, `${name}.in.jsonl`);
	const outPath = path.join(FIXTURES_DIR, `${name}.out.jsonl`);

	const inputLines = fs.readFileSync(inPath, "utf8").split("\n").filter(Boolean);
	const expectedLines = fs.readFileSync(outPath, "utf8").split("\n").filter(Boolean);

	const heddleHome = mkdtempSync(join(tmpdir(), "heddle-fixture-"));

	const mode =
		name === "error" ? "error" : name === "cancel" ? "cancel" : name === "heartbeat" ? "heartbeat" : "normal";
	const needsServer = name !== "version-mismatch";
	const server = needsServer ? createMockServer(mode) : null;
	const baseUrl = server ? `http://localhost:${server.port}` : "http://localhost:1";

	try {
		const child = spawn("bun", ["src/headless/index.ts"], {
			cwd: process.cwd(),
			stdio: ["pipe", "pipe", "pipe"],
			env: {
				...process.env,
				HEDDLE_BASE_URL: baseUrl,
				OPENROUTER_API_KEY: "test-key-fixture",
				HEDDLE_HOME: heddleHome,
				HEDDLE_PROTOCOL_VERSION: "0.2.0",
			},
		});

		// Drain stderr to prevent pipe blocking
		let _stderr = "";
		child.stderr!.on("data", (d: Buffer) => {
			_stderr += d.toString();
		});

		const rl = readline.createInterface({ input: child.stdout! });

		// Write all input lines
		for (const line of inputLines) {
			child.stdin!.write(`${line}\n`);
		}

		// Collect output
		const output: string[] = [];
		const timeoutMs = 10000;

		await new Promise<void>((resolve, reject) => {
			const timer = setTimeout(() => {
				child.kill();
				reject(new Error(`fixture "${name}" timeout after ${timeoutMs}ms, got ${output.length} lines`));
			}, timeoutMs);

			rl.on("line", (line: string) => {
				output.push(line);
				const msg = JSON.parse(line);
				expect(validateIpcMessage(msg)).toBe(true);
			});

			child.on("close", (_code: number | null) => {
				clearTimeout(timer);
				resolve();
			});
		});

		child.stdin!.end();

		expect(output.length).toBeGreaterThan(0);
		expect(output.length).toBe(expectedLines.length);

		for (let i = 0; i < expectedLines.length; i++) {
			const expectedLine = expectedLines[i];
			const actualLine = output[i];
			if (expectedLine === undefined || actualLine === undefined) {
				throw new Error(`Missing line at index ${i}`);
			}
			const expected = stripIgnored(JSON.parse(expectedLine));
			const actual = stripIgnored(JSON.parse(actualLine));
			expect(actual).toEqual(expected);
		}
	} finally {
		if (server) server.stop();
	}
}

describe("ipc fixtures", () => {
	it("normal", async () => {
		await runFixtureStrict("normal");
	});

	it("error", async () => {
		await runFixtureStrict("error");
	});

	it("cancel", async () => {
		await runFixtureStrict("cancel");
	});

	it("version mismatch", async () => {
		await runFixtureStrict("version-mismatch");
	});

	it("heartbeat", async () => {
		const inPath = path.join(FIXTURES_DIR, "heartbeat.in.jsonl");
		const outPath = path.join(FIXTURES_DIR, "heartbeat.out.jsonl");

		const inputLines = fs.readFileSync(inPath, "utf8").split("\n").filter(Boolean);
		const expectedLines = fs.readFileSync(outPath, "utf8").split("\n").filter(Boolean);

		const heddleHome = mkdtempSync(join(tmpdir(), "heddle-fixture-"));
		const server = createMockServer("heartbeat");

		try {
			const child = spawn("bun", ["src/headless/index.ts"], {
				cwd: process.cwd(),
				stdio: ["pipe", "pipe", "pipe"],
				env: {
					...process.env,
					HEDDLE_BASE_URL: `http://localhost:${server.port}`,
					OPENROUTER_API_KEY: "test-key-fixture",
					HEDDLE_HOME: heddleHome,
					HEDDLE_PROTOCOL_VERSION: "0.2.0",
					HEDDLE_HEARTBEAT_INTERVAL: "100",
				},
			});

			let _stderr = "";
			child.stderr!.on("data", (d: Buffer) => {
				_stderr += d.toString();
			});

			const rl = readline.createInterface({ input: child.stdout! });

			for (const line of inputLines) {
				child.stdin!.write(`${line}\n`);
			}

			const output: string[] = [];
			const timeoutMs = 10000;

			await new Promise<void>((resolve, reject) => {
				const timer = setTimeout(() => {
					child.kill();
					reject(new Error(`fixture "heartbeat" timeout after ${timeoutMs}ms, got ${output.length} lines`));
				}, timeoutMs);

				rl.on("line", (line: string) => {
					output.push(line);
					const msg = JSON.parse(line);
					expect(validateIpcMessage(msg)).toBe(true);
				});

				child.on("close", () => {
					clearTimeout(timer);
					resolve();
				});
			});

			child.stdin!.end();

			// Separate heartbeat events from other messages
			const heartbeats = output.filter((line) => {
				const msg = JSON.parse(line);
				return msg.type === "event" && msg.event?.event === "heartbeat";
			});
			const nonHeartbeats = output.filter((line) => {
				const msg = JSON.parse(line);
				return !(msg.type === "event" && msg.event?.event === "heartbeat");
			});

			// At least one heartbeat must have fired
			expect(heartbeats.length).toBeGreaterThanOrEqual(1);

			// Heartbeat events must have duration_ms and sequential event_seq
			for (const hbLine of heartbeats) {
				const hb = JSON.parse(hbLine);
				expect(hb.event.event).toBe("heartbeat");
				expect(typeof hb.event.duration_ms).toBe("number");
				expect(hb.event.duration_ms).toBeGreaterThan(0);
				expect(typeof hb.event_seq).toBe("number");
				expect(hb.send_id).toBe("2");
			}

			// Compare non-heartbeat output against expected (also with heartbeats filtered out)
			const expectedNonHeartbeats = expectedLines.filter((line) => {
				const msg = JSON.parse(line);
				return !(msg.type === "event" && msg.event?.event === "heartbeat");
			});

			expect(nonHeartbeats.length).toBe(expectedNonHeartbeats.length);
			for (let i = 0; i < expectedNonHeartbeats.length; i++) {
				const expected = stripIgnored(JSON.parse(expectedNonHeartbeats[i]!));
				const actual = stripIgnored(JSON.parse(nonHeartbeats[i]!));
				// Heartbeats shift event_seq, so strip it for comparison
				if (typeof (expected as Record<string, unknown>).event_seq === "number") {
					delete (expected as Record<string, unknown>).event_seq;
				}
				if (typeof (actual as Record<string, unknown>).event_seq === "number") {
					delete (actual as Record<string, unknown>).event_seq;
				}
				expect(actual).toEqual(expected);
			}
		} finally {
			server.stop();
		}
	});
});
