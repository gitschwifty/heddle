import type { IpcRequest, IpcResponse, WorkerEvent } from "./types.ts";

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

export function wrapEvent(event: WorkerEvent): IpcResponse {
	return { type: "event", event } as IpcResponse;
}

export function buildResult(
	id: string,
	opts: {
		status: string;
		response?: string;
		toolCallsMade: { name: string; args: unknown }[];
		usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number };
		iterations: number;
		error?: string;
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
	} as IpcResponse;
}

export function buildError(id: string | undefined, error: string): IpcResponse {
	return {
		type: "result",
		id: id ?? "unknown",
		status: "error",
		error,
		tool_calls_made: [],
		iterations: 0,
	} as IpcResponse;
}
