import { describe, expect, it } from "bun:test";
import {
	buildError,
	buildResult,
	type CorrelationContext,
	decodeRequest,
	encodeResponse,
	wrapEvent,
} from "../../src/ipc/codec.ts";
import type { IpcResponse, WorkerEvent } from "../../src/ipc/types.ts";

describe("encodeResponse", () => {
	it("serializes to JSON without trailing newline", () => {
		const res: IpcResponse = { type: "shutdown_ok", id: "1" } as IpcResponse;
		const encoded = encodeResponse(res);
		expect(encoded).toBe('{"type":"shutdown_ok","id":"1"}');
		expect(encoded.endsWith("\n")).toBe(false);
	});
});

describe("decodeRequest", () => {
	it("decodes a valid init request", () => {
		const line = JSON.stringify({ type: "init", id: "1", config: { model: "m", system_prompt: "s", tools: [] } });
		const result = decodeRequest(line);
		expect(result.ok).toBe(true);
		if (result.ok) {
			expect(result.request.type).toBe("init");
		}
	});

	it("returns error for invalid JSON", () => {
		const result = decodeRequest("not json{");
		expect(result.ok).toBe(false);
		if (!result.ok) expect(result.error).toBe("Invalid JSON");
	});

	it("returns error for missing type field", () => {
		const result = decodeRequest(JSON.stringify({ id: "1" }));
		expect(result.ok).toBe(false);
		if (!result.ok) expect(result.error).toBe("Missing 'type' field");
	});

	it("returns error for missing id field", () => {
		const result = decodeRequest(JSON.stringify({ type: "send" }));
		expect(result.ok).toBe(false);
		if (!result.ok) expect(result.error).toBe("Missing 'id' field");
	});

	it("returns error for non-object JSON", () => {
		const result = decodeRequest('"hello"');
		expect(result.ok).toBe(false);
		if (!result.ok) expect(result.error).toBe("Expected JSON object");
	});
});

describe("wrapEvent", () => {
	it("wraps a worker event with send_id and event_seq", () => {
		const event: WorkerEvent = { event: "content_delta", text: "hi" } as WorkerEvent;
		const wrapped = wrapEvent(event, "send-1", 0);
		expect(wrapped).toEqual({
			type: "event",
			event: { event: "content_delta", text: "hi" },
			send_id: "send-1",
			event_seq: 0,
		});
	});

	it("passes through higher sequence numbers", () => {
		const event: WorkerEvent = { event: "content_delta", text: "x" } as WorkerEvent;
		const wrapped = wrapEvent(event, "s2", 5);
		expect((wrapped as Record<string, unknown>).event_seq).toBe(5);
		expect((wrapped as Record<string, unknown>).send_id).toBe("s2");
	});

	it("includes correlation context when provided", () => {
		const event: WorkerEvent = { event: "content_delta", text: "hi" } as WorkerEvent;
		const ctx: CorrelationContext = { session_id: "sess-1", task_id: "task-1", worker_id: "worker-0" };
		const wrapped = wrapEvent(event, "send-1", 0, ctx);
		expect(wrapped).toEqual({
			type: "event",
			event: { event: "content_delta", text: "hi" },
			send_id: "send-1",
			event_seq: 0,
			session_id: "sess-1",
			task_id: "task-1",
			worker_id: "worker-0",
		});
	});

	it("omits undefined correlation fields", () => {
		const event: WorkerEvent = { event: "content_delta", text: "hi" } as WorkerEvent;
		const ctx: CorrelationContext = { session_id: "sess-1" };
		const wrapped = wrapEvent(event, "send-1", 0, ctx);
		const obj = wrapped as Record<string, unknown>;
		expect(obj.session_id).toBe("sess-1");
		expect("task_id" in obj).toBe(false);
		expect("worker_id" in obj).toBe(false);
	});

	it("works without correlation context (backward compatible)", () => {
		const event: WorkerEvent = { event: "content_delta", text: "hi" } as WorkerEvent;
		const wrapped = wrapEvent(event, "send-1", 0);
		const obj = wrapped as Record<string, unknown>;
		expect("session_id" in obj).toBe(false);
		expect("task_id" in obj).toBe(false);
		expect("worker_id" in obj).toBe(false);
	});
});

describe("buildResult", () => {
	it("builds a successful result", () => {
		const result = buildResult("2", {
			status: "ok",
			response: "Hello!",
			toolCallsMade: [{ name: "glob", args: { pattern: "*" } }],
			usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
			iterations: 1,
		});
		expect(result).toEqual({
			type: "result",
			id: "2",
			status: "ok",
			response: "Hello!",
			tool_calls_made: [{ name: "glob", args: { pattern: "*" } }],
			usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
			iterations: 1,
			error: undefined,
		});
	});

	it("builds an error result with ErrorEnvelope", () => {
		const result = buildResult("3", {
			status: "error",
			error: { code: "provider_error", message: "something broke", retryable: true },
			toolCallsMade: [],
			iterations: 0,
		});
		expect(result).toMatchObject({
			type: "result",
			id: "3",
			status: "error",
			error: { code: "provider_error", message: "something broke", retryable: true },
		});
	});

	it("builds result with error envelope including details", () => {
		const result = buildResult("4", {
			status: "error",
			error: { code: "protocol_error", message: "bad", retryable: false, details: { info: "extra" } },
			toolCallsMade: [],
			iterations: 0,
		});
		expect((result as Record<string, unknown>).error).toEqual({
			code: "protocol_error",
			message: "bad",
			retryable: false,
			details: { info: "extra" },
		});
	});
});

describe("buildResult with correlation and latency", () => {
	it("includes correlation IDs when provided", () => {
		const result = buildResult("2", {
			status: "ok",
			response: "Hello!",
			toolCallsMade: [],
			iterations: 1,
			correlation: { session_id: "sess-1", task_id: "task-1", worker_id: "worker-0" },
		});
		const obj = result as Record<string, unknown>;
		expect(obj.session_id).toBe("sess-1");
		expect(obj.task_id).toBe("task-1");
		expect(obj.worker_id).toBe("worker-0");
	});

	it("includes latency fields when provided", () => {
		const result = buildResult("2", {
			status: "ok",
			response: "Hello!",
			toolCallsMade: [],
			iterations: 1,
			totalLatencyMs: 200,
			modelLatencyMs: 150,
			toolLatencyMs: 50,
		});
		const obj = result as Record<string, unknown>;
		expect(obj.total_latency_ms).toBe(200);
		expect(obj.model_latency_ms).toBe(150);
		expect(obj.tool_latency_ms).toBe(50);
	});

	it("omits correlation and latency fields when not provided", () => {
		const result = buildResult("2", {
			status: "ok",
			toolCallsMade: [],
			iterations: 1,
		});
		const obj = result as Record<string, unknown>;
		expect("session_id" in obj).toBe(false);
		expect("task_id" in obj).toBe(false);
		expect("worker_id" in obj).toBe(false);
		expect("total_latency_ms" in obj).toBe(false);
		expect("model_latency_ms" in obj).toBe(false);
		expect("tool_latency_ms" in obj).toBe(false);
	});
});

describe("buildError", () => {
	it("builds an error with ErrorEnvelope", () => {
		const result = buildError("5", { code: "protocol_error", message: "bad request", retryable: false });
		expect(result).toEqual({
			type: "result",
			id: "5",
			status: "error",
			error: { code: "protocol_error", message: "bad request", retryable: false },
			tool_calls_made: [],
			iterations: 0,
		});
	});

	it("uses 'unknown' when id is undefined", () => {
		const result = buildError(undefined, { code: "protocol_error", message: "parse error", retryable: false });
		expect(result).toMatchObject({
			id: "unknown",
			error: { code: "protocol_error", message: "parse error", retryable: false },
		});
	});

	it("includes correlation context when provided", () => {
		const result = buildError(
			"5",
			{ code: "protocol_error", message: "bad", retryable: false },
			{ session_id: "sess-1", task_id: "task-1" },
		);
		const obj = result as Record<string, unknown>;
		expect(obj.session_id).toBe("sess-1");
		expect(obj.task_id).toBe("task-1");
		expect("worker_id" in obj).toBe(false);
	});
});
