import * as readline from "node:readline";
import { runAgentLoopStreaming } from "../agent/loop.ts";
import { appendMessage } from "../session/jsonl.ts";
import type { SessionContext } from "../session/setup.ts";
import { createSession } from "../session/setup.ts";
import type { Message } from "../types.ts";

export async function startCli(): Promise<void> {
	let ctx: SessionContext;
	try {
		ctx = await createSession();
	} catch (err) {
		console.error(`Error: ${err instanceof Error ? err.message : String(err)}`);
		process.exit(1);
	}
	const { config, provider, registry, messages, sessionFile } = ctx;
	const loopOptions = {
		...(config.doomLoopThreshold !== undefined ? { doomLoopThreshold: config.doomLoopThreshold } : {}),
	};

	const rl = readline.createInterface({
		input: process.stdin,
		output: process.stdout,
	});

	console.log(`Heddle CLI — model: ${config.model}`);
	console.log(`Session: ${sessionFile}`);
	console.log('Type "exit" or "quit" to stop.\n');

	const prompt = (): void => {
		rl.question("you> ", async (input) => {
			const trimmed = input.trim();
			if (!trimmed) {
				prompt();
				return;
			}
			if (trimmed === "exit" || trimmed === "quit") {
				console.log("Goodbye!");
				rl.close();
				return;
			}

			const userMsg: Message = { role: "user", content: trimmed };
			messages.push(userMsg);
			await appendMessage(sessionFile, userMsg);

			try {
				let needsNewline = false;
				for await (const event of runAgentLoopStreaming(provider, registry, messages, loopOptions)) {
					switch (event.type) {
						case "content_delta": {
							if (!needsNewline) {
								process.stdout.write("\nassistant> ");
								needsNewline = true;
							}
							process.stdout.write(event.text);
							break;
						}
						case "assistant_message": {
							if (needsNewline) {
								process.stdout.write("\n\n");
								needsNewline = false;
							}
							await appendMessage(sessionFile, event.message);
							break;
						}
						case "tool_start": {
							console.log(`  [tool] ${event.name}(${event.call.function.arguments})`);
							break;
						}
						case "tool_end": {
							const preview = event.result.length > 200 ? `${event.result.slice(0, 200)}...` : event.result;
							console.log(`  [result] ${preview}`);
							await appendMessage(sessionFile, {
								role: "tool",
								tool_call_id: event.call.id,
								content: event.result,
							});
							break;
						}
						case "usage": {
							ctx.costTracker.addUsage(event.usage);
							const { totalInputTokens, totalOutputTokens, totalCost } = ctx.costTracker;
							const costStr = totalCost !== null ? ` | cost: $${totalCost.toFixed(4)}` : "";
							console.log(`  [tokens: ${totalInputTokens} in / ${totalOutputTokens} out${costStr}]`);
							break;
						}
						case "loop_detected": {
							console.error(
								`\n  [warning] Doom loop detected: ${event.count} identical tool call iterations. Stopping.`,
							);
							break;
						}
						case "error": {
							console.error(`  [error] ${event.error.message}`);
							break;
						}
					}
				}
			} catch (err) {
				console.error(`Error: ${err instanceof Error ? err.message : String(err)}`);
			}

			prompt();
		});
	};

	prompt();
}

// Auto-run when executed directly
startCli();
