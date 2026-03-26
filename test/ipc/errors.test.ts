import { describe, expect, it } from "bun:test";
import { Value } from "@sinclair/typebox/value";
import { type ErrorEnvelope, ErrorEnvelopeSchema, normalizeError } from "../../src/ipc/errors.ts";

describe("ErrorEnvelopeSchema", () => {
	it("validates a minimal envelope", () => {
		const envelope: ErrorEnvelope = { code: "provider_error", message: "Model error", retryable: true };
		expect(Value.Check(ErrorEnvelopeSchema, envelope)).toBe(true);
	});

	it("validates an envelope with details", () => {
		const envelope: ErrorEnvelope = {
			code: "provider_error",
			message: "Model error",
			retryable: true,
			details: { error: { message: "Model error", type: "error", code: 500 } },
		};
		expect(Value.Check(ErrorEnvelopeSchema, envelope)).toBe(true);
	});

	it("rejects envelope missing required fields", () => {
		expect(Value.Check(ErrorEnvelopeSchema, { code: "x", message: "y" })).toBe(false); // missing retryable
		expect(Value.Check(ErrorEnvelopeSchema, { code: "x", retryable: false })).toBe(false); // missing message
		expect(Value.Check(ErrorEnvelopeSchema, { message: "y", retryable: false })).toBe(false); // missing code
	});
});

describe("normalizeError", () => {
	it("normalizes an OpenRouter API error with JSON details", () => {
		const err = new Error('OpenRouter API error (500): {"error":{"message":"Model error","type":"error","code":500}}');
		const result = normalizeError(err, "provider_error");
		expect(result.code).toBe("provider_error");
		expect(result.message).toBe("Model error");
		expect(result.retryable).toBe(true);
		expect(result.provider).toBe("openrouter");
		expect(result.details).toEqual({ error: { message: "Model error", type: "error", code: 500 } });
	});

	it("normalizes a plain string error", () => {
		const result = normalizeError("Something broke", "provider_error");
		expect(result.code).toBe("provider_error");
		expect(result.message).toBe("Something broke");
		expect(result.retryable).toBe(true);
	});

	it("normalizes an Error object without API pattern", () => {
		const result = normalizeError(new Error("Connection refused"), "provider_error");
		expect(result.code).toBe("provider_error");
		expect(result.message).toBe("Connection refused");
		expect(result.retryable).toBe(true);
	});

	it("normalizes a protocol error", () => {
		const result = normalizeError("Not initialized", "protocol_error");
		expect(result.code).toBe("protocol_error");
		expect(result.message).toBe("Not initialized");
		expect(result.retryable).toBe(false);
	});

	it("normalizes a loop_detected error", () => {
		const result = normalizeError("3 iterations", "loop_detected");
		expect(result.code).toBe("loop_detected");
		expect(result.retryable).toBe(false);
	});

	it("normalizes a cancelled error", () => {
		const result = normalizeError("cancelled", "cancelled");
		expect(result.code).toBe("cancelled");
		expect(result.message).toBe("cancelled");
		expect(result.retryable).toBe(false);
	});

	it("normalizes a tool_error", () => {
		const result = normalizeError("ENOENT: no such file", "tool_error");
		expect(result.code).toBe("tool_error");
		expect(result.retryable).toBe(false);
	});

	it("normalizes a protocol_version_mismatch", () => {
		const result = normalizeError("protocol_version_mismatch", "protocol_version_mismatch");
		expect(result.code).toBe("protocol_version_mismatch");
		expect(result.retryable).toBe(false);
	});

	it("handles API error string that partially matches pattern", () => {
		const result = normalizeError("Something API error happened", "provider_error");
		// Contains "API error" but doesn't match the (code): pattern — should use label fallback
		expect(result.code).toBe("provider_error");
		expect(result.message).toBe("Provider error");
		expect(result.retryable).toBe(true);
	});

	it("extracts provider from API error message", () => {
		const err = new Error('Anthropic API error (429): {"error":{"message":"Rate limited"}}');
		const result = normalizeError(err, "provider_error");
		expect(result.provider).toBe("anthropic");
		expect(result.message).toBe("Rate limited");
	});

	it("returns envelope without provider for non-API errors", () => {
		const result = normalizeError("timeout", "provider_error");
		expect(result.provider).toBeUndefined();
	});

	it("produces a valid ErrorEnvelope when provider is stripped", () => {
		const result = normalizeError(
			new Error('OpenRouter API error (500): {"error":{"message":"Model error"}}'),
			"provider_error",
		);
		const { provider, ...envelope } = result;
		expect(Value.Check(ErrorEnvelopeSchema, envelope)).toBe(true);
	});
});
