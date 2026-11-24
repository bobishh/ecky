import hashlib
import json
import tempfile
import unittest
import zipfile
from pathlib import Path

from package_models import package_model


class PackageModelsTest(unittest.TestCase):
    def test_gallery_archives_and_preview_addresses_match_published_assets(self):
        site = Path(__file__).resolve().parents[1]
        models = json.loads((site / 'src/showcase/models.json').read_text())
        for model in models:
            with self.subTest(model=model['id']):
                source = site / 'public' / model['sourceUrl'].lstrip('/')
                published_source = source.read_bytes()
                canonical = site / 'src/models' / model['sourceDownloadName']
                if canonical.is_file():
                    self.assertEqual(canonical.read_bytes(), published_source)
                snapshot_digest = model.get('snapshot', {}).get('sourceSha256')
                if snapshot_digest:
                    self.assertEqual(snapshot_digest, hashlib.sha256(published_source).hexdigest())
                files = {model['sourceDownloadName']: source}
                for companion in model.get('companionSources', []):
                    files[companion['downloadName']] = site / 'public' / companion['url'].lstrip('/')
                for part in model['parts']:
                    files[f"parts/{part['downloadName']}"] = site / 'public' / part['url'].lstrip('/')

                archive_path = site / 'public' / model['archiveUrl'].lstrip('/')
                with zipfile.ZipFile(archive_path) as archive:
                    self.assertCountEqual(archive.namelist(), files.keys())
                    for name, published in files.items():
                        self.assertEqual(archive.read(name), published.read_bytes(), name)

                preview = model.get('preview', {})
                source_digest = preview.get('provenance', {}).get('sourceSha256')
                if source_digest:
                    self.assertEqual(source_digest, hashlib.sha256(published_source).hexdigest())
                for part in preview.get('parts', []):
                    asset = site / 'public' / part['url'].lstrip('/')
                    self.assertEqual(asset.stem.rsplit('-', 1)[-1], hashlib.sha256(asset.read_bytes()).hexdigest()[:12])

    def test_archive_preserves_named_parts_and_exact_source(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'model.ecky'
            source.write_text('(model (part body (box 1 2 3)))')
            part = root / 'body.stl'
            part.write_bytes(b'published mesh bytes')
            archive = root / 'example.zip'
            package_model(archive, {'model.ecky': source, 'parts/body.stl': part})
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(set(bundle.namelist()), {'model.ecky', 'parts/body.stl'})
                self.assertEqual(bundle.read('parts/body.stl'), part.read_bytes())
                self.assertEqual(bundle.read('model.ecky'), source.read_bytes())
            first = archive.read_bytes()
            package_model(archive, {'model.ecky': source, 'parts/body.stl': part})
            self.assertEqual(first, archive.read_bytes())


if __name__ == '__main__':
    unittest.main()
