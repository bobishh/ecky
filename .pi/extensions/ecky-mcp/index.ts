import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { Type } from "@sinclair/typebox";

type JsonRpcError = {
	code: number;
	message: string;
	data?: unknown;
};

type JsonRpcResponse = {
	jsonrpc: "2.0";
	id?: string | number | null;
	result?: unknown;
	error?: JsonRpcError;
};

type McpToolDefinition = {
	name: string;
	description?: string;
	inputSchema?: unknown;
};

const DEFAULT_ENDPOINT = "http://127.0.0.1:39249/mcp";
const SUPPORTED_PROTOCOL = "2025-06-18";

const GENERIC_MCP_ARGS_SCHEMA = Type.Object(
	{
		args: Type.Optional(
			Type.Object({}, { additionalProperties: true, description: "Arguments for the underlying Ecky MCP tool call." }),
		),
	},
	{
		description:
			"Wrapper call for Ecky MCP tools. Put the tool arguments inside `args` as a JSON object.",
	},
);

function endpointFromEnv(): string {
	const raw = process.env.ECKY_MCP_URL?.trim();
	return raw && raw.length > 0 ? raw : DEFAULT_ENDPOINT;
}

function sanitizeSuffix(name: string): string {
	const cleaned = name
		.trim()
		.toLowerCase()
		.replace(/[^a-z0-9_]+/g, "_")
		.replace(/^_+|_+$/g, "");
	return cleaned || "tool";
}

function stringifyUnknown(value: unknown): string {
	try {
		return JSON.stringify(value, null, 2);
	} catch {
		return String(value);
	}
}

function extractMcpText(result: unknown): string {
	if (!result || typeof result !== "object") return stringifyUnknown(result);
	const payload = result as Record<string, unknown>;
	const content = payload.content;
	if (Array.isArray(content)) {
		const textParts = content
			.map((block) => {
				if (!block || typeof block !== "object") return null;
				const obj = block as Record<string, unknown>;
				if (obj.type === "text" && typeof obj.text === "string") return obj.text;
				return null;
			})
			.filter((entry): entry is string => Boolean(entry));
		if (textParts.length > 0) return textParts.join("\n\n");
	}
	return stringifyUnknown(result);
}

function notify(ctx: ExtensionContext, message: string, level: "info" | "warning" | "error" = "info") {
	if (ctx.hasUI) {
		ctx.ui.notify(message, level);
	} else {
		if (level === "error") console.error(message);
		else console.log(message);
	}
}

class EckyMcpHttpClient {
	private endpoint: string;
	private sessionId: string | null = null;
	private nextId = 1;

	constructor(endpoint: string) {
		this.endpoint = endpoint;
	}

	setEndpoint(nextEndpoint: string) {
		this.endpoint = nextEndpoint;
		this.sessionId = null;
	}

	getEndpoint(): string {
		return this.endpoint;
	}

	isConnected(): boolean {
		return this.sessionId !== null;
	}

	private async requestRaw(
		body: Record<string, unknown>,
		expectJson = true,
	): Promise<{ json: JsonRpcResponse; sessionId: string | null }> {
		const headers: Record<string, string> = {
			"content-type": "application/json",
		};
		if (this.sessionId) headers["mcp-session-id"] = this.sessionId;

		const response = await fetch(this.endpoint, {
			method: "POST",
			headers,
			body: JSON.stringify(body),
		});

		const sessionId = response.headers.get("mcp-session-id");
		const text = await response.text();
		let parsed: JsonRpcResponse | null = null;
		if (text.trim().length > 0) {
			try {
				parsed = JSON.parse(text) as JsonRpcResponse;
			} catch {
				if (expectJson) {
					throw new Error(`MCP HTTP ${response.status}: ${text || "non-JSON response"}`);
				}
			}
		}

		if (!response.ok) {
			const err = parsed?.error?.message || text || `HTTP ${response.status}`;
			throw new Error(`MCP request failed: ${err}`);
		}

		if (!expectJson) {
			return {
				json: { jsonrpc: "2.0", result: {}, id: null },
				sessionId,
			};
		}

		if (!parsed) {
			throw new Error(`MCP HTTP ${response.status}: empty response`);
		}

		if (parsed.error) {
			throw new Error(`MCP error ${parsed.error.code}: ${parsed.error.message}`);
		}

		return { json: parsed, sessionId };
	}

	async initialize(): Promise<void> {
		const id = this.nextId++;
		const initBody = {
			jsonrpc: "2.0",
			id,
			method: "initialize",
			params: {
				protocolVersion: SUPPORTED_PROTOCOL,
				capabilities: {},
				clientInfo: {
					name: "pi-ecky-mcp-extension",
					version: "0.1.0",
				},
			},
		};

		const initResponse = await this.requestRaw(initBody);
		this.sessionId = initResponse.sessionId;
		if (!this.sessionId) {
			throw new Error("Ecky MCP did not return mcp-session-id header on initialize.");
		}

		await this.requestRaw(
			{
				jsonrpc: "2.0",
				method: "notifications/initialized",
				params: {},
			},
			false,
		);
	}

	private async request(method: string, params: Record<string, unknown> = {}, allowRetry = true): Promise<unknown> {
		if (!this.sessionId) await this.initialize();

		const id = this.nextId++;
		try {
			const response = await this.requestRaw({
				jsonrpc: "2.0",
				id,
				method,
				params,
			});
			if (response.sessionId) this.sessionId = response.sessionId;
			return response.json.result;
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			if (allowRetry && message.toLowerCase().includes("unknown mcp session")) {
				this.sessionId = null;
				return this.request(method, params, false);
			}
			throw error;
		}
	}

	async healthCheck(): Promise<unknown> {
		return this.request("tools/call", { name: "health_check", arguments: {} });
	}

	async listTools(): Promise<McpToolDefinition[]> {
		const result = await this.request("tools/list", {});
		if (!result || typeof result !== "object") return [];
		const tools = (result as Record<string, unknown>).tools;
		if (!Array.isArray(tools)) return [];
		return tools.filter((entry): entry is McpToolDefinition => {
			return Boolean(entry && typeof entry === "object" && typeof (entry as Record<string, unknown>).name === "string");
		});
	}

	async callTool(name: string, args: Record<string, unknown>): Promise<unknown> {
		return this.request("tools/call", {
			name,
			arguments: args,
		});
	}
}

export default function eckyMcpExtension(pi: ExtensionAPI) {
	const client = new EckyMcpHttpClient(endpointFromEnv());
	const registeredByRemoteName = new Map<string, string>();
	const usedLocalNames = new Set<string>();

	const registerRemoteTool = (tool: McpToolDefinition): string | null => {
		if (!tool.name) return null;
		const existing = registeredByRemoteName.get(tool.name);
		if (existing) return existing;

		let localName = `ecky_mcp_${sanitizeSuffix(tool.name)}`;
		if (usedLocalNames.has(localName)) {
			let i = 2;
			while (usedLocalNames.has(`${localName}_${i}`)) i += 1;
			localName = `${localName}_${i}`;
		}

		usedLocalNames.add(localName);
		registeredByRemoteName.set(tool.name, localName);

		pi.registerTool({
			name: localName,
			label: `Ecky MCP: ${tool.name}`,
			description: tool.description?.trim() || `Call Ecky MCP tool \`${tool.name}\`.`,
			promptSnippet: `Call Ecky MCP tool \`${tool.name}\` via Ecky MCP server`,
			promptGuidelines: [
				"Pass MCP arguments in the `args` object.",
				"If the call fails with 'app is not in mcp mode', switch Ecky Settings → Agents → Connection Type to MCP.",
			],
			parameters: GENERIC_MCP_ARGS_SCHEMA,
			async execute(_toolCallId, params, signal) {
				if (signal?.aborted) throw new Error("Aborted before MCP call.");
				const args = (params.args && typeof params.args === "object" ? (params.args as Record<string, unknown>) : {}) as Record<
					string,
					unknown
				>;
				const result = await client.callTool(tool.name, args);
				return {
					content: [{ type: "text", text: extractMcpText(result) }],
					details: {
						endpoint: client.getEndpoint(),
						remoteTool: tool.name,
						raw: result,
					},
				};
			},
		});

		return localName;
	};

	const discoverAndRegister = async (ctx: ExtensionContext) => {
		await client.initialize();
		const tools = await client.listTools();
		const before = registeredByRemoteName.size;
		for (const tool of tools) {
			registerRemoteTool(tool);
		}
		const added = registeredByRemoteName.size - before;
		notify(
			ctx,
			`Ecky MCP connected (${client.getEndpoint()}). ${tools.length} tools discovered, ${added} newly registered.`,
			"info",
		);
	};

	pi.on("session_start", async (_event, ctx) => {
		try {
			await discoverAndRegister(ctx);
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			notify(
				ctx,
				`Ecky MCP auto-connect failed (${client.getEndpoint()}): ${message}. Use /ecky-mcp-connect after Ecky is running.`,
				"warning",
			);
		}
	});

	pi.registerCommand("ecky-mcp-connect", {
		description: "Connect pi to Ecky MCP and register remote tools. Optional URL arg.",
		handler: async (args, ctx) => {
			const nextEndpoint = args?.trim();
			if (nextEndpoint) client.setEndpoint(nextEndpoint);
			await discoverAndRegister(ctx);
		},
	});

	pi.registerCommand("ecky-mcp-status", {
		description: "Show Ecky MCP endpoint/session status and discovered tools.",
		handler: async (_args, ctx) => {
			const names = Array.from(registeredByRemoteName.entries())
				.map(([remote, local]) => `${local} → ${remote}`)
				.sort();
			const summary = [
				`endpoint: ${client.getEndpoint()}`,
				`session: ${client.isConnected() ? "connected" : "not initialized"}`,
				`registered tools: ${names.length}`,
			];
			notify(ctx, summary.join(" | "), "info");
			if (names.length > 0) {
				notify(ctx, names.join("\n"), "info");
			}
		},
	});

	pi.registerCommand("ecky-mcp-health", {
		description: "Run Ecky MCP health_check tool and print result.",
		handler: async (_args, ctx) => {
			await client.initialize();
			const result = await client.healthCheck();
			notify(ctx, extractMcpText(result), "info");
		},
	});
}
