import tempfile
import unittest
import zipfile
from pathlib import Path

from package_models import package_model


class PackageModelsTest(unittest.TestCase):
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
