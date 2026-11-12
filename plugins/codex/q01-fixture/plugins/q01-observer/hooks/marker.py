"""Write only a fixed delivery marker to the host-provided plugin data directory."""
import os
from pathlib import Path

root = Path(os.environ["PLUGIN_DATA"])
root.mkdir(mode=0o700, parents=True, exist_ok=True)
with (root / "q01-delivery.log").open("a", encoding="utf-8") as marker:
    marker.write("SessionStart delivered\n")
