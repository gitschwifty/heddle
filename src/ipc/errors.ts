import { type Static, Type } from "@sinclair/typebox";

// ── Schema ───────────────────────────────────────────────────────────

export const ErrorEnvelopeSchema = Type.Object({
	code: Type.String(),
	message: Type.String(),
	retryable: Type.Boolean(),
	details: Type.Optional(Type.Unknown()),
});

export type ErrorEnvelope = Static<typeof ErrorEnvelopeSchema>;

/**
 * ErrorEnvelope + optional provider field.
 * Callers that need both (e.g. headless error events) destructure:
 *   const { provider, ...envelope } = normalizeError(err);
 */
export interface NormalizedError extends ErrorEnvelope {
	provider?: string;
}

// ── Retryable mapping ────────────────────────────────────────────────

const RETRYABLE_CODES = new Set(["provider_error"]);

// ── Internals (ported from headless/index.ts) ────────────────────────

const ERROR_CODE_LABELS: Record<string, string> = {
	provider_error: "Provider error",
	tool_error: "Tool error",
	protocol_error: "Protocol error",
	loop_detected: "Doom loop detected",
	timeout: "Timeout",
};

/** Pattern: "OpenRouter API error (500): {json...}" or "OpenRouter API error (500): text" */
const PROVIDER_ERROR_RE = /^(.+?)\s+API error\s+\((\d+)\):\s*([\s\S]*)$/;

function extractErrorMessage(err: unknown): string {
	if (err instanceof Error) return err.message;
	if (typeof err === "string") return err;
	try {
		return JSON.stringify(err);
	} catch {
		return String(err);
	}
}

// ── Public API ───────────────────────────────────────────────────────

export function normalizeError(err: unknown, code = "provider_error"): NormalizedError {
	const retryable = RETRYABLE_CODES.has(code);
	const raw = extractErrorMessage(err);
	const match = PROVIDER_ERROR_RE.exec(raw);

	if (!match) {
		if (raw.includes("API error")) {
			return { code, message: ERROR_CODE_LABELS[code] ?? "Provider error", retryable, details: raw };
		}
		return { code, message: raw, retryable, details: typeof err === "string" ? undefined : err };
	}

	const provider = (match[1] ?? "unknown").toLowerCase();
	const rawDetails = match[3] ?? "";

	let details: unknown;
	try {
		details = JSON.parse(rawDetails);
	} catch {
		details = rawDetails;
	}

	let innerMsg: string | undefined;
	if (typeof details === "string" && details.trim()) {
		innerMsg = details.trim();
	}

	// Extract inner error message from parsed details (e.g. {error:{message:"Model error"}})
	if (details && typeof details === "object" && "error" in (details as Record<string, unknown>)) {
		const inner = (details as Record<string, unknown>).error;
		if (inner && typeof inner === "object" && "message" in (inner as Record<string, unknown>)) {
			innerMsg = String((inner as Record<string, unknown>).message);
		} else if (typeof inner === "string") {
			innerMsg = inner;
		}
	}

	const label = ERROR_CODE_LABELS[code] ?? "Unknown error";
	const message = innerMsg ?? label;

	return { code, message, retryable, provider, details };
}
