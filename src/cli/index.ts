import * as readline from "node:readline";
import type { AgentLoopOptions } from "../agent/loop.ts";
import { runAgentLoopStreaming } from "../agent/loop.ts";
import { createBuiltinCommands } from "../commands/builtins.ts";
import { loadCustomCommands } from "../commands/loader.ts";
import { CommandRegistry } from "../commands/registry.ts";
import type { CommandContext } from "../commands/types.ts";
import { pruneToolResults } from "../context/index.ts";
import { readOnlyToolFilter } from "../permissions/index.ts";
import { appendContextMarker, appendMessage } from "../session/jsonl.ts";
import type { SessionContext } from "../session/setup.ts";
import { createSession } from "../session/setup.ts";
import { createAskUserTool } from "../tools/ask-user.ts";
import type { Message, ToolCall } from "../types.ts";
import { formatShellForContext, printShellResult, runShell } from "./shell.ts";

function buildPermissionResolver(rl: readline.Interface) {
	return (name: string, _call: ToolCall): Promise<"allow" | "deny" | "always"> => {
		return new Promise((resolve) => {
			rl.question(`  Allow ${name}? [y/n/always] `, (answer) => {
				const trimmed = answer.trim().toLowerCase();
				if (trimmed === "y" || trimmed === "yes") {
					resolve("allow");
				} else if (trimmed === "always" || trimmed === "a") {
					resolve("always");
				} else {
					resolve("deny");
				}
			});
		});
	};
}

export async function startCli(): Promise<void> {
	let ctx: SessionContext;
	try {
		ctx = await createSession();
	} catch (err) {
		console.error(`Error: ${err instanceof Error ? err.message : String(err)}`);
		process.exit(1);
	}
	const { config, registry, messages, sessionFile } = ctx;
	let activeProvider = ctx.provider;

	const setModel = (model: string) => {
		config.model = model;
		activeProvider = ctx.provider.with({ model });
	};

	const rl = readline.createInterface({
		input: process.stdin,
		output: process.stdout,
	});

	// Set up slash commands
	const commandRegistry = new CommandRegistry();
	const builtins = createBuiltinCommands(commandRegistry);
	for (const cmd of builtins) commandRegistry.register(cmd);
	const customCommands = await loadCustomCommands();
	for (const cmd of customCommands) commandRegistry.register(cmd);

	const commandCtx: CommandContext = {
		config,
		messages,
		registry,
		costTracker: ctx.costTracker,
		sessionFile,
		sessionId: ctx.sessionId,
		provider: activeProvider,
		rl,
		setModel,
	};

	ctx.registry.register(
		createAskUserTool(async (question, options) => {
			let display = `\n  [ask_user] ${question}`;
			if (options?.length) {
				display += `\n  Options: ${options.join(", ")}`;
			}
			console.log(display);
			return new Promise<string>((resolve) => {
				rl.question("  your answer> ", (answer) => resolve(answer.trim() || "(no response)"));
			});
		}),
	);

	const loopOptions: AgentLoopOptions = {
		...(config.doomLoopThreshold !== undefined ? { doomLoopThreshold: config.doomLoopThreshold } : {}),
		...(ctx.permissionChecker ? { permissionChecker: ctx.permissionChecker } : {}),
		...(ctx.permissionChecker ? { permissionResolver: buildPermissionResolver(rl) } : {}),
		...(config.approvalMode === "plan" ? { toolFilter: readOnlyToolFilter, planMode: true } : {}),
	};

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

			// !! prefix — run shell, print output AND inject into agent context
			if (trimmed.startsWith("!!")) {
				const cmd = trimmed.slice(2).trim();
				if (!cmd) {
					prompt();
					return;
				}
				const result = await runShell(cmd);
				printShellResult(result);
				const contextMsg = formatShellForContext(cmd, result);
				messages.push(contextMsg);
				await appendMessage(sessionFile, contextMsg);
				prompt();
				return;
			}
			// ! prefix — run shell, print output only (NOT added to context)
			if (trimmed.startsWith("!")) {
				const cmd = trimmed.slice(1).trim();
				if (!cmd) {
					prompt();
					return;
				}
				const result = await runShell(cmd);
				printShellResult(result);
				prompt();
				return;
			}
			// / prefix — slash command dispatch
			if (trimmed.startsWith("/")) {
				const spaceIdx = trimmed.indexOf(" ");
				const name = spaceIdx === -1 ? trimmed.slice(1) : trimmed.slice(1, spaceIdx);
				const args = spaceIdx === -1 ? "" : trimmed.slice(spaceIdx + 1);
				const cmd = commandRegistry.get(name);
				if (cmd) {
					await cmd.execute(args, commandCtx);
				} else {
					const suggestion = commandRegistry.suggest(name);
					console.log(
						suggestion
							? `Unknown command: /${name}. Did you mean /${suggestion}?`
							: `Unknown command: /${name}. Type /help for available commands.`,
					);
				}
				prompt();
				return;
			}

			const userMsg: Message = { role: "user", content: trimmed };
			messages.push(userMsg);
			await appendMessage(sessionFile, userMsg);

			try {
				let needsNewline = false;
				for await (const event of runAgentLoopStreaming(activeProvider, registry, messages, loopOptions)) {
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
						case "permission_request": {
							console.log(`  [permission] ${event.name} requires approval: ${event.reason ?? ""}`);
							break;
						}
						case "permission_denied": {
							console.log(`  [denied] ${event.name}: ${event.reason}`);
							break;
						}
						case "plan_complete": {
							console.log("\n  [plan complete]");
							console.log(event.plan);
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
				const pruneResult = pruneToolResults(messages);
				if (pruneResult.messagesPruned > 0) {
					await appendContextMarker(sessionFile, {
						type: "context_prune",
						messages_pruned: pruneResult.messagesPruned,
						tokens_before: pruneResult.tokensBefore,
						tokens_after: pruneResult.tokensAfter,
						timestamp: new Date().toISOString(),
					});
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
