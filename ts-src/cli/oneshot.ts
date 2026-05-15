import { runAgentLoop } from "../agent/loop.ts";
import type { Provider } from "../provider/types.ts";
import type { SessionOptions } from "../session/setup.ts";
import { createSession } from "../session/setup.ts";
import type { ToolRegistry } from "../tools/registry.ts";
import type { Message } from "../types.ts";

export interface OneshotOptions {
	prompt: string;
	json?: boolean;
	quiet?: boolean;
	agent?: string;
	sessionOptions?: Partial<SessionOptions>;
}

export interface OneshotWithContextOptions {
	prompt: string;
	provider: Provider;
	registry: ToolRegistry;
	messages: Message[];
	json?: boolean;
	quiet?: boolean;
}

export interface OneshotResult {
	output: string;
	exitCode: number;
	toolCalls: number;
}

/**
 * Run a single prompt through the agent loop using a pre-built context.
 * This is the testable core — no session setup, no filesystem side effects.
 */
export async function runOneshotWithContext(options: OneshotWithContextOptions): Promise<OneshotResult> {
	const { prompt, provider, registry, messages } = options;

	if (!prompt) {
		return { output: "No prompt provided", exitCode: 1, toolCalls: 0 };
	}

	// Add the user message
	messages.push({ role: "user", content: prompt });

	let output = "";
	let toolCalls = 0;

	try {
		const gen = runAgentLoop(provider, registry, messages);

		for await (const event of gen) {
			switch (event.type) {
				case "assistant_message": {
					// The last assistant_message with content is the final output
					output = event.message.content ?? "";
					break;
				}
				case "tool_start": {
					toolCalls++;
					break;
				}
				case "error": {
					return {
						output: event.error.message,
						exitCode: 1,
						toolCalls,
					};
				}
			}
		}

		return { output, exitCode: 0, toolCalls };
	} catch (err) {
		const message = err instanceof Error ? err.message : String(err);
		return { output: message, exitCode: 1, toolCalls };
	}
}

/**
 * Run a single prompt end-to-end: create session, execute, return result.
 * This is the CLI entry point — handles session setup and teardown.
 */
export async function runOneshot(options: OneshotOptions): Promise<OneshotResult> {
	if (!options.prompt) {
		return { output: "No prompt provided", exitCode: 1, toolCalls: 0 };
	}

	try {
		const session = await createSession({
			...options.sessionOptions,
			agent: options.agent,
		});

		return runOneshotWithContext({
			prompt: options.prompt,
			provider: session.provider,
			registry: session.registry,
			messages: session.messages,
			json: options.json,
			quiet: options.quiet,
		});
	} catch (err) {
		const message = err instanceof Error ? err.message : String(err);
		return { output: message, exitCode: 1, toolCalls: 0 };
	}
}

/**
 * Format the oneshot result for output to stdout.
 */
export function formatOneshotOutput(result: OneshotResult, options: OneshotOptions): string {
	if (options.json) {
		return JSON.stringify({
			output: result.output,
			exitCode: result.exitCode,
			toolCalls: result.toolCalls,
		});
	}

	return result.output;
}
