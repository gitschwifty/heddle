import { readOnlyToolFilter } from "../permissions/checker.ts";
import type { Provider } from "../provider/types.ts";
import type { ToolRegistry } from "../tools/registry.ts";
import type { Message } from "../types.ts";
import type { AgentLoopOptions } from "./loop.ts";
import { runAgentLoop } from "./loop.ts";
import type { AgentEvent } from "./types.ts";

export interface ArchitectOptions {
	onPlanReady?: (plan: string) => Promise<boolean>;
}

/**
 * Two-phase architect/editor pipeline.
 *
 * Phase 1 (architect): Runs with planMode and read-only tools to produce a plan.
 * Phase 2 (editor): Runs with the full tool set to execute the plan.
 *
 * Yields all events from both phases.
 */
export async function* runArchitectPipeline(
	architectProvider: Provider,
	editorProvider: Provider,
	registry: ToolRegistry,
	messages: Message[],
	options?: AgentLoopOptions & ArchitectOptions,
): AsyncGenerator<AgentEvent> {
	// Phase 1: Architect — plan mode with read-only tools
	const architectMessages: Message[] = [...messages];
	let plan: string | undefined;

	try {
		const architectGen = runAgentLoop(architectProvider, registry, architectMessages, {
			...options,
			planMode: true,
			toolFilter: readOnlyToolFilter,
		});

		for await (const event of architectGen) {
			yield event;
			if (event.type === "plan_complete") {
				plan = event.plan;
			}
		}
	} catch (err) {
		yield {
			type: "error",
			error: err instanceof Error ? err : new Error(String(err)),
		};
		return;
	}

	if (!plan) {
		yield {
			type: "error",
			error: new Error("Architect phase produced no plan"),
		};
		return;
	}

	// Check with onPlanReady callback
	if (options?.onPlanReady) {
		const approved = await options.onPlanReady(plan);
		if (!approved) {
			yield {
				type: "error",
				error: new Error("Plan was rejected"),
			};
			return;
		}
	}

	// Phase 2: Editor — full tool access, original messages + plan
	const editorMessages: Message[] = [
		...messages,
		{
			role: "user",
			content: `Execute the following plan:\n\n${plan}`,
		},
	];

	const editorGen = runAgentLoop(editorProvider, registry, editorMessages, {
		...options,
		planMode: false,
		toolFilter: undefined,
	});

	for await (const event of editorGen) {
		yield event;
	}
}
