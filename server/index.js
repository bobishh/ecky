import express from 'express';
import dotenv from 'dotenv';
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { execa } from 'execa';
import { MODEL_SYSTEM_PROMPT, buildUserPrompt } from './prompt.js';

dotenv.config();

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, '..');
const OUTPUTS_DIR = path.join(ROOT, 'outputs');
const CONFIG_FILE = path.join(ROOT, 'config.json');
const TEMPLATE_MACRO = path.join(ROOT, 'templates', 'cache_pot_default.FCMacro');
const FREECAD_RUNNER = path.join(__dirname, 'freecad_runner.py');

// Initial default config
let activeConfig = {
  engines: [
    {
      id: 'default-gemini',
      name: 'Google Gemini 2.0',
      provider: 'gemini',
      apiKey: process.env.GEMINI_API_KEY || '',
      model: 'gemini-2.0-flash',
      baseUrl: '',
      systemPrompt: MODEL_SYSTEM_PROMPT
    },
    {
      id: 'default-openai',
      name: 'OpenAI GPT-4o',
      provider: 'openai',
      apiKey: process.env.OPENAI_API_KEY || '',
      model: 'gpt-4o',
      baseUrl: '',
      systemPrompt: MODEL_SYSTEM_PROMPT
    }
  ],
  selectedEngineId: 'default-gemini'
};

// Load config from file if exists
async function loadConfig() {
  try {
    const data = await fs.readFile(CONFIG_FILE, 'utf8');
    activeConfig = { ...activeConfig, ...JSON.parse(data) };
    console.log('Configuration loaded from disk.');
  } catch (e) {
    console.log('No config file found, using defaults.');
  }
}
loadConfig();

const app = express();
const port = Number(process.env.API_PORT || 8787);

app.use(express.json({ limit: '5mb' }));
app.use('/outputs', express.static(OUTPUTS_DIR));

app.get('/api/health', async (_req, res) => {
  res.json({ ok: true, port, outputsDir: OUTPUTS_DIR });
});

app.get('/api/config', (req, res) => {
  res.json(activeConfig);
});

app.post('/api/config', async (req, res) => {
  activeConfig = { ...activeConfig, ...req.body };
  await fs.writeFile(CONFIG_FILE, JSON.stringify(activeConfig, null, 2));
  res.json({ ok: true, config: activeConfig });
});

app.get('/api/default-macro', async (_req, res) => {
  try {
    const code = await fs.readFile(TEMPLATE_MACRO, 'utf8');
    res.json({ macroCode: code });
  } catch (error) {
    res.status(500).json({ error: String(error) });
  }
});

app.post('/api/generate', async (req, res) => {
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
  let uiSpec = null;
  let initialParams = {};
  let modelError = null;

  try {
    const modelOutput = await generateMacroWithModel(userPrompt);
    macroCode = modelOutput.macroCode;
    uiSpec = modelOutput.uiSpec;
    initialParams = modelOutput.initialParams;
  } catch (error) {
    modelError = String(error);
  }

  if (!looksLikeMacro(macroCode)) {
    macroCode = await fs.readFile(TEMPLATE_MACRO, 'utf8');
  }

  await fs.writeFile(macroPath, macroCode, 'utf8');

  let stlUrl = null;
  let renderError = null;

  try {
    await renderStlWithFreecad(macroPath, stlPath, initialParams);
    stlUrl = `/outputs/${id}.stl`;
  } catch (error) {
    renderError = String(error);
  }

  res.json({
    ok: true,
    macroCode,
    uiSpec,
    initialParams,
    stlUrl,
    modelError,
    renderError,
  });
});

app.post('/api/render', async (req, res) => {
  const { macroCode, parameters } = req.body;
  if (!macroCode) return res.status(400).json({ error: 'macroCode is required.' });

  const id = `render-${Date.now()}`;
  const macroPath = path.join(OUTPUTS_DIR, `${id}.FCMacro`);
  const stlPath = path.join(OUTPUTS_DIR, `${id}.stl`);

  try {
    await fs.writeFile(macroPath, macroCode, 'utf8');
    await renderStlWithFreecad(macroPath, stlPath, parameters || {});
    res.json({ ok: true, stlUrl: `/outputs/${id}.stl` });
  } catch (error) {
    res.status(500).json({ error: String(error) });
  }
});

app.listen(port, () => {
  console.log(`drydemacher API listening on http://localhost:${port}`);
});

async function generateMacroWithModel(userPrompt) {
  const engine = activeConfig.engines.find(e => e.id === activeConfig.selectedEngineId);
  if (!engine) throw new Error('No active engine selected.');

  const { provider, apiKey, model, systemPrompt, baseUrl } = engine;
  
  if (!apiKey && provider !== 'ollama') {
    throw new Error(`API Key for ${engine.name} is not set.`);
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
        response_format: { type: "json_object" }
      }),
    });

    if (!response.ok) {
      throw new Error(`${provider} error ${response.status}: ${await response.text()}`);
    }

    const json = await response.json();
    return JSON.parse(json.choices[0].message.content);
  } 
  
  if (provider === 'gemini') {
    const url = baseUrl || `https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent?key=${apiKey}`;
    const response = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        contents: [{
          role: "user",
          parts: [{ text: `${systemPrompt}\n\nUser request: ${userPrompt}\n\nReturn JSON only.` }]
        }],
        generationConfig: {
          responseMimeType: "application/json",
        }
      })
    });

    if (!response.ok) {
      throw new Error(`Gemini error ${response.status}: ${await response.text()}`);
    }

    const json = await response.json();
    const text = json.candidates[0].content.parts[0].text;
    return JSON.parse(text);
  }

  throw new Error(`Unsupported provider: ${provider}`);
}

function looksLikeMacro(code) {
  if (!code || code.length < 80) return false;
  return /import\s+FreeCAD|import\s+Part|App\./.test(code) && /Part\.|Shape|addObject/.test(code);
}

async function renderStlWithFreecad(macroPath, stlPath, params = {}) {
  const freecadCmd = process.env.FREECAD_CMD || 'FreeCADCmd';
  const timeout = Number(process.env.FREECAD_TIMEOUT_MS || 180000);

  // Pass parameters as a JSON string to the python runner
  const paramsJson = JSON.stringify(params);

  await execa(freecadCmd, [FREECAD_RUNNER, '--macro', macroPath, '--stl', stlPath, '--params', paramsJson], {
    cwd: ROOT,
    timeout,
  });

  const stat = await fs.stat(stlPath);
  if (!stat.isFile() || stat.size < 512) {
    throw new Error('STL file was not produced or is unexpectedly small.');
  }
}
