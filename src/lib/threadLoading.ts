export type ThreadLoadSnapshot = {
  activeThreadId: string | null;
  loadingThreadId: string | null;
  threadHasMessages: boolean;
  threadMessagesLoading: boolean;
};

export function isCurrentThreadLoad(
  token: number,
  latestToken: number,
  activeThreadId: string | null,
  targetThreadId: string,
): boolean {
  return token === latestToken && activeThreadId === targetThreadId;
}

export function shouldSkipThreadSelect(
  targetThreadId: string,
  snapshot: ThreadLoadSnapshot,
): boolean {
  if (snapshot.activeThreadId !== targetThreadId) return false;
  if (snapshot.loadingThreadId === targetThreadId) return true;
  return snapshot.threadHasMessages && !snapshot.threadMessagesLoading;
}

export function shouldShowDialoguePreloader(messagesLoading: boolean): boolean {
  return messagesLoading;
}
