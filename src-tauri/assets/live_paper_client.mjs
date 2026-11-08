const params = new URLSearchParams(location.search);
const sessionId = params.get('session');
const token = params.get('token');
const video = document.querySelector('#camera');
const startButton = document.querySelector('#start');
const stopButton = document.querySelector('#stop');
const status = document.querySelector('#status');
const help = document.querySelector('#help');
const wholeFrame = document.querySelector('#whole-frame');
const portraitLayout = window.matchMedia('(orientation: portrait)');
let peer = null;
let stream = null;
let wakeLock = null;
let generation = 0;
let constraintUpdate = Promise.resolve();

function report(message) { status.textContent = message; }

function updateViewport() {
  const viewport = window.visualViewport;
  const root = document.documentElement.style;
  root.setProperty('--viewport-width', `${viewport?.width ?? window.innerWidth}px`);
  root.setProperty('--viewport-height', `${viewport?.height ?? window.innerHeight}px`);
  root.setProperty('--viewport-top', `${viewport?.offsetTop ?? 0}px`);
  root.setProperty('--viewport-left', `${viewport?.offsetLeft ?? 0}px`);
}

function cameraConstraints() {
  const portrait = portraitLayout.matches;
  return {
    facingMode: { ideal: 'environment' },
    width: { ideal: portrait ? 720 : 1280, max: portrait ? 1080 : 1920 },
    height: { ideal: portrait ? 1280 : 720, max: portrait ? 1920 : 1080 },
    aspectRatio: { ideal: portrait ? 9 / 16 : 16 / 9 },
    frameRate: { ideal: 15, max: 20 },
  };
}

function refreshCameraOrientation() {
  const attempt = generation;
  const currentStream = stream;
  constraintUpdate = constraintUpdate.then(async () => {
    if (!currentStream || stream !== currentStream || attempt !== generation) return;
    const track = currentStream.getVideoTracks()[0];
    if (!track) return;
    try { await track.applyConstraints(cameraConstraints()); }
    catch {
      if (stream === currentStream && attempt === generation) report('Camera kept its current resolution. The whole frame remains visible.');
    }
  });
  return constraintUpdate;
}

updateViewport();
window.addEventListener('resize', updateViewport);
window.visualViewport?.addEventListener('resize', updateViewport);
window.visualViewport?.addEventListener('scroll', updateViewport);
portraitLayout.addEventListener('change', () => { updateViewport(); void refreshCameraOrientation(); });

document.querySelector('#help-open').addEventListener('click', () => help.showModal());
document.querySelector('#help-close').addEventListener('click', () => help.close());
wholeFrame.addEventListener('change', () => video.classList.toggle('whole-frame', wholeFrame.checked));

async function releaseWakeLock() {
  const active = wakeLock;
  wakeLock = null;
  if (active) await active.release().catch(() => {});
}

async function acquireWakeLock() {
  if (!stream || wakeLock && !wakeLock.released || document.visibilityState !== 'visible' || !navigator.wakeLock) return;
  const attempt = generation;
  try {
    const acquired = await navigator.wakeLock.request('screen');
    if (attempt !== generation || !stream || document.visibilityState !== 'visible') await acquired.release();
    else if (wakeLock && !wakeLock.released) await acquired.release();
    else wakeLock = acquired;
  } catch { /* Streaming works without screen Wake Lock. */ }
}

function stopCamera(message = 'Camera stopped. Accepted sketch geometry stays in Ecky.') {
  generation += 1;
  const active = peer;
  peer = null;
  if (active) active.close();
  if (stream) for (const track of stream.getTracks()) track.stop();
  stream = null;
  video.srcObject = null;
  void releaseWakeLock();
  startButton.disabled = false;
  stopButton.disabled = true;
  report(message);
}

function waitForIceGathering(active) {
  if (active.iceGatheringState === 'complete') return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { cleanup(); reject(new Error('Local network candidates could not be collected. Try starting the camera again.')); }, 12000);
    function cleanup() { clearTimeout(timer); active.removeEventListener('icegatheringstatechange', changed); }
    function changed() { if (active.iceGatheringState === 'complete') { cleanup(); resolve(); } }
    active.addEventListener('icegatheringstatechange', changed);
  });
}

export async function startCamera() {
  if (!sessionId || !token) { report('Pairing link is incomplete. Open a new live-paper link from Ecky.'); return; }
  if (!navigator.mediaDevices?.getUserMedia || !window.RTCPeerConnection) {
    report('Camera requires a trusted HTTPS page. Install and trust Ecky’s local certificate, then reopen this link in Safari.');
    return;
  }
  const attempt = ++generation;
  startButton.disabled = true;
  stopButton.disabled = false;
  report('Requesting camera permission…');
  try {
    const initialPortrait = portraitLayout.matches;
    const media = await navigator.mediaDevices.getUserMedia({ audio: false, video: cameraConstraints() });
    if (attempt !== generation) { for (const track of media.getTracks()) track.stop(); return; }
    stream = media;
    video.srcObject = stream;
    await video.play();
    if (attempt !== generation) return;
    if (initialPortrait !== portraitLayout.matches) await refreshCameraOrientation();
    if (attempt !== generation) return;
    report('Connecting encrypted live video…');
    const active = new RTCPeerConnection({ iceServers: [] });
    peer = active;
    const transceiver = active.addTransceiver(stream.getVideoTracks()[0], { direction: 'sendonly', streams: [stream] });
    const codecs = RTCRtpSender.getCapabilities?.('video')?.codecs?.filter(codec =>
      codec.mimeType.toLowerCase() === 'video/h264' && /packetization-mode=1/.test(codec.sdpFmtpLine ?? '') && /profile-level-id=42/i.test(codec.sdpFmtpLine ?? ''));
    if (codecs?.length && transceiver.setCodecPreferences) transceiver.setCodecPreferences(codecs);
    active.addEventListener('connectionstatechange', () => {
      if (peer !== active) return;
      if (active.connectionState === 'connected') report('Streaming live paper to Ecky. Keep all four markers visible.');
      else if (active.connectionState === 'failed') stopCamera('Video connection failed. Check that phone and computer share a local network, then start again.');
      else if (active.connectionState === 'disconnected') report('Video connection interrupted. Accepted geometry stays in Ecky.');
    });
    stream.getVideoTracks()[0].addEventListener('ended', () => { if (peer === active) stopCamera('Camera ended. Start again to resume live paper.'); });
    await active.setLocalDescription(await active.createOffer());
    await waitForIceGathering(active);
    if (attempt !== generation) return;
    report('Connecting encrypted live video…');
    const response = await fetch(`/api/live-paper/${encodeURIComponent(sessionId)}/offer`, {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ token, sdp: active.localDescription.sdp }), signal: AbortSignal.timeout(20000),
    });
    if (!response.ok) throw new Error(await response.text());
    const answer = await response.json();
    if (attempt !== generation) return;
    await active.setRemoteDescription(answer);
    stopButton.disabled = false;
    await acquireWakeLock();
  } catch (error) {
    if (attempt !== generation) return;
    const details = error?.name === 'NotAllowedError' ? 'Camera permission denied. Allow camera access in Safari settings, then start again.' : `Could not start live video: ${error?.message ?? String(error)}`;
    stopCamera(details);
  }
}

startButton.addEventListener('click', () => { void startCamera(); });
stopButton.addEventListener('click', () => stopCamera());
window.addEventListener('pagehide', () => stopCamera());
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') void acquireWakeLock();
  else { void releaseWakeLock(); if (stream) report('Page hidden; Safari may pause the camera. Return here to resume.'); }
});
