export {};

declare global {
  interface MockThreadMessage {
    id: string;
    role: string;
    content?: string;
    status?: string;
    timestamp?: number;
    output?: unknown;
    usage?: unknown;
    artifactBundle?: unknown;
    modelManifest?: unknown;
  }

  interface MockThread {
    id: string;
    title?: string;
    messages: MockThreadMessage[];
    [key: string]: unknown;
  }

  interface Window {
    __TAURI_INTERNALS__: {
      invoke: (cmd: string, args: Record<string, any>) => Promise<any>;
      metadata?: object;
    };
    __MOCK_HISTORY__: Array<Record<string, any>>;
    __MOCK_THREADS__: Record<string, MockThread>;
    __MOCK_LAST_DESIGN__: Record<string, any> | null;
    __MOCK_MODEL_MANIFESTS__: Record<string, any>;
    __MOCK_BUNDLES__: Record<string, any>;
  }
}
