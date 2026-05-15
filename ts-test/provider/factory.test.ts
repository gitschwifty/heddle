import { afterAll, describe, expect, mock, test } from "bun:test";
import type { HeddleConfig } from "../../src/config/loader.ts";
import { createProviders } from "../../src/provider/factory.ts";
import { mockJsonResponse, mockTextResponse } from "../mocks/openrouter.ts";

describe("createProviders", () => {
	const originalFetch = globalThis.fetch;

	afterAll(() => {
		globalThis.fetch = originalFetch;
	});

	const baseConfig: HeddleConfig = {
		apiKey: "test-key",
		model: "main-model",
	};

	function setupFetchMock(): ReturnType<typeof mock> {
		const fetchMock = mock();
		fetchMock.mockImplementation(() => Promise.resolve(mockJsonResponse(mockTextResponse("ok"))));
		globalThis.fetch = fetchMock as unknown as typeof fetch;
		return fetchMock;
	}

	test("returns { main } with send/stream/with methods", () => {
		const providers = createProviders(baseConfig);
		expect(providers.main).toBeDefined();
		expect(typeof providers.main.send).toBe("function");
		expect(typeof providers.main.stream).toBe("function");
		expect(typeof providers.main.with).toBe("function");
	});

	test("weakModel set → returns { main, weak } with send/stream/with", () => {
		const providers = createProviders({ ...baseConfig, weakModel: "weak-model" });
		expect(providers.main).toBeDefined();
		expect(providers.weak).toBeDefined();
		expect(typeof providers.weak!.send).toBe("function");
		expect(typeof providers.weak!.stream).toBe("function");
		expect(typeof providers.weak!.with).toBe("function");
	});

	test("editorModel set → returns { main, editor }", () => {
		const providers = createProviders({ ...baseConfig, editorModel: "editor-model" });
		expect(providers.main).toBeDefined();
		expect(providers.editor).toBeDefined();
		expect(typeof providers.editor!.send).toBe("function");
		expect(typeof providers.editor!.stream).toBe("function");
		expect(typeof providers.editor!.with).toBe("function");
	});

	test("both weakModel and editorModel set → all three providers returned", () => {
		const providers = createProviders({
			...baseConfig,
			weakModel: "weak-model",
			editorModel: "editor-model",
		});
		expect(providers.main).toBeDefined();
		expect(providers.weak).toBeDefined();
		expect(providers.editor).toBeDefined();
	});

	test("no weakModel → weak is undefined", () => {
		const providers = createProviders(baseConfig);
		expect(providers.weak).toBeUndefined();
	});

	test("no editorModel → editor is undefined", () => {
		const providers = createProviders(baseConfig);
		expect(providers.editor).toBeUndefined();
	});

	test("missing apiKey → throws Error", () => {
		expect(() => createProviders({ model: "some-model" })).toThrow("API key is required");
	});

	test("all providers use same baseUrl", async () => {
		const fetchMock = setupFetchMock();

		const providers = createProviders({
			...baseConfig,
			baseUrl: "https://custom.api.com/v1",
			weakModel: "weak-model",
			editorModel: "editor-model",
		});

		await providers.main.send([{ role: "user", content: "hi" }]);
		await providers.weak!.send([{ role: "user", content: "hi" }]);
		await providers.editor!.send([{ role: "user", content: "hi" }]);

		expect(fetchMock).toHaveBeenCalledTimes(3);
		for (let i = 0; i < 3; i++) {
			const [url] = fetchMock.mock.calls[i] as [string, RequestInit];
			expect(url).toBe("https://custom.api.com/v1/chat/completions");
		}
	});

	test("all providers share requestParams (maxTokens/temperature)", async () => {
		const fetchMock = setupFetchMock();

		const providers = createProviders({
			...baseConfig,
			maxTokens: 1000,
			temperature: 0.5,
			weakModel: "weak-model",
			editorModel: "editor-model",
		});

		await providers.main.send([{ role: "user", content: "hi" }]);
		await providers.weak!.send([{ role: "user", content: "hi" }]);
		await providers.editor!.send([{ role: "user", content: "hi" }]);

		expect(fetchMock).toHaveBeenCalledTimes(3);
		for (let i = 0; i < 3; i++) {
			const [, opts] = fetchMock.mock.calls[i] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.max_tokens).toBe(1000);
			expect(body.temperature).toBe(0.5);
		}
	});

	test("each provider sends correct model string in request body", async () => {
		const fetchMock = setupFetchMock();

		const providers = createProviders({
			...baseConfig,
			model: "main-model",
			weakModel: "weak-model",
			editorModel: "editor-model",
		});

		await providers.main.send([{ role: "user", content: "hi" }]);
		await providers.weak!.send([{ role: "user", content: "hi" }]);
		await providers.editor!.send([{ role: "user", content: "hi" }]);

		expect(fetchMock).toHaveBeenCalledTimes(3);

		const [, mainOpts] = fetchMock.mock.calls[0] as [string, RequestInit];
		const [, weakOpts] = fetchMock.mock.calls[1] as [string, RequestInit];
		const [, editorOpts] = fetchMock.mock.calls[2] as [string, RequestInit];

		expect(JSON.parse(mainOpts.body as string).model).toBe("main-model");
		expect(JSON.parse(weakOpts.body as string).model).toBe("weak-model");
		expect(JSON.parse(editorOpts.body as string).model).toBe("editor-model");
	});
});
