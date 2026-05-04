import {
  buildExportChooserOptions,
  buildExportDefaultNames,
  buildMultipartExportParts,
  hasMultipartExportAssets,
} from '../exportOptions';
import type { ArtifactBundle, Message, RuntimeCapabilities } from '../types/domain';
import type { ExportChooserOption, MultipartExportPart } from '../exportOptions';

export type ExportStateInput = {
  activeArtifactBundle: ArtifactBundle | null;
  activeThreadTitle: string | null;
  activeVersionMessage: Message | null;
  runtimeCapabilities?: RuntimeCapabilities | null;
};

export type ExportState = {
  exportModelTitle: string;
  exportDefaultNames: ReturnType<typeof buildExportDefaultNames>;
  exportOptions: ExportChooserOption[];
  hasMultipartExportModel: boolean;
  multipartExportParts: MultipartExportPart[];
  canExportModel: boolean;
};

export function deriveExportState(input: ExportStateInput): ExportState {
  const exportModelTitle =
    input.activeVersionMessage?.output?.title?.trim() ||
    input.activeThreadTitle?.trim() ||
    'design';
  const exportDefaultNames = buildExportDefaultNames(exportModelTitle);
  const exportOptions = buildExportChooserOptions(
    input.activeArtifactBundle,
    input.runtimeCapabilities,
  );
  const hasMultipartExportModel = hasMultipartExportAssets(input.activeArtifactBundle);
  const multipartExportParts = buildMultipartExportParts(input.activeArtifactBundle);
  const canExportModel = Boolean(
    input.activeArtifactBundle && exportOptions.some((option) => !option.disabled),
  );

  return {
    exportModelTitle,
    exportDefaultNames,
    exportOptions,
    hasMultipartExportModel,
    multipartExportParts,
    canExportModel,
  };
}
