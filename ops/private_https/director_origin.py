"""Update only the explicitly disposable Director instance during origin migration."""
import json
import os
import sys
from pathlib import Path
root = Path("/srv/rack-ai-media")
for line in (root / "director-test/environment").read_text().splitlines():
    key, separator, value = line.partition("=")
    if separator and not key.startswith("#"):
        os.environ[key] = value
release = root / "releases/director-1e7fa41f0ad1424b10c82a84dec1e7bc5be188ed"
os.chdir(release)
sys.path.insert(0, str(release))
os.environ["DJANGO_SETTINGS_MODULE"] = "director.settings"
import django
django.setup()
from django.conf import settings
from director.models import AppSetting
from director.rack_media.configuration import ENDPOINT_KEY
root = Path("/srv/rack-ai-media")
expected = root / "director-test/director.sqlite3"
if Path(settings.DATABASES["default"]["NAME"]).resolve() != expected:
    raise RuntimeError("Refusing to update a different Director database")
origin = json.loads((root / "config.json").read_text())["public_origin"]
if origin not in ("https://gpurack.duckdns.org", "https://gpurack.tailc214fc.ts.net"):
    raise RuntimeError("Unexpected launcher origin")
AppSetting.set(ENDPOINT_KEY, origin)
print("Disposable Director endpoint updated to " + origin)
