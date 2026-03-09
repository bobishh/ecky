const BYTES_IN_MB = 1024 * 1024;
const PROFILER_FLAG_KEY = 'ecky_cad_profiler';

type HeapStats = {
  usedMb: number;
  totalMb: number;
  limitMb: number;
} | null;

function roundMb(bytes: number): number {
  return Number((bytes / BYTES_IN_MB).toFixed(2));
}

function getHeapStats(): HeapStats {
  try {
    const perf = performance as Performance & {
      memory?: {
        usedJSHeapSize: number;
        totalJSHeapSize: number;
        jsHeapSizeLimit: number;
      };
    };
    if (!perf.memory) return null;
    return {
      usedMb: roundMb(perf.memory.usedJSHeapSize),
      totalMb: roundMb(perf.memory.totalJSHeapSize),
      limitMb: roundMb(perf.memory.jsHeapSizeLimit),
    };
  } catch {
    return null;
  }
}

export function isProfilerEnabled(): boolean {
  if (!import.meta.env.DEV) return false;
  try {
    return localStorage.getItem(PROFILER_FLAG_KEY) === '1';
  } catch {
    return false;
  }
}

export function estimateBase64Bytes(dataUrl?: string | null): number {
  if (!dataUrl) return 0;
  const commaIndex = dataUrl.indexOf(',');
  const base64Part = commaIndex >= 0 ? dataUrl.slice(commaIndex + 1) : dataUrl;
  if (!base64Part) return 0;
  // Approximate decoded byte size for base64 payload.
  return Math.floor((base64Part.length * 3) / 4);
}

export function profileLog(event: string, details: Record<string, unknown> = {}): void {
  if (!isProfilerEnabled()) return;
  const heap = getHeapStats();
  const ts = new Date().toISOString();
  console.log(`[PROF] ${event}`, { ts, ...details, heap });
}
