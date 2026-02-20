import { describe, expect, it } from "bun:test";
import { buildError, buildResult, decodeRequest, encodeResponse, wrapEvent } from "../../src/ipc/codec.ts";
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
	it("wraps a worker event in an IPC response", () => {
		const event: WorkerEvent = { event: "content_delta", text: "hi" } as WorkerEvent;
		const wrapped = wrapEvent(event);
		expect(wrapped).toEqual({ type: "event", event: { event: "content_delta", text: "hi" } });
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

	it("builds an error result", () => {
		const result = buildResult("3", {
			status: "error",
			error: "something broke",
			toolCallsMade: [],
			iterations: 0,
		});
		expect(result).toMatchObject({ type: "result", id: "3", status: "error", error: "something broke" });
	});
});

describe("buildError", () => {
	it("builds an error with id", () => {
		const result = buildError("5", "bad request");
		expect(result).toEqual({
			type: "result",
			id: "5",
			status: "error",
			error: "bad request",
			tool_calls_made: [],
			iterations: 0,
		});
	});

	it("uses 'unknown' when id is undefined", () => {
		const result = buildError(undefined, "parse error");
		expect(result).toMatchObject({ id: "unknown", error: "parse error" });
	});
});
