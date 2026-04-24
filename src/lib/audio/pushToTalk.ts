export interface PromptAudioCapture {
  base64Data: string;
  mimeType: string;
}

export interface PromptAudioRecorder {
  start(): Promise<void>;
  stop(): Promise<PromptAudioCapture>;
  cancel(): void;
}

type PushToTalkWindow = Window &
  typeof globalThis & {
    __ECKY_TEST_AUDIO_RECORDER__?: PromptAudioRecorder;
    webkitAudioContext?: typeof AudioContext;
  };

function pushToTalkWindow(): PushToTalkWindow | null {
  if (typeof window === 'undefined') return null;
  return window as PushToTalkWindow;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}

function mergeFloatChunks(chunks: Float32Array[]): Float32Array {
  const length = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const merged = new Float32Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  return merged;
}

function writeAscii(view: DataView, offset: number, value: string) {
  for (let i = 0; i < value.length; i += 1) {
    view.setUint8(offset + i, value.charCodeAt(i));
  }
}

export function encodePcm16Wav(samples: Float32Array, sampleRate: number): Uint8Array {
  const bytesPerSample = 2;
  const headerBytes = 44;
  const dataBytes = samples.length * bytesPerSample;
  const buffer = new ArrayBuffer(headerBytes + dataBytes);
  const view = new DataView(buffer);

  writeAscii(view, 0, 'RIFF');
  view.setUint32(4, 36 + dataBytes, true);
  writeAscii(view, 8, 'WAVE');
  writeAscii(view, 12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * bytesPerSample, true);
  view.setUint16(32, bytesPerSample, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, 'data');
  view.setUint32(40, dataBytes, true);

  let offset = headerBytes;
  for (const sample of samples) {
    const clamped = Math.max(-1, Math.min(1, sample));
    const scaled = clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff;
    view.setInt16(offset, Math.trunc(scaled), true);
    offset += bytesPerSample;
  }

  return new Uint8Array(buffer);
}

export function appendTranscriptToPrompt(currentPrompt: string, transcript: string): string {
  const cleanTranscript = transcript.trim();
  if (!cleanTranscript) return currentPrompt;
  if (!currentPrompt.trim()) return cleanTranscript;
  if (/\s$/.test(currentPrompt)) return `${currentPrompt}${cleanTranscript}`;
  return `${currentPrompt} ${cleanTranscript}`;
}

class BrowserPcmWavRecorder implements PromptAudioRecorder {
  private chunks: Float32Array[] = [];
  private context: AudioContext | null = null;
  private processor: ScriptProcessorNode | null = null;
  private source: MediaStreamAudioSourceNode | null = null;
  private stream: MediaStream | null = null;
  private sampleRate = 16_000;
  private started = false;

  async start(): Promise<void> {
    const win = pushToTalkWindow();
    const AudioContextCtor = win?.AudioContext ?? win?.webkitAudioContext;
    if (!AudioContextCtor) {
      throw new Error('Microphone recording failed: AudioContext is unavailable.');
    }
    if (!navigator.mediaDevices?.getUserMedia) {
      throw new Error('Microphone recording failed: mediaDevices.getUserMedia is unavailable.');
    }

    this.cancel();
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
      },
    });
    this.context = new AudioContextCtor();
    this.sampleRate = this.context.sampleRate;
    if (this.context.state === 'suspended') {
      await this.context.resume();
    }
    this.source = this.context.createMediaStreamSource(this.stream);
    this.processor = this.context.createScriptProcessor(4096, 1, 1);
    this.processor.onaudioprocess = (event) => {
      const input = event.inputBuffer.getChannelData(0);
      this.chunks.push(new Float32Array(input));
    };
    this.source.connect(this.processor);
    this.processor.connect(this.context.destination);
    this.started = true;
  }

  async stop(): Promise<PromptAudioCapture> {
    if (!this.started) {
      throw new Error('Microphone recording failed: no active recording.');
    }
    const samples = mergeFloatChunks(this.chunks);
    const sampleRate = this.sampleRate;
    await this.cleanup();
    if (!samples.length) {
      throw new Error('Microphone recording failed: no audio captured.');
    }
    return {
      base64Data: bytesToBase64(encodePcm16Wav(samples, sampleRate)),
      mimeType: 'audio/wav',
    };
  }

  cancel(): void {
    void this.cleanup();
  }

  private async cleanup() {
    this.started = false;
    try {
      this.processor?.disconnect();
    } catch {
      // Browser node may already be disconnected.
    }
    try {
      this.source?.disconnect();
    } catch {
      // Browser node may already be disconnected.
    }
    for (const track of this.stream?.getTracks() ?? []) {
      track.stop();
    }
    const context = this.context;
    this.context = null;
    this.processor = null;
    this.source = null;
    this.stream = null;
    this.chunks = [];
    if (context && context.state !== 'closed') {
      await context.close().catch(() => undefined);
    }
  }
}

export function createPromptAudioRecorder(): PromptAudioRecorder {
  return pushToTalkWindow()?.__ECKY_TEST_AUDIO_RECORDER__ ?? new BrowserPcmWavRecorder();
}
