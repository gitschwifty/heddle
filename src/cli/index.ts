import { randomUUID } from "node:crypto";
import { join } from "node:path";
import * as readline from "node:readline";
import { runAgentLoop } from "../agent/loop.ts";
import { loadConfig } from "../config/loader.ts";
import { ensureHeddleDirs, getProjectSessionsDir } from "../config/paths.ts";
import { createOpenRouterProvider } from "../provider/openrouter.ts";
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

	const provider = createOpenRouterProvider({ apiKey, model: config.model });

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

	const messages: Message[] = [
		{
			role: "system",
			content:
				config.systemPrompt ??
				"You are a helpful coding assistant. You have access to file system tools to read, write, edit, and list files. Use them when the user asks you to work with files.",
		},
	];
	await appendMessage(sessionFile, messages[0]!);

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
				for await (const event of runAgentLoop(provider, registry, messages)) {
					switch (event.type) {
						case "assistant_message": {
							if (event.message.content) {
								console.log(`\nassistant> ${event.message.content}\n`);
							}
							messages.push(event.message);
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
							const toolMsg: Message = {
								role: "tool",
								tool_call_id: event.call.id,
								content: event.result,
							};
							messages.push(toolMsg);
							await appendMessage(sessionFile, toolMsg);
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
