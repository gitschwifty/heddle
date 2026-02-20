export interface ModelPricingInfo {
	id: string;
	name: string;
	promptPrice: number;
	completionPrice: number;
	contextLength: number;
	maxCompletionTokens: number;
	modality: string;
	supportedParameters: string[];
}

interface ApiModelEntry {
	id: string;
	name: string;
	pricing: { prompt: string; completion: string };
	context_length: number;
	top_provider: { max_completion_tokens: number };
	architecture: { modality: string };
	supported_parameters: string[];
}

export class ModelPricing {
	private models: Map<string, ModelPricingInfo> | null = null;
	private fetchPromise: Promise<void> | null = null;
	private apiKey: string;
	private baseUrl: string;

	constructor(apiKey: string, baseUrl?: string) {
		this.apiKey = apiKey;
		this.baseUrl = baseUrl ?? "https://openrouter.ai/api/v1";
	}

	async getModel(modelId: string): Promise<ModelPricingInfo | undefined> {
		await this.ensureLoaded();
		return this.models?.get(modelId);
	}

	async getAllModels(): Promise<ModelPricingInfo[]> {
		await this.ensureLoaded();
		return Array.from(this.models?.values() ?? []);
	}

	async estimateCost(modelId: string, promptTokens: number, completionTokens: number): Promise<number | null> {
		const model = await this.getModel(modelId);
		if (!model) return null;
		return promptTokens * model.promptPrice + completionTokens * model.completionPrice;
	}

	get isLoaded(): boolean {
		return this.models !== null;
	}

	private async ensureLoaded(): Promise<void> {
		if (this.models) return;
		if (this.fetchPromise) {
			await this.fetchPromise;
			return;
		}
		this.fetchPromise = this.fetchModels();
		await this.fetchPromise;
	}

	private async fetchModels(): Promise<void> {
		const res = await fetch(`${this.baseUrl}/models`, {
			headers: { Authorization: `Bearer ${this.apiKey}` },
		});
		if (!res.ok) {
			throw new Error(`Models API returned ${res.status}`);
		}
		const json = (await res.json()) as { data?: ApiModelEntry[] };
		if (!Array.isArray(json.data)) {
			throw new Error(`Models API returned unexpected shape: ${JSON.stringify(json).slice(0, 200)}`);
		}
		const map = new Map<string, ModelPricingInfo>();
		for (const entry of json.data) {
			map.set(entry.id, {
				id: entry.id,
				name: entry.name,
				promptPrice: Number.parseFloat(entry.pricing.prompt),
				completionPrice: Number.parseFloat(entry.pricing.completion),
				contextLength: entry.context_length,
				maxCompletionTokens: entry.top_provider.max_completion_tokens,
				modality: entry.architecture.modality,
				supportedParameters: entry.supported_parameters,
			});
		}
		this.models = map;
	}
}
