import readline from "node:readline";
import { runAgentLoopStreaming } from "../agent/loop.ts";
import type { AgentEvent } from "../agent/types.ts";
import { debug, setHeadless } from "../debug.ts";
import { buildError, buildResult, decodeRequest, encodeResponse, wrapEvent } from "../ipc/codec.ts";
import { checkCompatibility, PROTOCOL_VERSION } from "../ipc/protocol.ts";
import type { IpcRequest, IpcResponse, WorkerEvent } from "../ipc/types.ts";
import { appendMessage } from "../session/jsonl.ts";
import type { SessionContext } from "../session/setup.ts";
import { createSession } from "../session/setup.ts";

setHeadless(true);

// ── State ──────────────────────────────────────────────────────────────
let session: SessionContext | null = null;
let activeId: string | null = null;
let cancelTargetId: string | null = null;
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
				buildError((request as { id?: string }).id, `Unknown message type: ${(request as { type: string }).type}`),
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
					error: "protocol_version_mismatch",
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

		writeLine({
			type: "init_ok",
			id: request.id,
			session_id: session.sessionId,
			protocol_version: PROTOCOL_VERSION,
		} as IpcResponse);
	} catch (err) {
		writeLine(buildError(request.id, err instanceof Error ? err.message : String(err)));
	}
}

async function handleSend(request: IpcRequest & { type: "send" }): Promise<void> {
	if (!session) {
		writeLine(buildError(request.id, "Not initialized. Send 'init' first."));
		return;
	}

	if (activeId) {
		writeLine(buildError(request.id, "A send is already in progress."));
		return;
	}

	activeId = request.id;
	cancelTargetId = null;

	const userMessage = { role: "user" as const, content: request.message };
	session.messages.push(userMessage);
	await appendMessage(session.sessionFile, userMessage);

	const toolCallsMade: { name: string; args: unknown }[] = [];
	let iterations = 0;
	let response: string | undefined;
	let totalUsage: { prompt_tokens: number; completion_tokens: number; total_tokens: number } | undefined;
	let sawContentDelta = false;
	let errorMsg: string | undefined;

	try {
		const gen = runAgentLoopStreaming(session.provider, session.registry, session.messages);

		for await (const event of gen) {
			if (checkCancel()) {
				writeLine(
					buildResult(request.id, {
						status: "error",
						error: "cancelled",
						toolCallsMade,
						iterations,
					}),
				);
				activeId = null;
				return;
			}

			const mapped = mapAgentEvent(event);
			if (mapped) {
				writeLine(wrapEvent(mapped));
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
					errorMsg = `Doom loop detected: ${event.count} iterations`;
					break;
				case "error":
					errorMsg = event.error.message;
					break;
			}
		}
	} catch (err) {
		const msg = err instanceof Error ? err.message : String(err);
		writeLine(wrapEvent({ event: "error", error: msg, code: "provider_error" }));
		writeLine(
			buildResult(request.id, {
				status: "error",
				error: msg,
				toolCallsMade,
				usage: totalUsage,
				iterations,
			}),
		);
		activeId = null;
		return;
	}

	if (errorMsg) {
		writeLine(
			buildResult(request.id, {
				status: "error",
				error: errorMsg,
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

	activeId = null;
}

function handleStatus(request: IpcRequest & { type: "status" }): void {
	if (!session) {
		writeLine(buildError(request.id, "Not initialized. Send 'init' first."));
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
			return { event: "error", error: `Doom loop detected: ${event.count} iterations`, code: "loop_detected" };
		case "error":
			return { event: "error", error: event.error.message };
		case "assistant_message":
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
		writeLine(buildError(undefined, decoded.error));
		return;
	}
	messageQueue.push(decoded.request);
	processQueue();
});

rl.on("close", () => {
	stdinClosed = true;
	checkExit();
});

// Global error handlers
process.on("unhandledRejection", (err) => {
	const msg = err instanceof Error ? err.message : String(err);
	writeLine(buildError(undefined, msg));
	process.exit(1);
});

process.on("uncaughtException", (err) => {
	writeLine(buildError(undefined, err.message));
	process.exit(1);
});
