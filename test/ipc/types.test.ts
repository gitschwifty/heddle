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

		it("validates config with task_id and worker_id", () => {
			expect(
				Value.Check(InitConfigSchema, {
					model: "openrouter/auto",
					system_prompt: "prompt",
					tools: [],
					task_id: "task-123",
					worker_id: "worker-0",
				}),
			).toBe(true);
		});

		it("validates config without optional task_id and worker_id", () => {
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

		it("validates error with code, message, retryable", () => {
			expect(
				Value.Check(WorkerEventSchema, {
					event: "error",
					code: "loop_detected",
					message: "Doom loop detected",
					retryable: false,
				}),
			).toBe(true);
		});

		it("validates error with provider and details", () => {
			expect(
				Value.Check(WorkerEventSchema, {
					event: "error",
					code: "provider_error",
					message: "Model error",
					retryable: true,
					provider: "openrouter",
					details: { error: { message: "Model error", type: "error", code: 500 } },
				}),
			).toBe(true);
		});

		it("rejects error missing required retryable field", () => {
			expect(Value.Check(WorkerEventSchema, { event: "error", code: "provider_error", message: "fail" })).toBe(false);
		});

		it("rejects error missing required code field", () => {
			expect(Value.Check(WorkerEventSchema, { event: "error", message: "fail", retryable: false })).toBe(false);
		});

		it("rejects old flat error shape (error string instead of message)", () => {
			expect(Value.Check(WorkerEventSchema, { event: "error", error: "something broke" })).toBe(false);
		});

		it("validates context_prune event with required fields", () => {
			expect(
				Value.Check(WorkerEventSchema, {
					event: "context_prune",
					messages_pruned: 5,
					tokens_before: 50000,
					tokens_after: 30000,
				}),
			).toBe(true);
		});

		it("rejects context_prune missing required fields", () => {
			expect(
				Value.Check(WorkerEventSchema, {
					event: "context_prune",
					messages_pruned: 5,
				}),
			).toBe(false);
		});

		it("validates context_compact placeholder", () => {
			expect(Value.Check(WorkerEventSchema, { event: "context_compact" })).toBe(true);
		});

		it("validates context_handoff placeholder", () => {
			expect(Value.Check(WorkerEventSchema, { event: "context_handoff" })).toBe(true);
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

		it("validates init_ok with error envelope", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "init_ok",
					id: "1",
					session_id: "sess-1",
					protocol_version: "0.1.0",
					error: { code: "protocol_error", message: "bad config", retryable: false },
				}),
			).toBe(true);
		});

		it("validates result without error", () => {
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

		it("validates result with ErrorEnvelope", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "result",
					id: "2",
					status: "error",
					tool_calls_made: [],
					iterations: 0,
					error: { code: "provider_error", message: "Model error", retryable: true },
				}),
			).toBe(true);
		});

		it("validates result with ErrorEnvelope including details", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "result",
					id: "2",
					status: "error",
					tool_calls_made: [],
					iterations: 0,
					error: { code: "protocol_error", message: "bad", retryable: false, details: "extra info" },
				}),
			).toBe(true);
		});

		it("rejects result with old flat error string", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "result",
					id: "2",
					status: "error",
					tool_calls_made: [],
					iterations: 0,
					error: "Model error",
				}),
			).toBe(false);
		});

		it("validates event with event_seq and send_id", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "event",
					event: { event: "content_delta", text: "hi" },
					event_seq: 0,
					send_id: "2",
				}),
			).toBe(true);
		});

		it("validates event with correlation IDs", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "event",
					event: { event: "content_delta", text: "hi" },
					event_seq: 0,
					send_id: "2",
					session_id: "sess-1",
					task_id: "task-1",
					worker_id: "worker-0",
				}),
			).toBe(true);
		});

		it("validates event without optional correlation IDs", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "event",
					event: { event: "content_delta", text: "hi" },
					event_seq: 0,
					send_id: "2",
				}),
			).toBe(true);
		});

		it("validates result with correlation IDs and latency fields", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "result",
					id: "2",
					status: "ok",
					tool_calls_made: [],
					iterations: 1,
					session_id: "sess-1",
					task_id: "task-1",
					worker_id: "worker-0",
					model_latency_ms: 150,
					tool_latency_ms: 50,
					total_latency_ms: 200,
				}),
			).toBe(true);
		});

		it("validates result without optional correlation and latency fields", () => {
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

		it("rejects event missing event_seq", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "event",
					event: { event: "content_delta", text: "hi" },
					send_id: "2",
				}),
			).toBe(false);
		});

		it("rejects event missing send_id", () => {
			expect(
				Value.Check(IpcResponseSchema, {
					type: "event",
					event: { event: "content_delta", text: "hi" },
					event_seq: 0,
				}),
			).toBe(false);
		});
	});
});
