import { Type } from "@sinclair/typebox";
import { runAgentLoop } from "../agent/loop.ts";
import type { AgentEvent } from "../agent/types.ts";
import type { CostTracker } from "../cost/tracker.ts";
import type { HooksRunner } from "../hooks/runner.ts";
import type { PermissionChecker } from "../permissions/checker.ts";
import type { Provider } from "../provider/types.ts";
import type { Message } from "../types.ts";
import type { ToolRegistry } from "./registry.ts";
import type { HeddleTool } from "./types.ts";

export interface SubagentOptions {
	permissionChecker?: PermissionChecker;
	costTracker?: CostTracker;
	hooksRunner?: HooksRunner;
	maxIterations?: number;
}

export function createSubagentTool(provider: Provider, registry: ToolRegistry, options: SubagentOptions): HeddleTool {
	return {
		name: "subagent",
		description: "Spawn a child agent with isolated context to perform a subtask. Returns the agent's final response.",
		parameters: Type.Object({
			prompt: Type.String({ description: "The task for the subagent" }),
			tools: Type.Optional(
				Type.Array(Type.String(), {
					description: "Filter to only these tools from the registry",
				}),
			),
		}),
		execute: async (params, execOptions) => {
			const { prompt, tools } = params as {
				prompt: string;
				tools?: string[];
			};

			const effectiveRegistry = tools ? registry.subset(tools) : registry;

			const messages: Message[] = [
				{
					role: "system",
					content: "You are a subagent. Complete the given task using available tools. Be concise and focused.",
				},
				{ role: "user", content: prompt },
			];

			try {
				const events: AgentEvent[] = [];
				const gen = runAgentLoop(provider, effectiveRegistry, messages, {
					maxIterations: options.maxIterations,
					permissionChecker: options.permissionChecker,
					hooksRunner: options.hooksRunner,
					signal: execOptions?.signal,
				});

				for await (const event of gen) {
					events.push(event);

					// Accumulate usage into parent cost tracker
					if (event.type === "usage" && options.costTracker) {
						options.costTracker.addUsage(event.usage);
					}
				}

				// Extract the last assistant message content as the result
				const lastAssistant = events.filter((e) => e.type === "assistant_message").pop();

				if (lastAssistant?.type === "assistant_message" && lastAssistant.message.content) {
					return lastAssistant.message.content;
				}

				// Check for errors
				const errorEvent = events.find((e) => e.type === "error");
				if (errorEvent?.type === "error") {
					return `Error: Subagent failed — ${errorEvent.error.message}`;
				}

				return "Error: Subagent produced no response";
			} catch (err) {
				const message = err instanceof Error ? err.message : String(err);
				return `Error: Subagent failed — ${message}`;
			}
		},
	};
}
