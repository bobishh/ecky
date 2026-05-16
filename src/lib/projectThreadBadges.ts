import type { Thread } from './types/domain';

export type ProjectThreadBadge = {
  label: string;
  className: 'queued' | 'confirm';
  title: string;
};

export function deriveProjectThreadBadges(
  thread: Pick<Thread, 'queuedCount' | 'pendingConfirm'>,
): ProjectThreadBadge[] {
  const badges: ProjectThreadBadge[] = [];
  const queuedCount = Number(thread.queuedCount || 0);
  const pendingConfirm = thread.pendingConfirm?.trim() || '';

  if (queuedCount > 0) {
    badges.push({
      label: `INBOX ${queuedCount}`,
      className: 'queued',
      title: `${queuedCount} queued user ${queuedCount === 1 ? 'message' : 'messages'} waiting in this thread`,
    });
  }

  if (pendingConfirm) {
    badges.push({
      label: 'CONFIRM',
      className: 'confirm',
      title: `Pending confirmation: ${pendingConfirm}`,
    });
  }

  return badges;
}
