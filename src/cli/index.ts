import { randomUUID } from "node:crypto";
import { join } from "node:path";
import * as readline from "node:readline";
import { runAgentLoopStreaming } from "../agent/loop.ts";
import { loadAgentsContext } from "../config/agents-md.ts";
import { loadConfig } from "../config/loader.ts";
import { ensureHeddleDirs, getProjectSessionsDir } from "../config/paths.ts";
import { createOpenRouterProvider } from "../provider/openrouter.ts";
import type { ProviderConfig } from "../provider/types.ts";
import { appendMessage, writeSessionMeta } from "../session/jsonl.ts";
import { createEditTool } from "../tools/edit.ts";
import { createGlobTool } from "../tools/glob.ts";
import { createReadTool } from "../tools/read.ts";
import { ToolRegistry } from "../tools/registry.ts";
import { createWriteTool } from "../tools/write.ts";
import type { Message } from "../types.ts";

export async function startCli(): Promise<void> {
	ensureHeddleDirs();
	const config = loadConfig();

	const apiKey = config.apiKey;
	if (!apiKey) {
		console.error("Error: OPENROUTER_API_KEY environment variable or api_key in config.toml is required");
		process.exit(1);
	}

	// Build provider config from expanded HeddleConfig
	const providerConfig: ProviderConfig = {
		apiKey,
		model: config.model,
		baseUrl: config.baseUrl,
	};

	// Build requestParams from config fields (only when defined)
	const requestParams: Record<string, unknown> = {};
	if (config.maxTokens !== undefined) requestParams.max_tokens = config.maxTokens;
	if (config.temperature !== undefined) requestParams.temperature = config.temperature;
	if (Object.keys(requestParams).length > 0) providerConfig.requestParams = requestParams;

	const provider = createOpenRouterProvider(providerConfig);

	const registry = new ToolRegistry();
	registry.register(createReadTool());
	registry.register(createWriteTool());
	registry.register(createEditTool());
	registry.register(createGlobTool());

	const sessionId = randomUUID();
	const sessionDir = getProjectSessionsDir();
	const sessionFile = join(sessionDir, `${sessionId}.jsonl`);

	await writeSessionMeta(sessionFile, {
		type: "session_meta",
		id: sessionId,
		cwd: process.cwd(),
		model: config.model,
		created: new Date().toISOString(),
		heddle_version: "0.1.0",
	});

	const agentsContext = loadAgentsContext();

	const systemContent = [
		agentsContext,
		config.systemPrompt ??
			"You are a helpful coding assistant. You have access to file system tools to read, write, edit, and list files. Use them when the user asks you to work with files.",
	]
		.filter(Boolean)
		.join("\n\n");

	const messages: Message[] = [
		{
			role: "system",
			content: systemContent,
		},
	];
	const systemMessage = messages[0];
	if (systemMessage) await appendMessage(sessionFile, systemMessage);

	const rl = readline.createInterface({
		input: process.stdin,
		output: process.stdout,
	});

	console.log(`Heddle CLI — model: ${config.model}`);
	console.log(`Session: ${sessionFile}`);
	if (agentsContext) {
		console.log("AGENTS.md: loaded project instructions");
	} else {
		console.log("AGENTS.md: none found (using default system prompt)");
	}
	console.log('Type "exit" or "quit" to stop.\n');

	// Agent loop options from config
	const loopOptions = {
		...(config.doomLoopThreshold !== undefined ? { doomLoopThreshold: config.doomLoopThreshold } : {}),
	};

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
						case "usage":
							break;
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
