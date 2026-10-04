export function codexQueueErrorCopy(status: string, error: string | null): string | null {
  if (status === 'queued' && error && /already has an active writer|already has an active or pending turn/i.test(error)) {
    return 'Codex is busy. This request will send automatically when ready.';
  }
  return error;
}
