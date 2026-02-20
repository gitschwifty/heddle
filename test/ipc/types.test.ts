import { describe, expect, it } from "bun:test";
import { Value } from "@sinclair/typebox/value";
import { InitConfigSchema, IpcRequestSchema, IpcResponseSchema, WorkerEventSchema } from "../../src/ipc/types.ts";

describe("IPC schemas", () => {
	describe("InitConfigSchema", () => {
		it("validates a complete config", () => {
			expect(
				Value.Check(InitConfigSchema, {
					model: "openrouter/auto",
					system_prompt: "You are helpful.",
					tools: ["read_file", "glob"],
					max_iterations: 10,
				}),
			).toBe(true);
		});

		it("validates config without optional max_iterations", () => {
			expect(
				Value.Check(InitConfigSchema, {
					model: "openrouter/auto",
					system_prompt: "prompt",
					tools: [],
				}),
			).toBe(true);
		});

		it("rejects config missing model", () => {
			expect(
				Value.Check(InitConfigSchema, {
					system_prompt: "prompt",
					tools: [],
				}),
			).toBe(false);
		});
	});

	describe("IpcRequestSchema", () => {
		it("validates init request", () => {
			expect(
				Value.Check(IpcRequestSchema, {
					type: "init",
					id: "1",
					protocol_version: "0.1.0",
					config: { model: "m", system_prompt: "s", tools: [] },
				}),
			).toBe(true);
		});

		it("validates send request", () => {
			expect(Value.Check(IpcRequestSchema, { type: "send", id: "2", message: "hello" })).toBe(true);
		});

		it("validates cancel request", () => {
			expect(Value.Check(IpcRequestSchema, { type: "cancel", id: "3", target_id: "2" })).toBe(true);
		});

		it("rejects unknown type", () => {
			expect(Value.Check(IpcRequestSchema, { type: "unknown", id: "1" })).toBe(false);
		});
	});

	describe("WorkerEventSchema", () => {
		it("validates content_delta", () => {
			expect(Value.Check(WorkerEventSchema, { event: "content_delta", text: "hello" })).toBe(true);
		});

		it("validates error with code", () => {
			expect(Value.Check(WorkerEventSchema, { event: "error", error: "fail", code: "loop_detected" })).toBe(true);
		});
	});

	describe("IpcResponseSchema", () => {
		it("validates init_ok", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "init_ok",
					id: "1",
					session_id: "sess-1",
					protocol_version: "0.1.0",
				}),
			).toBe(true);
		});

		it("validates result", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "result",
					id: "2",
					status: "ok",
					tool_calls_made: [],
					iterations: 1,
				}),
			).toBe(true);
		});
	});
});
