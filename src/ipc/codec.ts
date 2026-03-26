import type { ErrorEnvelope } from "./errors.ts";
import type { IpcRequest, IpcResponse, WorkerEvent } from "./types.ts";

export interface CorrelationContext {
	session_id?: string;
	task_id?: string;
	worker_id?: string;
}

function spreadDefined(obj: CorrelationContext): Partial<CorrelationContext> {
	const result: Partial<CorrelationContext> = {};
	if (obj.session_id !== undefined) result.session_id = obj.session_id;
	if (obj.task_id !== undefined) result.task_id = obj.task_id;
	if (obj.worker_id !== undefined) result.worker_id = obj.worker_id;
	return result;
}

export function encodeResponse(response: IpcResponse): string {
	return JSON.stringify(response);
}

export function decodeRequest(line: string): { ok: true; request: IpcRequest } | { ok: false; error: string } {
	let parsed: unknown;
	try {
		parsed = JSON.parse(line);
	} catch {
		return { ok: false, error: "Invalid JSON" };
	}
	if (typeof parsed !== "object" || parsed === null) {
		return { ok: false, error: "Expected JSON object" };
	}
	const obj = parsed as Record<string, unknown>;
	if (typeof obj.type !== "string") {
		return { ok: false, error: "Missing 'type' field" };
	}
	if (typeof obj.id !== "string") {
		return { ok: false, error: "Missing 'id' field" };
	}
	return { ok: true, request: obj as unknown as IpcRequest };
}

export function wrapEvent(event: WorkerEvent, sendId: string, eventSeq: number, ctx?: CorrelationContext): IpcResponse {
	return {
		type: "event",
		event,
		send_id: sendId,
		event_seq: eventSeq,
		...(ctx ? spreadDefined(ctx) : {}),
	} as IpcResponse;
}

export function buildResult(
	id: string,
	opts: {
		status: string;
		response?: string;
		toolCallsMade: { name: string; args: unknown }[];
		usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number };
		iterations: number;
		error?: ErrorEnvelope;
		correlation?: CorrelationContext;
		modelLatencyMs?: number;
		toolLatencyMs?: number;
		totalLatencyMs?: number;
	},
): IpcResponse {
	return {
		type: "result",
		id,
		status: opts.status,
		response: opts.response,
		tool_calls_made: opts.toolCallsMade,
		usage: opts.usage,
		iterations: opts.iterations,
		error: opts.error,
		...(opts.correlation ? spreadDefined(opts.correlation) : {}),
		...(opts.totalLatencyMs !== undefined ? { total_latency_ms: opts.totalLatencyMs } : {}),
		...(opts.modelLatencyMs !== undefined ? { model_latency_ms: opts.modelLatencyMs } : {}),
		...(opts.toolLatencyMs !== undefined ? { tool_latency_ms: opts.toolLatencyMs } : {}),
	} as IpcResponse;
}

export function buildError(
	id: string | undefined,
	error: ErrorEnvelope,
	correlation?: CorrelationContext,
): IpcResponse {
	return {
		type: "result",
		id: id ?? "unknown",
		status: "error",
		error,
		tool_calls_made: [],
		iterations: 0,
		...(correlation ? spreadDefined(correlation) : {}),
	} as IpcResponse;
}
