#!/usr/bin/env python3
"""Downloads a TNTP benchmark network into data/tntp/<Name>/.

    python data/tntp/fetch.py ChicagoRegional Philadelphia

Files come from https://github.com/bstabler/TransportationNetworks. The small networks
(SiouxFalls, Anaheim, ChicagoSketch) are already in the repository.
"""

import sys
import urllib.request
from pathlib import Path

BASE = "https://raw.githubusercontent.com/bstabler/TransportationNetworks/master"

# name used here -> (upstream folder, [files to download])
NETWORKS = {
    "SiouxFalls": ("SiouxFalls", ["SiouxFalls_net.tntp", "SiouxFalls_trips.tntp", "SiouxFalls_flow.tntp"]),
    "Anaheim": ("Anaheim", ["Anaheim_net.tntp", "Anaheim_trips.tntp", "Anaheim_flow.tntp"]),
    "ChicagoSketch": ("Chicago-Sketch", ["ChicagoSketch_net.tntp", "ChicagoSketch_trips.tntp", "ChicagoSketch_flow.tntp"]),
    "ChicagoRegional": ("chicago-regional", ["ChicagoRegional_net.tntp", "ChicagoRegional_trips.tntp", "ChicagoRegional_flow.tntp"]),
    "Philadelphia": ("Philadelphia", ["Philadelphia_net.tntp", "Philadelphia_trips.tntp"]),
    "Winnipeg": ("Winnipeg", ["Winnipeg_net.tntp", "Winnipeg_trips.tntp", "Winnipeg_flow.tntp"]),
    "Barcelona": ("Barcelona", ["Barcelona_net.tntp", "Barcelona_trips.tntp", "Barcelona_flow.tntp"]),
}


def main(names):
    here = Path(__file__).resolve().parent
    for name in names:
        if name not in NETWORKS:
            sys.exit(f"unknown network '{name}'; choose from {', '.join(NETWORKS)}")
        folder, files = NETWORKS[name]
        target = here / name
        target.mkdir(exist_ok=True)
        for file in files:
            path = target / file
            if path.exists():
                print(f"{name}/{file}: already there")
                continue
            print(f"{name}/{file}: downloading ...", flush=True)
            urllib.request.urlretrieve(f"{BASE}/{folder}/{file}", path)
        print(f"{name}: done")


if __name__ == "__main__":
    main(sys.argv[1:] or sys.exit(__doc__))
