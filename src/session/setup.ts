import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { loadAgentsContext } from "../config/agents-md.ts";
import type { HeddleConfig } from "../config/loader.ts";
import { loadConfig } from "../config/loader.ts";
import { ensureHeddleDirs, getProjectSessionsDir } from "../config/paths.ts";
import { createProviders } from "../provider/factory.ts";
import type { Provider } from "../provider/types.ts";
import { createBashTool } from "../tools/bash.ts";
import { createEditTool } from "../tools/edit.ts";
import { createGlobTool } from "../tools/glob.ts";
import { createGrepTool } from "../tools/grep.ts";
import { createReadTool } from "../tools/read.ts";
import { ToolRegistry } from "../tools/registry.ts";
import type { HeddleTool } from "../tools/types.ts";
import { createWriteTool } from "../tools/write.ts";
import type { Message } from "../types.ts";
import { appendMessage, writeSessionMeta } from "./jsonl.ts";

const DEFAULT_PROMPT =
	"You are a helpful coding assistant. You have access to file system tools to read, write, edit, and list files. Use them when the user asks you to work with files.";

export interface SessionContext {
	config: HeddleConfig;
	provider: Provider;
	weakProvider?: Provider;
	editorProvider?: Provider;
	registry: ToolRegistry;
	messages: Message[];
	sessionFile: string;
	sessionId: string;
}

export interface SessionOptions {
	model?: string;
	systemPrompt?: string;
	tools?: string[];
	cwd?: string;
}

function createDefaultTools(): HeddleTool[] {
	return [createReadTool(), createWriteTool(), createEditTool(), createGlobTool(), createGrepTool(), createBashTool()];
}

export async function createSession(options?: SessionOptions): Promise<SessionContext> {
	ensureHeddleDirs();

	if (options?.cwd) {
		if (!existsSync(options.cwd)) {
			throw new Error(`Directory does not exist: ${options.cwd}`);
		}
		process.chdir(options.cwd);
	}

	const config = loadConfig();

	if (!config.apiKey) {
		throw new Error("OPENROUTER_API_KEY environment variable or api_key in config.toml is required");
	}

	// Apply model override before creating providers
	const effectiveConfig = options?.model ? { ...config, model: options.model } : config;
	const providers = createProviders(effectiveConfig);
	const provider = providers.main;

	// Tool registration with filtering
	const registry = new ToolRegistry();
	const allTools = createDefaultTools();
	const toolFilter = (options?.tools?.length ? options.tools : null) ?? (config.tools?.length ? config.tools : null);
	const toolsToRegister = toolFilter ? allTools.filter((t) => toolFilter.includes(t.name)) : allTools;
	for (const tool of toolsToRegister) {
		registry.register(tool);
	}

	// Session file
	const sessionId = randomUUID();
	const sessionDir = getProjectSessionsDir();
	const sessionFile = join(sessionDir, `${sessionId}.jsonl`);

	await writeSessionMeta(sessionFile, {
		type: "session_meta",
		id: sessionId,
		cwd: process.cwd(),
		model: effectiveConfig.model,
		created: new Date().toISOString(),
		heddle_version: "0.1.0",
	});

	// System message
	const agentsContext = loadAgentsContext();
	const systemContent = [agentsContext, options?.systemPrompt ?? config.systemPrompt ?? DEFAULT_PROMPT]
		.filter(Boolean)
		.join("\n\n");

	const systemMessage: Message = { role: "system", content: systemContent };
	const messages: Message[] = [systemMessage];
	await appendMessage(sessionFile, systemMessage);

	return {
		config,
		provider,
		weakProvider: providers.weak,
		editorProvider: providers.editor,
		registry,
		messages,
		sessionFile,
		sessionId,
	};
}
