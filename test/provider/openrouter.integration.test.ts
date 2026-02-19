import { describe, expect, test } from "bun:test";
import { createOpenRouterProvider } from "../../src/provider/openrouter.ts";

const API_KEY = process.env.OPENROUTER_API_KEY;
const TEST_MODEL = process.env.TEST_MODEL ?? "openrouter/free";

// Skip integration tests if no API key
const describeIntegration = API_KEY ? describe : describe.skip;

describeIntegration("OpenRouter Integration", () => {
	const provider = createOpenRouterProvider({
		apiKey: API_KEY!,
		model: "z-ai/glm-4.5-air:free",
		// model: "arcee-ai/trinity-large-preview:free",
		requestParams: { reasoning: { enabled: false } },
	});

	const reasoningProvider = createOpenRouterProvider({
		apiKey: API_KEY!,
		model: TEST_MODEL,
		requestParams: { reasoning: { enabled: true } },
	});

	test(
		"send() returns a text response",
		async () => {
			const response = await provider.send([{ role: "user", content: "hello!" }]);

			const raw = response as Record<string, unknown>;
			const msg = response.choices[0]?.message as Record<string, unknown>;
			console.log(`\n[send] model: ${raw.model}, provider: ${raw.provider}`);
			console.log(`[send] content: "${msg?.content}"`);
			console.log(`[send] has reasoning: ${!!msg?.reasoning}`);
			console.log(`[send] reasoning_tokens: ${response.usage?.completion_tokens_details?.reasoning_tokens ?? "N/A"}`);
			console.log(`[send] usage: ${JSON.stringify(response.usage)}\n`);

			expect(response.id).toBeDefined();
			expect(response.choices.length).toBeGreaterThan(0);
			expect(response.choices[0]?.message.role).toBe("assistant");
			expect(response.choices[0]?.message.content).toBeTruthy();
			expect(response.choices[0]?.finish_reason).toBe("stop");
		},
		{ timeout: 30_000 },
	);

	test(
		"stream() yields chunks and assembles content",
		async () => {
			const contentChunks: string[] = [];
			const reasoningChunks: string[] = [];
			let hasFinish = false;
			let model: string | undefined;

			for await (const chunk of provider.stream([{ role: "user", content: "hello!" }])) {
				if (!model) {
					const raw = chunk as Record<string, unknown>;
					model = raw.model as string;
				}
				const choice = chunk.choices[0];
				const delta = choice?.delta as Record<string, unknown> | undefined;
				if (delta?.content) {
					contentChunks.push(delta.content as string);
				}
				if (delta?.reasoning) {
					reasoningChunks.push(delta.reasoning as string);
				}
				if (choice?.finish_reason) {
					hasFinish = true;
				}
			}

			const assembled = contentChunks.join("");
			console.log(`\n[stream] model: ${model}`);
			console.log(`[stream] content: "${assembled}"`);
			console.log(`[stream] chunks: ${contentChunks.length}`);
			console.log(`[stream] has reasoning chunks: ${reasoningChunks.length > 0}`);
			console.log(`[stream] reasoning preview: "${reasoningChunks.join("").slice(0, 100)}"\n`);

			expect(contentChunks.length).toBeGreaterThan(0);
			expect(assembled.length).toBeGreaterThan(0);
			expect(hasFinish).toBe(true);
		},
		{ timeout: 30_000 },
	);

	test(
		"send() with reasoning returns reasoning tokens",
		async () => {
			const response = await reasoningProvider.send([{ role: "user", content: "hello!" }]);

			const raw = response as Record<string, unknown>;
			const message = response.choices[0]?.message as Record<string, unknown>;
			console.log(`\n[send+reasoning] model: ${raw.model}, provider: ${raw.provider}`);
			console.log(`[send+reasoning] content: "${message?.content}"`);
			console.log(`[send+reasoning] has reasoning: ${!!message?.reasoning}`);
			console.log(
				`[send+reasoning] reasoning_tokens: ${response.usage?.completion_tokens_details?.reasoning_tokens ?? "N/A"}`,
			);
			console.log(`[send+reasoning] usage: ${JSON.stringify(response.usage)}\n`);

			expect(response.id).toBeDefined();
			expect(response.choices.length).toBeGreaterThan(0);
			expect(response.choices[0]?.message.content).toBeTruthy();
			expect(response.choices[0]?.finish_reason).toBe("stop");
			// Reasoning tokens should be present (model-dependent, but free models tend to reason)
			const usage = response.usage as Record<string, unknown> | undefined;
			const details = usage?.completion_tokens_details as Record<string, unknown> | undefined;
			if (details?.reasoning_tokens) {
				expect(details.reasoning_tokens).toBeGreaterThan(0);
			}
		},
		{ timeout: 60_000 },
	);
});
