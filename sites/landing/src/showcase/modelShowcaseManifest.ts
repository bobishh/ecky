import snapshots from './models.json';

export type ModelShowcasePart = {
  label: string;
  url: string;
  downloadName: string;
  color: string;
};

export type ModelShowcaseVariant = {
  id: string;
  label: string;
  title: string;
  note: string;
  sourceUrl: string;
  sourceDownloadName: string;
  sourceLabel?: string;
  companionSources?: Array<{
    label: string;
    url: string;
    downloadName: string;
  }>;
  archiveUrl: string;
  archiveDownloadName: string;
  view: { yaw: number; pitch: number };
  parts: ModelShowcasePart[];
  preview?: { parts: ModelShowcasePart[]; upAxis: 'y' | 'z'; layout: string };
};

export const modelShowcaseVariants = snapshots as ModelShowcaseVariant[];
