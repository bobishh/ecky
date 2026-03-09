export type ParamValue = number | string | boolean | null;
export type DesignParams = Record<string, ParamValue>;

export interface UiField {
  key: string;
  label?: string;
  type?: 'number' | 'range' | 'boolean' | 'text' | string;
  min?: number;
  max?: number;
  step?: number;
  min_from?: string;
  max_from?: string;
  freezed?: boolean;
  [extra: string]: unknown;
}

export interface UiSpec {
  fields: UiField[];
  [extra: string]: unknown;
}

export interface DesignOutput {
  title: string;
  versionName: string;
  response: string;
  interactionMode: 'design' | 'question' | string;
  macroCode: string;
  uiSpec: UiSpec;
  initialParams: DesignParams;
}

export interface Message {
  id: string;
  role: 'user' | 'assistant' | string;
  content: string;
  status: 'pending' | 'success' | 'error' | 'discarded' | string;
  output?: DesignOutput;
  imageData?: string | null;
  timestamp: number;
}

export interface GenieTraits {
  [key: string]: unknown;
}

export interface Thread {
  id: string;
  title: string;
  summary: string;
  messages: Message[];
  updatedAt: number;
  versionCount: number;
  pendingCount: number;
  errorCount: number;
  genieTraits?: GenieTraits;
}

export interface ThreadReference {
  id: string;
  threadId: string;
  sourceMessageId?: string;
  ordinal: number;
  kind: string;
  name: string;
  content: string;
  summary: string;
  pinned: boolean;
  createdAt: number;
}

export interface Attachment {
  path: string;
  name: string;
  explanation: string;
  type: 'image' | 'cad' | string;
}

export interface EngineConfig {
  id: string;
  name: string;
  provider: 'gemini' | 'openai' | 'ollama' | string;
  apiKey: string;
  model: string;
  lightModel: string;
  baseUrl: string;
  systemPrompt: string;
}

export interface AssetConfig {
  id: string;
  name: string;
  path: string;
  format: string;
}

export interface MicrowaveConfig {
  humId: string | null;
  dingId: string | null;
  muted: boolean;
}

export interface AppConfig {
  engines: EngineConfig[];
  selectedEngineId: string;
  assets?: AssetConfig[];
  microwave?: MicrowaveConfig | null;
}

export interface GenerateOutput {
  design: DesignOutput;
  threadId: string;
  messageId: string;
}

export interface IntentDecision {
  intentMode: 'question' | 'design' | string;
  confidence: number;
  response: string;
}

export type RequestPhase =
  | 'classifying'
  | 'answering'
  | 'generating'
  | 'queued_for_render'
  | 'rendering'
  | 'committing'
  | 'repairing'
  | 'success'
  | 'error'
  | 'canceled';

export interface RequestResult {
  design: DesignOutput | null;
  threadId: string;
  messageId: string;
  stlUrl: string;
}

export interface Request {
  id: string;
  prompt: string;
  attachments: Attachment[];
  createdAt: number;
  phase: RequestPhase;
  attempt: number;
  maxAttempts: number;
  isQuestion: boolean;
  lightResponse: string;
  screenshot: string | null;
  threadId: string | null;
  result: RequestResult | null;
  error: string | null;
  cookingStartTime: number | null;
  cookingElapsed: number;
}
