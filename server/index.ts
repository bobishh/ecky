import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import dotenv from 'dotenv';
import express from 'express';
import { execa } from 'execa';
import type { Request as ExpressRequest, Response as ExpressResponse } from 'express';

import { MODEL_SYSTEM_PROMPT, buildUserPrompt, type ServerModelOutput } from './prompt.js';
import type {
  AppConfig,
  DesignOutput,
  DesignParams,
  EngineConfig,
  UiSpec,
} from '../src/lib/types/domain.js';

dotenv.config();

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, '..');
const OUTPUTS_DIR = path.join(ROOT, 'outputs');
const CONFIG_FILE = path.join(ROOT, 'config.json');
const TEMPLATE_MACRO = path.join(ROOT, 'templates', 'cache_pot_default.FCMacro');
const CAD_SDK_SOURCE = path.join(ROOT, 'model-runtime', 'cad_sdk.py');
const FREECAD_RUNNER = path.join(__dirname, 'freecad_runner.py');

const app = express();
const port = Number(process.env.API_PORT || 8787);

function createDefaultConfig(): AppConfig {
  const engines: EngineConfig[] = [
    {
      id: 'default-gemini',
      name: 'Google Gemini 2.0',
      provider: 'gemini',
      apiKey: process.env.GEMINI_API_KEY || '',
      model: 'gemini-2.0-flash',
      lightModel: 'gemini-2.0-flash-lite',
      baseUrl: '',
      systemPrompt: MODEL_SYSTEM_PROMPT,
    },
    {
      id: 'default-openai',
      name: 'OpenAI GPT-4o',
      provider: 'openai',
      apiKey: process.env.OPENAI_API_KEY || '',
      model: 'gpt-4o',
      lightModel: 'gpt-4o-mini',
      baseUrl: '',
      systemPrompt: MODEL_SYSTEM_PROMPT,
    },
  ];

  return {
    engines,
    selectedEngineId: engines[0]?.id ?? '',
    freecadCmd: '',
    assets: [],
    microwave: null,
  };
}

let activeConfig: AppConfig = createDefaultConfig();

async function loadConfig(): Promise<void> {
  try {
    const data = await fs.readFile(CONFIG_FILE, 'utf8');
    activeConfig = {
      ...activeConfig,
      ...JSON.parse(data),
    } as AppConfig;
    console.log('Configuration loaded from disk.');
  } catch {
    console.log('No config file found, using defaults.');
  }
}

async function ensureCadSdk(outputDir: string): Promise<void> {
  const target = path.join(outputDir, 'cad_sdk.py');
  try {
    await fs.copyFile(CAD_SDK_SOURCE, target);
  } catch (error) {
    console.warn('Failed to copy cad_sdk.py into outputs:', String(error));
  }
}

function looksLikeMacro(code: string): boolean {
  if (!code || code.length < 80) return false;
  return /import\s+FreeCAD|import\s+Part|App\./.test(code) && /Part\.|Shape|addObject/.test(code);
}

async function renderStlWithFreecad(
  macroPath: string,
  stlPath: string,
  params: DesignParams = {},
): Promise<void> {
  const freecadCmd = process.env.FREECAD_CMD || 'FreeCADCmd';
  const timeout = Number(process.env.FREECAD_TIMEOUT_MS || 180000);
  const paramsJson = JSON.stringify(params);

  await execa(
    freecadCmd,
    [FREECAD_RUNNER, '--macro', macroPath, '--stl', stlPath, '--params', paramsJson],
    {
      cwd: ROOT,
      timeout,
    },
  );

  const stat = await fs.stat(stlPath);
  if (!stat.isFile() || stat.size < 512) {
    throw new Error('STL file was not produced or is unexpectedly small.');
  }
}

function activeEngine(): EngineConfig {
  const engine = activeConfig.engines.find((item) => item.id === activeConfig.selectedEngineId);
  if (!engine) {
    throw new Error('No active engine selected.');
  }
  return engine;
}

async function parseJsonResponse(response: globalThis.Response): Promise<ServerModelOutput> {
  const body = (await response.json()) as {
    choices?: Array<{ message?: { content?: string } }>;
    candidates?: Array<{ content?: { parts?: Array<{ text?: string }> } }>;
  };
  if (body.choices?.[0]?.message?.content) {
    return JSON.parse(body.choices[0].message.content) as ServerModelOutput;
  }
  const text = body.candidates?.[0]?.content?.parts?.[0]?.text;
  if (!text) {
    throw new Error('Model response did not contain JSON content.');
  }
  return JSON.parse(text) as ServerModelOutput;
}

async function generateMacroWithModel(userPrompt: string): Promise<ServerModelOutput> {
  const engine = activeEngine();
  const { provider, apiKey, model, systemPrompt, baseUrl } = engine;

  if (!apiKey && provider !== 'ollama') {
    throw new Error(`API key for ${engine.name} is not set.`);
  }

  if (provider === 'openai' || provider === 'ollama') {
    const url = baseUrl || 'https://api.openai.com/v1/chat/completions';
    const response = await fetch(url, {
      method: 'POST',
      headers: {
        ...(apiKey ? { Authorization: `Bearer ${apiKey}` } : {}),
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        model,
        messages: [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: buildUserPrompt(userPrompt) },
        ],
        response_format: { type: 'json_object' },
      }),
    });

    if (!response.ok) {
      throw new Error(`${provider} error ${response.status}: ${await response.text()}`);
    }

    return parseJsonResponse(response);
  }

  if (provider === 'gemini') {
    const url =
      baseUrl ||
      `https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent?key=${apiKey}`;
    const response = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contents: [
          {
            role: 'user',
            parts: [{ text: `${systemPrompt}\n\n${buildUserPrompt(userPrompt)}` }],
          },
        ],
        generationConfig: {
          responseMimeType: 'application/json',
        },
      }),
    });

    if (!response.ok) {
      throw new Error(`Gemini error ${response.status}: ${await response.text()}`);
    }

    return parseJsonResponse(response);
  }

  throw new Error(`Unsupported provider: ${provider}`);
}

loadConfig().catch((error) => {
  console.error('Failed to load config:', error);
});

app.use(express.json({ limit: '5mb' }));
app.use('/outputs', express.static(OUTPUTS_DIR));

app.get('/api/health', async (_req: ExpressRequest, res: ExpressResponse) => {
  res.json({ ok: true, port, outputsDir: OUTPUTS_DIR });
});

app.get('/api/config', (_req: ExpressRequest, res: ExpressResponse) => {
  res.json(activeConfig);
});

app.post('/api/config', async (req: ExpressRequest, res: ExpressResponse) => {
  activeConfig = { ...activeConfig, ...req.body } as AppConfig;
  await fs.writeFile(CONFIG_FILE, JSON.stringify(activeConfig, null, 2));
  res.json({ ok: true, config: activeConfig });
});

app.get('/api/default-macro', async (_req: ExpressRequest, res: ExpressResponse) => {
  try {
    const code = await fs.readFile(TEMPLATE_MACRO, 'utf8');
    res.json({ macroCode: code });
  } catch (error) {
    res.status(500).json({ error: String(error) });
  }
});

app.post('/api/generate', async (req: ExpressRequest, res: ExpressResponse) => {
  const userPrompt = String(req.body?.prompt || '').trim();
  if (!userPrompt) {
    res.status(400).json({ error: 'Prompt is required.' });
    return;
  }

  await fs.mkdir(OUTPUTS_DIR, { recursive: true });

  const id = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const macroPath = path.join(OUTPUTS_DIR, `${id}.FCMacro`);
  const stlPath = path.join(OUTPUTS_DIR, `${id}.stl`);

  let macroCode = '';
  let uiSpec: UiSpec = { fields: [] };
  let initialParams: DesignParams = {};
  let title = 'Untitled Design';
  let versionName = 'V1';
  let responseText = '';
  let interactionMode: DesignOutput['interactionMode'] = 'design';
  let modelError: string | null = null;

  try {
    const modelOutput = await generateMacroWithModel(userPrompt);
    macroCode = modelOutput.macroCode;
    uiSpec = modelOutput.uiSpec;
    initialParams = modelOutput.initialParams;
    title = modelOutput.title;
    versionName = modelOutput.versionName;
    responseText = modelOutput.response;
    interactionMode = modelOutput.interactionMode;
  } catch (error) {
    modelError = String(error);
  }

  if (!looksLikeMacro(macroCode)) {
    macroCode = await fs.readFile(TEMPLATE_MACRO, 'utf8');
  }

  await fs.writeFile(macroPath, macroCode, 'utf8');
  await ensureCadSdk(OUTPUTS_DIR);

  let stlUrl: string | null = null;
  let renderError: string | null = null;

  try {
    await renderStlWithFreecad(macroPath, stlPath, initialParams);
    stlUrl = `/outputs/${id}.stl`;
  } catch (error) {
    renderError = String(error);
  }

  res.json({
    ok: true,
    title,
    versionName,
    response: responseText,
    interactionMode,
    macroCode,
    uiSpec,
    initialParams,
    stlUrl,
    modelError,
    renderError,
  });
});

app.post('/api/render', async (req: ExpressRequest, res: ExpressResponse) => {
  const { macroCode, parameters } = req.body as {
    macroCode?: string;
    parameters?: DesignParams;
  };
  if (!macroCode) {
    res.status(400).json({ error: 'macroCode is required.' });
    return;
  }

  const id = `render-${Date.now()}`;
  const macroPath = path.join(OUTPUTS_DIR, `${id}.FCMacro`);
  const stlPath = path.join(OUTPUTS_DIR, `${id}.stl`);

  try {
    await fs.mkdir(OUTPUTS_DIR, { recursive: true });
    await fs.writeFile(macroPath, macroCode, 'utf8');
    await ensureCadSdk(OUTPUTS_DIR);
    await renderStlWithFreecad(macroPath, stlPath, parameters || {});
    res.json({ ok: true, stlUrl: `/outputs/${id}.stl` });
  } catch (error) {
    res.status(500).json({ error: String(error) });
  }
});

app.listen(port, () => {
  console.log(`Ecky CAD API listening on http://localhost:${port}`);
});
