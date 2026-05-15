import type { HeddleConfig } from "../config/loader.ts";
import { createOpenRouterProvider } from "./openrouter.ts";
import type { Provider, ProviderConfig } from "./types.ts";

export interface Providers {
	main: Provider;
	weak?: Provider;
	editor?: Provider;
}

export function createProviders(config: HeddleConfig): Providers {
	if (!config.apiKey) {
		throw new Error("API key is required");
	}

	const base: Pick<ProviderConfig, "apiKey" | "baseUrl"> = {
		apiKey: config.apiKey,
		...(config.baseUrl ? { baseUrl: config.baseUrl } : {}),
	};

	const requestParams: Record<string, unknown> = {};
	if (config.maxTokens !== undefined) requestParams.max_tokens = config.maxTokens;
	if (config.temperature !== undefined) requestParams.temperature = config.temperature;
	const params = Object.keys(requestParams).length > 0 ? { requestParams } : {};

	const main = createOpenRouterProvider({ ...base, model: config.model, ...params });

	const weak = config.weakModel ? createOpenRouterProvider({ ...base, model: config.weakModel, ...params }) : undefined;

	const editor = config.editorModel
		? createOpenRouterProvider({ ...base, model: config.editorModel, ...params })
		: undefined;

	return { main, weak, editor };
}
