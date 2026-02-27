import { debug } from "../debug.ts";
import type { PermissionChecker } from "../permissions/checker.ts";
import type { Provider, RequestOverrides } from "../provider/types.ts";
import type { ToolRegistry } from "../tools/registry.ts";
import type { AssistantMessage, Message, ToolCall, ToolDefinition, ToolMessage, Usage } from "../types.ts";
import type { AgentEvent } from "./types.ts";

export interface AgentLoopOptions {
	maxIterations?: number;
	doomLoopThreshold?: number;
	requestOverrides?: RequestOverrides;
	permissionChecker?: PermissionChecker;
	permissionResolver?: (name: string, call: ToolCall) => Promise<"allow" | "deny" | "always">;
	toolFilter?: (tools: ToolDefinition[]) => ToolDefinition[];
	planMode?: boolean;
}

const DEFAULT_MAX_ITERATIONS = 20;
const DEFAULT_DOOM_LOOP_THRESHOLD = 3;

/** Compute a hash string for an iteration's set of tool calls (for doom loop detection). */
function hashToolCalls(toolCalls: Array<{ name: string; arguments: string }>): string {
	return toolCalls
		.map((tc) => {
			let normalizedArgs: string;
			try {
				normalizedArgs = JSON.stringify(JSON.parse(tc.arguments));
			} catch {
				normalizedArgs = tc.arguments;
			}
			return `${tc.name}:${normalizedArgs}`;
		})
		.join("|");
}

/** Check if the recent hashes indicate a doom loop. */
function isDoomLoop(recentHashes: string[], threshold: number): boolean {
	if (recentHashes.length < threshold) return false;
	const last = recentHashes[recentHashes.length - 1];
	return recentHashes.slice(-threshold).every((h) => h === last);
}

/**
 * Check permission for a single tool call.
 * Returns events to yield and optionally a tool message (error result) to push.
 * If toolMessage is null, proceed to normal execute.
 */
async function checkPermission(
	checker: PermissionChecker,
	resolver: ((name: string, call: ToolCall) => Promise<"allow" | "deny" | "always">) | undefined,
	call: ToolCall,
): Promise<{ events: AgentEvent[]; toolMessage: ToolMessage | null }> {
	const toolName = call.function.name;
	let args: Record<string, unknown> | undefined;
	try {
		args = JSON.parse(call.function.arguments);
	} catch {
		// If args can't be parsed, proceed without them for permission check
	}

	const result = checker.check(toolName, args);

	if (result.decision === "allow") {
		return { events: [], toolMessage: null };
	}

	if (result.decision === "deny") {
		const reason = result.reason ?? "Permission denied";
		return {
			events: [{ type: "permission_denied", name: toolName, call, reason }],
			toolMessage: {
				role: "tool",
				tool_call_id: call.id,
				content: `Error: Permission denied — ${reason}`,
			},
		};
	}

	// decision === "ask"
	const reason = result.reason ?? `${toolName} requires approval`;

	if (!resolver) {
		// No resolver — default to deny
		return {
			events: [{ type: "permission_denied", name: toolName, call, reason }],
			toolMessage: {
				role: "tool",
				tool_call_id: call.id,
				content: `Error: Permission denied — ${reason}`,
			},
		};
	}

	const events: AgentEvent[] = [{ type: "permission_request", name: toolName, call, reason }];
	const response = await resolver(toolName, call);

	if (response === "always") {
		checker.allowAlways(toolName);
		return { events, toolMessage: null };
	}

	if (response === "allow") {
		return { events, toolMessage: null };
	}

	// response === "deny"
	events.push({ type: "permission_denied", name: toolName, call, reason });
	return {
		events,
		toolMessage: {
			role: "tool",
			tool_call_id: call.id,
			content: `Error: Permission denied — ${reason}`,
		},
	};
}

/**
 * Core agent loop: send messages → if tool_calls, execute tools → append results → repeat.
 * Mutates the passed-in messages array directly (appends assistant + tool messages).
 * Terminates when the assistant response has no tool_calls (text-only) or max iterations reached.
 */
export async function* runAgentLoop(
	provider: Provider,
	registry: ToolRegistry,
	messages: Message[],
	options?: AgentLoopOptions,
): AsyncGenerator<AgentEvent> {
	const maxIterations = options?.maxIterations ?? DEFAULT_MAX_ITERATIONS;
	const doomThreshold = options?.doomLoopThreshold ?? DEFAULT_DOOM_LOOP_THRESHOLD;
	let tools = registry.definitions();
	if (options?.toolFilter) {
		tools = options.toolFilter(tools);
	}
	const recentHashes: string[] = [];

	const overrides = options?.requestOverrides;
	const checker = options?.permissionChecker;
	const resolver = options?.permissionResolver;
	let lastAssistantContent: string | null = null;

	for (let iteration = 0; iteration < maxIterations; iteration++) {
		const response = await provider.send(messages, tools.length > 0 ? tools : undefined, overrides);

		if (response.usage) {
			yield { type: "usage", usage: response.usage };
		}

		const choice = response.choices[0];
		if (!choice) {
			yield { type: "error", error: new Error("No choice in response") };
			return;
		}

		const assistantMsg: AssistantMessage = {
			role: "assistant",
			content: choice.message.content,
			...(choice.message.tool_calls?.length ? { tool_calls: choice.message.tool_calls } : {}),
		};

		yield { type: "assistant_message", message: assistantMsg };
		messages.push(assistantMsg);
		lastAssistantContent = assistantMsg.content;

		const toolCalls = choice.message.tool_calls;
		if (!toolCalls?.length) {
			// No more tool calls — emit plan_complete if in plan mode
			if (options?.planMode && lastAssistantContent) {
				yield { type: "plan_complete", plan: lastAssistantContent };
			}
			return;
		}

		// Execute each tool call and collect results
		const toolMessages: ToolMessage[] = [];
		for (const call of toolCalls) {
			// Permission check
			if (checker) {
				const permResult = await checkPermission(checker, resolver, call);
				for (const event of permResult.events) {
					yield event;
				}
				if (permResult.toolMessage) {
					toolMessages.push(permResult.toolMessage);
					continue;
				}
			}

			yield { type: "tool_start", name: call.function.name, call };
			const result = await registry.execute(call.function.name, call.function.arguments);
			yield { type: "tool_end", name: call.function.name, result, call };
			toolMessages.push({
				role: "tool",
				tool_call_id: call.id,
				content: result,
			});
		}
		messages.push(...toolMessages);

		// Doom loop detection
		const hash = hashToolCalls(toolCalls.map((tc) => ({ name: tc.function.name, arguments: tc.function.arguments })));
		recentHashes.push(hash);
		if (recentHashes.length > doomThreshold) {
			recentHashes.shift();
		}
		if (isDoomLoop(recentHashes, doomThreshold)) {
			yield { type: "loop_detected", count: doomThreshold };
			return;
		}
	}

	yield {
		type: "error",
		error: new Error(`Max iterations (${maxIterations}) reached — possible infinite loop`),
	};
}

/**
 * Streaming agent loop: uses provider.stream() instead of provider.send().
 * Yields content_delta events as text arrives, assembles tool call deltas,
 * and executes tools the same as the non-streaming loop.
 */
export async function* runAgentLoopStreaming(
	provider: Provider,
	registry: ToolRegistry,
	messages: Message[],
	options?: AgentLoopOptions,
): AsyncGenerator<AgentEvent> {
	const maxIterations = options?.maxIterations ?? DEFAULT_MAX_ITERATIONS;
	const doomThreshold = options?.doomLoopThreshold ?? DEFAULT_DOOM_LOOP_THRESHOLD;
	let tools = registry.definitions();
	if (options?.toolFilter) {
		tools = options.toolFilter(tools);
	}
	const recentHashes: string[] = [];
	const overrides = options?.requestOverrides;
	const checker = options?.permissionChecker;
	const resolver = options?.permissionResolver;
	let lastAssistantContent: string | null = null;

	for (let iteration = 0; iteration < maxIterations; iteration++) {
		// Accumulate content and tool call deltas from the stream
		let contentParts = "";
		const assembledToolCalls: Map<number, { id: string; name: string; arguments: string }> = new Map();
		let streamUsage: Usage | undefined;

		for await (const chunk of provider.stream(messages, tools.length > 0 ? tools : undefined, overrides)) {
			const choice = chunk.choices[0];
			if (!choice) continue;

			const delta = choice.delta;

			// Yield text deltas as they arrive
			if (delta.content) {
				yield { type: "content_delta", text: delta.content };
				contentParts += delta.content;
			}

			// Assemble tool call deltas
			if (delta.tool_calls) {
				for (const tc of delta.tool_calls) {
					let entry = assembledToolCalls.get(tc.index);
					if (!entry) {
						entry = { id: "", name: "", arguments: "" };
						assembledToolCalls.set(tc.index, entry);
					}
					if (tc.id) entry.id = tc.id;
					if (tc.function?.name) entry.name += tc.function.name;
					if (tc.function?.arguments) entry.arguments += tc.function.arguments;
				}
			}

			if (chunk.usage) {
				streamUsage = chunk.usage;
				debug("agent", "usage", streamUsage);
			}
		}

		// Build assembled tool calls array (sorted by index)
		const toolCalls: ToolCall[] = [...assembledToolCalls.entries()]
			.sort(([a], [b]) => a - b)
			.map(([, tc]) => ({
				id: tc.id,
				type: "function" as const,
				function: { name: tc.name, arguments: tc.arguments },
			}));

		// Construct the assistant message
		const assistantMsg: AssistantMessage = {
			role: "assistant",
			content: contentParts || null,
			...(toolCalls.length ? { tool_calls: toolCalls } : {}),
		};

		yield { type: "assistant_message", message: assistantMsg };
		if (streamUsage) {
			yield { type: "usage", usage: streamUsage };
			streamUsage = undefined;
		}
		messages.push(assistantMsg);
		lastAssistantContent = assistantMsg.content;

		// No tool calls — done
		if (!toolCalls.length) {
			if (options?.planMode && lastAssistantContent) {
				yield { type: "plan_complete", plan: lastAssistantContent };
			}
			return;
		}

		// Execute each tool call
		const toolMessages: ToolMessage[] = [];
		for (const call of toolCalls) {
			// Permission check
			if (checker) {
				const permResult = await checkPermission(checker, resolver, call);
				for (const event of permResult.events) {
					yield event;
				}
				if (permResult.toolMessage) {
					toolMessages.push(permResult.toolMessage);
					continue;
				}
			}

			yield { type: "tool_start", name: call.function.name, call };
			const result = await registry.execute(call.function.name, call.function.arguments);
			yield { type: "tool_end", name: call.function.name, result, call };
			toolMessages.push({
				role: "tool",
				tool_call_id: call.id,
				content: result,
			});
		}
		messages.push(...toolMessages);

		// Doom loop detection
		const hash = hashToolCalls(toolCalls.map((tc) => ({ name: tc.function.name, arguments: tc.function.arguments })));
		recentHashes.push(hash);
		if (recentHashes.length > doomThreshold) {
			recentHashes.shift();
		}
		if (isDoomLoop(recentHashes, doomThreshold)) {
			yield { type: "loop_detected", count: doomThreshold };
			return;
		}
	}

	yield {
		type: "error",
		error: new Error(`Max iterations (${maxIterations}) reached — possible infinite loop`),
	};
}
