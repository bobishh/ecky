"""Rebuild deterministic model ZIPs from the checked-in gallery snapshots."""
import json
import zipfile
from pathlib import Path


def package_model(destination, files):
    with zipfile.ZipFile(destination, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, path in sorted(files.items()):
            info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, Path(path).read_bytes(), compresslevel=9)


if __name__ == '__main__':
    site = Path(__file__).resolve().parents[1]
    for model in json.loads((site / 'src/showcase/models.json').read_text()):
        sources = [{'url': model['sourceUrl'], 'downloadName': model['sourceDownloadName']}]
        sources += model.get('companionSources', [])
        files = {source['downloadName']: site / 'public' / source['url'].lstrip('/') for source in sources}
        files.update({'parts/' + part['downloadName']: site / 'public' / part['url'].lstrip('/') for part in model['parts']})
        package_model(site / 'public' / model['archiveUrl'].lstrip('/'), files)
