import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { loadAgentsContext } from "../config/agents-md.ts";
import { type DiscoveryResult, resolveDiscovery } from "../config/discovery.ts";
import type { FeatureFlags } from "../config/features.ts";
import { getFeatures } from "../config/features.ts";
import type { HeddleConfig } from "../config/loader.ts";
import { loadConfig } from "../config/loader.ts";
import { ensureHeddleDirs, getProjectMemoryDir, getProjectSessionsDir } from "../config/paths.ts";
import { ModelPricing } from "../cost/pricing.ts";
import { CostTracker } from "../cost/tracker.ts";
import { HooksRunner } from "../hooks/runner.ts";
import { loadMemoryContext } from "../memory/loader.ts";
import { PermissionChecker } from "../permissions/index.ts";
import { createProviders } from "../provider/factory.ts";
import type { Provider } from "../provider/types.ts";
import { createBashTool } from "../tools/bash.ts";
import { createEditTool } from "../tools/edit.ts";
import { createGlobTool } from "../tools/glob.ts";
import { createGrepTool } from "../tools/grep.ts";
import { createReadTool } from "../tools/read.ts";
import { ToolRegistry } from "../tools/registry.ts";
import { createSaveMemoryTool } from "../tools/save-memory.ts";
import type { HeddleTool } from "../tools/types.ts";
import { createWebFetchTool } from "../tools/web-fetch.ts";
import { createWriteTool } from "../tools/write.ts";
import type { Message } from "../types.ts";
import { forkSession } from "./fork.ts";
import { appendMessage, loadSession, writeSessionMeta } from "./jsonl.ts";
import { findSession } from "./list.ts";

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
	costTracker: CostTracker;
	modelPricing: ModelPricing;
	permissionChecker?: PermissionChecker;
	hooksRunner?: HooksRunner;
	features: FeatureFlags;
	discovery?: DiscoveryResult;
}

export interface SessionOptions {
	model?: string;
	systemPrompt?: string;
	tools?: string[];
	cwd?: string;
	resume?: string;
	fork?: string;
	sessionName?: string;
	permissionOverrides?: { allow?: string[]; deny?: string[]; ask?: string[] };
}

function createDefaultTools(): HeddleTool[] {
	return [
		createReadTool(),
		createWriteTool(),
		createEditTool(),
		createGlobTool(),
		createGrepTool(),
		createBashTool(),
		createWebFetchTool(),
	];
}

export async function createSession(options?: SessionOptions): Promise<SessionContext> {
	ensureHeddleDirs();
	import("../file-history/cleanup.ts").then((m) => m.runFileHistoryCleanup()).catch(() => {});

	if (options?.cwd) {
		if (!existsSync(options.cwd)) {
			throw new Error(`Directory does not exist: ${options.cwd}`);
		}
		process.chdir(options.cwd);
	}

	const config = loadConfig();
	const features = getFeatures("interactive", config.features);
	const discovery = resolveDiscovery();

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
	const toolFilter =
		(options?.tools !== undefined ? options.tools : null) ?? (config.tools !== undefined ? config.tools : null);
	const toolsToRegister = toolFilter ? allTools.filter((t) => toolFilter.includes(t.name)) : allTools;
	for (const tool of toolsToRegister) {
		registry.register(tool);
	}
	registry.register(createSaveMemoryTool(getProjectMemoryDir()));

	// Session file — resume, fork, or create new
	let sessionId: string;
	let sessionFile: string;
	let messages: Message[];

	if (options?.resume) {
		const found = await findSession(options.resume);
		if (!found) throw new Error(`Session not found: ${options.resume}`);
		sessionFile = found;
		const loaded = await loadSession(sessionFile);
		const firstLine = (await Bun.file(sessionFile).text()).split("\n")[0] ?? "";
		const meta = JSON.parse(firstLine);
		sessionId = meta.id;
		messages = loaded;
	} else if (options?.fork) {
		const source = await findSession(options.fork);
		if (!source) throw new Error(`Session not found: ${options.fork}`);
		const result = await forkSession(source);
		sessionFile = result.sessionFile;
		sessionId = result.sessionId;
		messages = await loadSession(sessionFile);
	} else {
		sessionId = randomUUID();
		const sessionDir = getProjectSessionsDir();
		sessionFile = join(sessionDir, `${sessionId}.jsonl`);

		await writeSessionMeta(sessionFile, {
			type: "session_meta",
			id: sessionId,
			cwd: process.cwd(),
			model: effectiveConfig.model,
			created: new Date().toISOString(),
			heddle_version: "0.1.0",
			...(options?.sessionName ? { name: options.sessionName } : {}),
		});

		// System message
		const agentsContext = loadAgentsContext();
		const memoryContext = loadMemoryContext();
		const systemContent = [agentsContext, memoryContext, options?.systemPrompt ?? config.systemPrompt ?? DEFAULT_PROMPT]
			.filter(Boolean)
			.join("\n\n");

		const systemMessage: Message = { role: "system", content: systemContent };
		messages = [systemMessage];
		await appendMessage(sessionFile, systemMessage);
	}

	const costTracker = new CostTracker();
	const modelPricing = new ModelPricing(effectiveConfig.apiKey ?? "", effectiveConfig.baseUrl);

	let permissionChecker: PermissionChecker | undefined;
	if (effectiveConfig.approvalMode) {
		const layers: Array<{ allow: string[]; deny: string[]; ask: string[] }> = [];

		// Config layers (global → local)
		if (effectiveConfig.permissionsLayers) {
			layers.push(...effectiveConfig.permissionsLayers);
		}

		// Headless overrides (most specific layer)
		if (options?.permissionOverrides) {
			layers.push({
				allow: options.permissionOverrides.allow ?? [],
				deny: options.permissionOverrides.deny ?? [],
				ask: options.permissionOverrides.ask ?? [],
			});
		}

		permissionChecker = new PermissionChecker(effectiveConfig.approvalMode, {
			layers: layers.length > 0 ? layers : undefined,
			projectDir: process.cwd(),
		});
	}

	let hooksRunner: HooksRunner | undefined;
	if (features.hooks && effectiveConfig.hooks && Object.keys(effectiveConfig.hooks).length > 0) {
		hooksRunner = new HooksRunner(effectiveConfig.hooks, "interactive", {
			sessionId,
			project: process.cwd(),
			model: effectiveConfig.model,
		});
	}

	return {
		config: effectiveConfig,
		provider,
		weakProvider: providers.weak,
		editorProvider: providers.editor,
		registry,
		messages,
		sessionFile,
		sessionId,
		costTracker,
		modelPricing,
		permissionChecker,
		hooksRunner,
		features,
		discovery,
	};
}
