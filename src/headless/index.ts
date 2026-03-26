import readline from "node:readline";
import { runAgentLoopStreaming } from "../agent/loop.ts";
import type { AgentEvent } from "../agent/types.ts";
import { debug, setHeadless } from "../debug.ts";
import { buildError, buildResult, decodeRequest, encodeResponse, wrapEvent } from "../ipc/codec.ts";
import { normalizeError } from "../ipc/errors.ts";
import { checkCompatibility, PROTOCOL_VERSION } from "../ipc/protocol.ts";
import type { IpcRequest, IpcResponse, WorkerEvent } from "../ipc/types.ts";
import { appendMessage } from "../session/jsonl.ts";
import type { SessionContext } from "../session/setup.ts";
import { createSession } from "../session/setup.ts";
import { createAskUserTool } from "../tools/ask-user.ts";

setHeadless(true);

// ── State ──────────────────────────────────────────────────────────────
let session: SessionContext | null = null;
let activeId: string | null = null;
let cancelTargetId: string | null = null;
let activeAbortController: AbortController | null = null;
let stdinClosed = false;

const messageQueue: IpcRequest[] = [];
let processing = false;

// ── Core I/O ───────────────────────────────────────────────────────────
function writeLine(data: IpcResponse): void {
	process.stdout.write(`${encodeResponse(data)}\n`);
}

function checkExit(): void {
	if (!stdinClosed) return;
	if (processing || messageQueue.length > 0) {
		setTimeout(checkExit, 10);
		return;
	}
	process.exit(0);
}

// ── Queue ──────────────────────────────────────────────────────────────
async function processQueue(): Promise<void> {
	if (processing) return;
	processing = true;

	while (messageQueue.length > 0) {
		const request = messageQueue.shift();
		if (request) await handleRequest(request);
	}

	processing = false;
	checkExit();
}

function checkCancel(): boolean {
	for (let i = 0; i < messageQueue.length; i++) {
		const msg = messageQueue[i];
		if (!msg) continue;
		if (msg.type === "cancel" && "target_id" in msg && (msg as { target_id: string }).target_id === activeId) {
			cancelTargetId = activeId;
			messageQueue.splice(i, 1);
			return true;
		}
	}
	return cancelTargetId === activeId;
}

// ── Request handlers ───────────────────────────────────────────────────
async function handleRequest(request: IpcRequest): Promise<void> {
	switch (request.type) {
		case "init":
			await handleInit(request);
			break;
		case "send":
			await handleSend(request);
			break;
		case "status":
			handleStatus(request);
			break;
		case "shutdown":
			handleShutdown(request);
			break;
		case "cancel":
			handleCancel(request);
			break;
		default:
			writeLine(
				buildError((request as { id?: string }).id, {
					code: "protocol_error",
					message: `Unknown message type: ${(request as { type: string }).type}`,
					retryable: false,
				}),
			);
	}
}

async function handleInit(request: IpcRequest & { type: "init" }): Promise<void> {
	if (request.protocol_version) {
		const compat = checkCompatibility(request.protocol_version);
		if (!compat.compatible) {
			writeLine(
				buildResult(request.id, {
					status: "error",
					error: { code: "protocol_version_mismatch", message: "protocol_version_mismatch", retryable: false },
					toolCallsMade: [],
					iterations: 0,
				}),
			);
			process.exit(1);
		}
		if (compat.warn) {
			debug("headless", compat.warn);
		}
	}

	try {
		session = await createSession({
			model: request.config.model,
			systemPrompt: request.config.system_prompt,
			tools: request.config.tools,
		});

		session.registry.register(
			createAskUserTool(async () => {
				return "User interaction not available in headless mode";
			}),
		);

		writeLine({
			type: "init_ok",
			id: request.id,
			session_id: session.sessionId,
			protocol_version: PROTOCOL_VERSION,
		} as IpcResponse);
	} catch (err) {
		writeLine(
			buildError(request.id, {
				code: "protocol_error",
				message: err instanceof Error ? err.message : String(err),
				retryable: false,
			}),
		);
	}
}

async function handleSend(request: IpcRequest & { type: "send" }): Promise<void> {
	if (!session) {
		writeLine(
			buildError(request.id, {
				code: "protocol_error",
				message: "Not initialized. Send 'init' first.",
				retryable: false,
			}),
		);
		return;
	}

	if (activeId) {
		writeLine(
			buildError(request.id, {
				code: "protocol_error",
				message: "A send is already in progress.",
				retryable: false,
			}),
		);
		return;
	}

	activeId = request.id;
	cancelTargetId = null;
	let eventSeq = 0;

	const userMessage = { role: "user" as const, content: request.message };
	session.messages.push(userMessage);
	await appendMessage(session.sessionFile, userMessage);

	const toolCallsMade: { name: string; args: unknown }[] = [];
	let iterations = 0;
	let response: string | undefined;
	let totalUsage: { prompt_tokens: number; completion_tokens: number; total_tokens: number } | undefined;
	let sawContentDelta = false;
	let errorEnvelope: { code: string; message: string; retryable: boolean; details?: unknown } | undefined;

	// Heartbeat timer
	const heartbeatInterval = Number(process.env.HEDDLE_HEARTBEAT_INTERVAL) || 5000;
	const heartbeatTimer = setInterval(() => {
		writeLine(
			wrapEvent({ event: "heartbeat", timestamp: new Date().toISOString() } as WorkerEvent, request.id, eventSeq++),
		);
	}, heartbeatInterval);

	// AbortController for cancel
	const abortController = new AbortController();
	activeAbortController = abortController;

	try {
		const gen = runAgentLoopStreaming(session.provider, session.registry, session.messages, {
			...(session.permissionChecker ? { permissionChecker: session.permissionChecker } : {}),
			signal: abortController.signal,
		});

		for await (const event of gen) {
			if (checkCancel()) {
				abortController.abort();
				clearInterval(heartbeatTimer);
				writeLine(
					buildResult(request.id, {
						status: "error",
						error: { code: "cancelled", message: "cancelled", retryable: false },
						toolCallsMade,
						iterations,
					}),
				);
				activeAbortController = null;
				activeId = null;
				return;
			}

			const mapped = mapAgentEvent(event);
			if (mapped) {
				writeLine(wrapEvent(mapped, request.id, eventSeq++));
			}

			switch (event.type) {
				case "content_delta":
					sawContentDelta = true;
					break;
				case "tool_start": {
					let args: unknown = {};
					try {
						args = JSON.parse(event.call.function.arguments);
					} catch {}
					toolCallsMade.push({ name: event.name, args });
					break;
				}
				case "assistant_message":
					iterations++;
					if (!sawContentDelta && event.message.content) {
						response = event.message.content;
					}
					break;
				case "usage":
					totalUsage = {
						prompt_tokens: event.usage.prompt_tokens,
						completion_tokens: event.usage.completion_tokens,
						total_tokens: event.usage.total_tokens,
					};
					break;
				case "loop_detected":
					errorEnvelope = {
						code: "loop_detected",
						message: `Doom loop detected: ${event.count} iterations`,
						retryable: false,
					};
					break;
				case "error":
					errorEnvelope = {
						code: "provider_error",
						message: event.error.message,
						retryable: true,
					};
					break;
			}
		}
	} catch (err) {
		clearInterval(heartbeatTimer);
		const normalized = normalizeError(err, "provider_error");
		const { provider, ...envelope } = normalized;
		writeLine(
			wrapEvent(
				{
					event: "error",
					code: normalized.code,
					message: normalized.message,
					retryable: normalized.retryable,
					provider,
					details: normalized.details,
				},
				request.id,
				eventSeq++,
			),
		);
		writeLine(
			buildResult(request.id, {
				status: "error",
				error: envelope,
				toolCallsMade,
				usage: totalUsage,
				iterations,
			}),
		);
		activeAbortController = null;
		activeId = null;
		return;
	}

	clearInterval(heartbeatTimer);

	// Check if cancel occurred (may have been triggered while tool was executing)
	if (checkCancel() || abortController.signal.aborted) {
		writeLine(
			buildResult(request.id, {
				status: "error",
				error: { code: "cancelled", message: "cancelled", retryable: false },
				toolCallsMade,
				iterations,
			}),
		);
		activeAbortController = null;
		activeId = null;
		return;
	}

	if (errorEnvelope) {
		writeLine(
			buildResult(request.id, {
				status: "error",
				error: errorEnvelope,
				toolCallsMade,
				usage: totalUsage,
				iterations,
			}),
		);
	} else {
		if (sawContentDelta && !response) {
			const lastMsg = session.messages[session.messages.length - 1];
			if (lastMsg && "content" in lastMsg && typeof lastMsg.content === "string") {
				response = lastMsg.content;
			}
		}

		writeLine(
			buildResult(request.id, {
				status: "ok",
				response,
				toolCallsMade,
				usage: totalUsage,
				iterations,
			}),
		);
	}

	// Persist messages added by the agent loop
	for (const msg of session.messages.slice(session.messages.indexOf(userMessage) + 1)) {
		await appendMessage(session.sessionFile, msg);
	}

	activeAbortController = null;
	activeId = null;
}

function handleStatus(request: IpcRequest & { type: "status" }): void {
	if (!session) {
		writeLine(
			buildError(request.id, {
				code: "protocol_error",
				message: "Not initialized. Send 'init' first.",
				retryable: false,
			}),
		);
		return;
	}

	writeLine({
		type: "status_ok",
		id: request.id,
		model: session.config.model,
		messages_count: session.messages.length,
		session_id: session.sessionId,
		active: activeId !== null,
	} as IpcResponse);
}

function handleShutdown(request: IpcRequest & { type: "shutdown" }): void {
	writeLine({ type: "shutdown_ok", id: request.id } as IpcResponse);
	process.exit(0);
}

function handleCancel(request: IpcRequest & { type: "cancel" }): void {
	if ("target_id" in request && request.target_id === activeId) {
		cancelTargetId = activeId;
		activeAbortController?.abort();
	}
}

// ── Event mapping ──────────────────────────────────────────────────────
function mapAgentEvent(event: AgentEvent): WorkerEvent | null {
	switch (event.type) {
		case "content_delta":
			return { event: "content_delta", text: event.text };
		case "tool_start": {
			let args: unknown = {};
			try {
				args = JSON.parse(event.call.function.arguments);
			} catch {}
			return { event: "tool_start", name: event.name, args };
		}
		case "tool_end":
			return { event: "tool_end", name: event.name, result_preview: event.result.slice(0, 500) };
		case "usage":
			return {
				event: "usage",
				prompt_tokens: event.usage.prompt_tokens,
				completion_tokens: event.usage.completion_tokens,
				total_tokens: event.usage.total_tokens,
			};
		case "loop_detected":
			return {
				event: "error",
				code: "loop_detected",
				message: `Doom loop detected: ${event.count} iterations`,
				retryable: false,
			};
		case "error": {
			const normalized = normalizeError(event.error, "provider_error");
			return {
				event: "error",
				code: normalized.code,
				message: normalized.message,
				retryable: normalized.retryable,
				provider: normalized.provider,
				details: normalized.details,
			};
		}
		case "permission_denied":
			return {
				event: "permission_denied",
				name: event.name,
				reason: event.reason,
			};
		case "plan_complete":
			return { event: "plan_complete", plan: event.plan };
		case "assistant_message":
			return null;
		case "permission_request":
			return null;
		default:
			return null;
	}
}

// ── Stdin readline ─────────────────────────────────────────────────────
const rl = readline.createInterface({ input: process.stdin, terminal: false });

rl.on("line", (line: string) => {
	const decoded = decodeRequest(line);
	if (!decoded.ok) {
		writeLine(buildError(undefined, { code: "protocol_error", message: decoded.error, retryable: false }));
		return;
	}
	const request = decoded.request;
	// If a cancel arrives for the active send, abort immediately so blocked tools are interrupted
	if (
		request.type === "cancel" &&
		"target_id" in request &&
		(request as { target_id: string }).target_id === activeId
	) {
		cancelTargetId = activeId;
		activeAbortController?.abort();
	}
	messageQueue.push(request);
	processQueue();
});

rl.on("close", () => {
	stdinClosed = true;
	checkExit();
});

// Global error handlers
process.on("unhandledRejection", (err) => {
	const msg = err instanceof Error ? err.message : String(err);
	writeLine(buildError(undefined, { code: "protocol_error", message: msg, retryable: false }));
	process.exit(1);
});

process.on("uncaughtException", (err) => {
	writeLine(buildError(undefined, { code: "protocol_error", message: err.message, retryable: false }));
	process.exit(1);
});
