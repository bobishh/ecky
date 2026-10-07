import { listen } from '@tauri-apps/api/event';
import { formatBackendError, startCodexVoice, stopCodexVoice } from '../tauri/client';
import type { CodexVoiceConnection, CodexVoiceStartInput, CodexVoiceStopInput } from '../tauri/contracts';

export type VoiceMediaState = 'connecting' | 'connected' | 'closed' | 'error';
type VoiceEvent = { sessionId?: string | null; threadId: string; method: string; params: { message?: string; reason?: string | null } };
export type CodexVoicePorts = {
  microphone: () => Promise<MediaStream>;
  peer: () => RTCPeerConnection;
  audio: () => HTMLAudioElement;
  sessionId: () => string;
  start: (input: CodexVoiceStartInput) => Promise<CodexVoiceConnection>;
  stop: (input: CodexVoiceStopInput) => Promise<void>;
  listen: (callback: (event: VoiceEvent) => void) => Promise<() => void>;
};

const nativePorts: CodexVoicePorts = {
  microphone: () => {
    if (!navigator.mediaDevices?.getUserMedia) {
      throw new Error('Microphone capture unavailable: mediaDevices.getUserMedia is missing in this webview.');
    }
    return navigator.mediaDevices.getUserMedia({ audio: true });
  },
  peer: () => new RTCPeerConnection(),
  audio: () => new Audio(),
  sessionId: () => crypto.randomUUID(),
  start: startCodexVoice,
  stop: stopCodexVoice,
  listen: (callback) => listen<VoiceEvent>('codex-voice-event', (event) => callback(event.payload)),
};

/** Audio transport only. Rust owns provider session, dialogue and CAD lifecycle. */
export class CodexVoiceMedia {
  private closed = false;
  private stream: MediaStream | null = null;
  private peer: RTCPeerConnection | null = null;
  private audio: HTMLAudioElement | null = null;
  private unlisten: (() => void) | null = null;
  private connection: CodexVoiceConnection | null = null;
  private starting: Promise<CodexVoiceConnection> | null = null;
  private stopping: Promise<void> | null = null;
  private readonly sessionId: string;

  constructor(
    private readonly eckyThreadId: string,
    private readonly ports: CodexVoicePorts = nativePorts,
    private readonly project: (state: VoiceMediaState, detail?: string) => void = () => {},
  ) {
    this.sessionId = ports.sessionId();
  }

  async connect(): Promise<void> {
    this.project('connecting');
    try {
      const stream = await this.ports.microphone();
      if (this.closed) { stream.getTracks().forEach((track) => track.stop()); return; }
      this.stream = stream;
      const peer = this.ports.peer();
      this.peer = peer;
      const audio = this.ports.audio();
      this.audio = audio;
      audio.autoplay = true;
      peer.ontrack = (event) => {
        if (this.closed) return;
        audio.srcObject = event.streams[0] ?? new MediaStream([event.track]);
        void audio.play().catch((error) => this.fail(error));
      };
      peer.onconnectionstatechange = () => {
        if (this.closed) return;
        if (peer.connectionState === 'connected') this.project('connected');
        if (peer.connectionState === 'failed') this.fail(new Error('Codex audio WebRTC connection failed.'));
      };
      for (const track of stream.getTracks()) peer.addTrack(track, stream);
      // Codex requires a realtime data channel in the SDP even though Rust's
      // authenticated sideband owns transcript and delegation notifications.
      peer.createDataChannel('oai-events');
      const unlisten = await this.ports.listen((event) => {
        if (this.closed) return;
        if (event.sessionId ? event.sessionId !== this.sessionId : event.threadId !== this.connection?.threadId) return;
        if (event.method === 'thread/realtime/error') this.fail(new Error(event.params.message ?? 'Codex realtime error'));
        if (event.method === 'thread/realtime/closed') {
          this.project('closed', event.params.reason ?? undefined);
          void this.close().catch((error) => this.project('error', formatBackendError(error)));
        }
      });
      if (this.closed) { unlisten(); return; }
      this.unlisten = unlisten;
      const offer = await peer.createOffer();
      await peer.setLocalDescription(offer);
      if (this.closed) return;
      const sdp = peer.localDescription?.sdp;
      if (!sdp) throw new Error('WebRTC did not produce a Codex audio SDP offer.');
      this.starting = this.ports.start({ eckyThreadId: this.eckyThreadId, sessionId: this.sessionId, sdp });
      this.connection = await this.starting;
      if (this.closed) { await this.stopProvider(); return; }
      await peer.setRemoteDescription({ type: 'answer', sdp: this.connection.sdp });
    } catch (error) {
      if (this.closed) return;
      this.fail(error);
    }
  }

  private fail(error: unknown): void {
    if (this.closed) return;
    this.project('error', formatBackendError(error));
    void this.close().catch((stopError) => this.project('error', `${formatBackendError(error)}\n${formatBackendError(stopError)}`));
  }

  private stopProvider(): Promise<void> {
    this.stopping ??= (async () => {
      if (!this.starting) return;
      await this.ports.stop({ eckyThreadId: this.eckyThreadId, sessionId: this.sessionId });
    })();
    return this.stopping;
  }

  async close(): Promise<void> {
    this.closed = true;
    this.stream?.getTracks().forEach((track) => track.stop());
    this.stream = null;
    this.peer?.close();
    this.peer = null;
    if (this.audio) { this.audio.pause(); this.audio.srcObject = null; this.audio = null; }
    this.unlisten?.();
    this.unlisten = null;
    await this.stopProvider();
  }
}
