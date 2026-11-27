import { buildMultipartExportParts, getStepExportPath, type ExportMode, type MultipartExportPart } from '../exportOptions';
import type { ArtifactBundle } from '../types/domain';

type DefaultNames = { threeMf: string; multipartStlZip: string; stl: string; step: string; fcstd: string };
type Save = (options: { filters: { name: string; extensions: string[] }[]; defaultPath: string }) => Promise<string | null>;

export function createModelIo(deps: {
  save: Save;
  exportFile: (source: string, target: string) => Promise<void>;
  export3mf: (parts: MultipartExportPart[], target: string, threadId: string | null) => Promise<void>;
  exportMultipartStlZip: (parts: MultipartExportPart[], target: string, title: string) => Promise<void>;
  setStatus: (message: string) => void;
  setError: (message: string) => void;
  formatError: (error: unknown) => string;
}) {
  async function exportModel(mode: ExportMode, bundle: ArtifactBundle | null, names: DefaultNames, parts: MultipartExportPart[], isMultipart: boolean, title: string, threadId: string | null) {
    if (!bundle) return;
    try {
      if (mode === '3mf') {
        const stlParts = buildMultipartExportParts(bundle, 'stl');
        if (stlParts.length === 0 && bundle.modelStlPath) {
          stlParts.push({
            label: title,
            path: bundle.modelStlPath,
            objectName: null,
            partId: 'model',
            displayColor: null,
          });
        }
        const path = await deps.save({ filters: [{ name: '3MF Package', extensions: ['3mf'] }], defaultPath: names.threeMf });
        if (!path) return;
        await deps.export3mf(stlParts, path, threadId);
        deps.setStatus('Exported 3MF.');
        return;
      }
      if (mode === 'multipartStlZip') {
        if (!isMultipart) return;
        const path = await deps.save({ filters: [{ name: 'Multipart STL Archive', extensions: ['zip'] }], defaultPath: names.multipartStlZip });
        if (!path) return;
        await deps.exportMultipartStlZip(parts, path, title);
        deps.setStatus('Exported multipart STL archive.');
        return;
      }
      const source = mode === 'stl' ? bundle.modelStlPath : mode === 'step' ? getStepExportPath(bundle) : bundle.fcstdPath;
      if (!source) return;
      const type = mode === 'stl' ? ['STL 3D Model', ['stl'], names.stl] : mode === 'step' ? ['STEP CAD Model', ['step', 'stp'], names.step] : ['FreeCAD Document', ['FCStd'], names.fcstd];
      const path = await deps.save({ filters: [{ name: type[0] as string, extensions: type[1] as string[] }], defaultPath: type[2] as string });
      if (!path) return;
      await deps.exportFile(source, path);
      deps.setStatus(mode === 'stl' ? (isMultipart ? 'Exported flattened STL. Use 3MF or Multipart STL to preserve separate bodies.' : 'Exported STL.') : mode === 'step' ? 'Exported STEP.' : 'Exported FCStd.');
    } catch (error) { deps.setError(`Export Error: ${deps.formatError(error)}`); }
  }
  return { exportModel };
}
