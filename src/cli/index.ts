import * as readline from "node:readline";
import type { AgentLoopOptions } from "../agent/loop.ts";
import { runAgentLoopStreaming } from "../agent/loop.ts";
import { createBuiltinCommands } from "../commands/builtins.ts";
import { loadCustomCommands } from "../commands/loader.ts";
import { CommandRegistry } from "../commands/registry.ts";
import type { CommandContext } from "../commands/types.ts";
import { getProjectDir } from "../config/paths.ts";
import { pruneToolResults } from "../context/index.ts";
import { appendHistoryEntry } from "../history/writer.ts";
import { readOnlyToolFilter } from "../permissions/index.ts";
import { appendContextMarker, appendMessage } from "../session/jsonl.ts";
import type { SessionContext } from "../session/setup.ts";
import { createSession } from "../session/setup.ts";
import { createAskUserTool } from "../tools/ask-user.ts";
import type { Message, ToolCall } from "../types.ts";
import { writeUsageRecord } from "../usage/writer.ts";
import { createMentionCompleter } from "./completer.ts";
import { buildMentionMessage, resolveMentions } from "./mentions.ts";
import { formatOneshotOutput, runOneshot } from "./oneshot.ts";
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
	// Non-interactive mode: -p <prompt>
	const pIdx = process.argv.indexOf("-p");
	const promptIdx = process.argv.indexOf("--prompt");
	const oneshotIdx = pIdx !== -1 ? pIdx : promptIdx;
	if (oneshotIdx !== -1) {
		const prompt = process.argv[oneshotIdx + 1];
		if (!prompt) {
			console.error("Error: -p requires a prompt argument");
			process.exit(1);
		}
		const json = process.argv.includes("--json");
		const quiet = process.argv.includes("--quiet");
		const agentIdx = process.argv.indexOf("--agent");
		const agent = agentIdx !== -1 ? process.argv[agentIdx + 1] : undefined;
		const result = await runOneshot({ prompt, json, quiet, agent });
		console.log(formatOneshotOutput(result, { prompt, json, quiet }));
		process.exit(result.exitCode);
	}

	// Pipe mode: read stdin if not a TTY
	if (!process.stdin.isTTY && !process.argv.includes("--interactive")) {
		const chunks: string[] = [];
		for await (const chunk of process.stdin) {
			chunks.push(chunk.toString());
		}
		const prompt = chunks.join("").trim();
		if (prompt) {
			const result = await runOneshot({ prompt });
			console.log(formatOneshotOutput(result, { prompt }));
			process.exit(result.exitCode);
		}
	}

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
		completer: createMentionCompleter(process.cwd()),
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
		weakProvider: ctx.weakProvider,
		editorProvider: ctx.editorProvider,
		discovery: ctx.discovery,
		agentDefinitions: ctx.agentDefinitions,
		pasteCache: ctx.pasteCache,
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
		...(ctx.hooksRunner ? { hooksRunner: ctx.hooksRunner } : {}),
	};

	// Fire session_start hooks
	if (ctx.hooksRunner) {
		ctx.hooksRunner.run("session_start", {}).catch(() => {});
	}

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
				if (ctx.hooksRunner) {
					await ctx.hooksRunner.run("session_end", {}).catch(() => {});
				}
				// Write usage record on exit
				if (ctx.metricsCollector) {
					const { totalCost } = ctx.costTracker;
					writeUsageRecord(
						{
							session_id: ctx.sessionId,
							project: process.cwd(),
							created: new Date(ctx.sessionStartTime).toISOString(),
							ended: new Date().toISOString(),
							duration_ms: Date.now() - ctx.sessionStartTime,
							metrics: ctx.metricsCollector.metrics,
							...(totalCost !== null ? { cost_usd: totalCost } : {}),
						},
						getProjectDir(),
					).catch(() => {});
				}
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

			// Resolve @ mentions before sending to agent
			const mentions = await resolveMentions(trimmed, process.cwd());
			for (const f of mentions.injectedFiles) {
				console.log(`  [injected] ${f.path} (${f.lines} lines)`);
			}
			for (const err of mentions.errors) {
				console.log(`  [mention] ${err}`);
			}
			const content =
				mentions.injectedFiles.length > 0 ? buildMentionMessage(trimmed, mentions.injectedFiles) : trimmed;
			const userMsg: Message = { role: "user", content };
			messages.push(userMsg);
			await appendMessage(sessionFile, userMsg);

			// Track user message in metrics
			if (ctx.metricsCollector) {
				ctx.metricsCollector.onUserMessage();
			}

			if (ctx.features.history) {
				await appendHistoryEntry({
					timestamp: new Date().toISOString(),
					session_id: ctx.sessionId,
					project: process.cwd(),
					message_preview: trimmed.slice(0, 200),
					content_type: mentions.injectedFiles.length > 0 ? "mention" : "text",
				});
			}

			// Pre-prompt hooks
			if (ctx.hooksRunner) {
				const hookResults = await ctx.hooksRunner.run("pre_prompt", { userInput: trimmed });
				const blocked = hookResults.find((r) => r.blocked);
				if (blocked) {
					console.log(`\n  [hook blocked] ${blocked.reason ?? "hook rejected"}`);
					prompt();
					return;
				}
			}

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
							if (ctx.metricsCollector) ctx.metricsCollector.onAssistantMessage();
							break;
						}
						case "tool_start": {
							console.log(`  [tool] ${event.name}(${event.call.function.arguments})`);
							if (ctx.metricsCollector) ctx.metricsCollector.onToolCall(event.name);
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
							if (ctx.metricsCollector) ctx.metricsCollector.onUsage(event.usage);
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
							if (ctx.metricsCollector) ctx.metricsCollector.onError("provider");
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
				// Auto-compact if context is getting large
				if (ctx.weakProvider) {
					const { shouldCompact, compactContext } = await import("../context/compaction.ts");
					const modelLimit = config.maxTokens ?? 128000;
					if (shouldCompact(messages, modelLimit)) {
						const stats = await compactContext(messages, ctx.weakProvider, modelLimit);
						if (stats.messagesRemoved > 0) {
							await appendContextMarker(sessionFile, {
								type: "context_compaction",
								messages_removed: stats.messagesRemoved,
								tokens_before: stats.tokensBefore,
								tokens_after: stats.tokensAfter,
								timestamp: new Date().toISOString(),
							});
						}
					}
				}
				// Post-turn hooks
				if (ctx.hooksRunner) {
					await ctx.hooksRunner.run("post_turn", {}).catch(() => {});
				}
			} catch (err) {
				if (ctx.hooksRunner) {
					ctx.hooksRunner.run("error", {}).catch(() => {});
				}
				console.error(`Error: ${err instanceof Error ? err.message : String(err)}`);
			}

			prompt();
		});
	};

	prompt();
}

// Auto-run when executed directly
startCli();
