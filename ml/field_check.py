"""Check the detectors on the imagery Lifeline actually uses.

Sen1Floods11 chips are Sentinel-2 L1C (top of atmosphere); the app feeds
L2A surface reflectance from Microsoft Planetary Computer. There are no hand
labels for those scenes, so this compares each detector with ESA WorldCover
2021 permanent water on clear dry-season scenes at a handful of sites. It is
a weak reference (WorldCover has its own errors and rivers move), meant to
catch a domain gap, not to rank detectors.

    python ml/field_check.py [--model ml/runs/water-unet.onnx]
"""

import argparse
import io
import json
import pathlib
import urllib.request

import numpy as np

STAC = "https://planetarycomputer.microsoft.com/api/stac/v1/search"
DATA = "https://planetarycomputer.microsoft.com/api/data/v1/item/bbox"
CACHE = pathlib.Path(__file__).parent / "data" / "field"
BANDS = ["B02", "B03", "B04", "B08", "B11", "B12"]
SIZE = 512  # pixels per side at 20 m

SITES = [
    ("Indus near Dadu, Pakistan", 26.75, 67.75, "2021-12-01/2022-01-20"),
    ("Sylhet haor, Bangladesh", 24.95, 91.80, "2023-01-15/2023-03-15"),
    ("Buzi and Pungwe, Mozambique", -19.85, 34.65, "2023-07-01/2023-09-15"),
    ("Tonle Sap, Cambodia", 12.45, 104.50, "2024-01-10/2024-03-10"),
    ("Kahramanmaraş, Türkiye", 37.55, 36.95, "2023-07-01/2023-09-01"),
    ("Po near Piacenza, Italy", 45.08, 9.85, "2023-07-01/2023-08-31"),
]


def fetch(url: str, body: dict | None = None) -> bytes:
    key = CACHE / (str(abs(hash((url, json.dumps(body, sort_keys=True))))) + ".bin")
    if key.exists():
        return key.read_bytes()
    data = json.dumps(body).encode() if body else None
    req = urllib.request.Request(url, data=data, headers={"User-Agent": "lifeline-ml", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=300) as r:
        out = r.read()
    CACHE.mkdir(parents=True, exist_ok=True)
    key.write_bytes(out)
    return out


def bbox(lat: float, lon: float, size_m: float) -> tuple[float, float, float, float]:
    dlat = size_m / 2 / 110_574
    dlon = size_m / 2 / (111_320 * np.cos(np.radians(lat)))
    return lon - dlon, lat - dlat, lon + dlon, lat + dlat


def clearest(collection: str, box, window: str | None) -> dict:
    body = {"collections": [collection], "bbox": list(box), "limit": 50}
    if window:
        body["datetime"] = window
        body["query"] = {"eo:cloud_cover": {"lt": 5}}
    feats = json.loads(fetch(STAC, body))["features"]
    if not feats:
        raise RuntimeError(f"no {collection} item")
    return min(feats, key=lambda f: f["properties"].get("eo:cloud_cover", 0))


def crop(box, collection: str, item: str, assets: list[str]) -> np.ndarray:
    w, s, e, n = box
    q = "&".join(f"assets={a}" for a in assets)
    url = f"{DATA}/{w:.6f},{s:.6f},{e:.6f},{n:.6f}/{SIZE}x{SIZE}.npy?collection={collection}&item={item}&{q}&nodata=0"
    return np.load(io.BytesIO(fetch(url))).astype(np.float64)


def dilate(mask: np.ndarray, r: int) -> np.ndarray:
    out = mask.copy()
    for axis in (0, 1):
        acc = out.copy()
        for d in range(1, r + 1):
            acc |= np.roll(out, d, axis) | np.roll(out, -d, axis)
        out = acc
    return out


def counts(pred: np.ndarray, ref: np.ndarray, valid: np.ndarray) -> np.ndarray:
    """[water found, water missed, land flagged, land clear] over valid pixels."""
    p, t = pred[valid], ref[valid]
    return np.array([(p & t).sum(), (~p & t).sum(), (p & ~t).sum(), (~p & ~t).sum()], dtype=np.int64)


def describe(c: np.ndarray) -> str:
    found, missed, flagged, clear = c
    recall = found / max(found + missed, 1)
    false_alarm = flagged / max(flagged + clear, 1)
    return f"{recall:8.1%} {false_alarm:8.2%}"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=pathlib.Path(__file__).parent / "runs" / "water-unet.onnx", type=pathlib.Path)
    args = ap.parse_args()
    sess = None
    if args.model.exists():
        import onnxruntime as ort

        sess = ort.InferenceSession(str(args.model))

    head = f"{'site':30s} {'scene':10s} {'water':>6s}   {'MNDWI':>8s} {'':8s}"
    if sess:
        head += f"   {'U-Net':>8s}"
    print(head)
    sub = f"{'':30s} {'':10s} {'':6s}   {'recall':>8s} {'false+':>8s}"
    print(sub + (f"   {'recall':>8s} {'false+':>8s}" if sess else ""))
    totals: dict[str, np.ndarray] = {}
    for name, lat, lon, window in SITES:
        box = bbox(lat, lon, SIZE * 20.0)
        item = clearest("sentinel-2-l2a", box, window)
        baseline = float(item["properties"].get("s2:processing_baseline", "0"))
        a = crop(box, "sentinel-2-l2a", item["id"], BANDS + ["SCL"])
        offset = 1000.0 if baseline >= 4.0 else 0.0
        refl = np.clip((a[:6] - offset) / 10_000.0, 0, None).astype(np.float32)
        scl = a[6].astype(np.uint8)
        valid = (a[7] > 0) & np.isin(scl, [4, 5, 6]) & ~dilate(np.isin(scl, [3, 8, 9, 10]), 4)

        wc_item = clearest("esa-worldcover", box, None)
        wc = crop(box, "esa-worldcover", wc_item["id"], ["map"])[0].astype(np.uint8)
        ref = wc == 80
        valid &= ~np.isin(wc, [0, 70, 90])  # no data, snow, herbaceous wetland (ambiguous)

        g, s = refl[1], refl[4]
        preds = {"MNDWI": (g - s) / np.maximum(g + s, 1e-4) > 0.05}
        if sess:
            preds["U-Net"] = sess.run(None, {"reflectance": refl[None]})[0][0, 0] > 0.5
        line = f"{name:30s} {item['properties']['datetime'][:10]:10s} {ref[valid].mean():6.1%}"
        for k, pred in preds.items():
            c = counts(pred, ref, valid)
            totals[k] = totals.get(k, 0) + c
            line += f"   {describe(c)}"
        print(line)
    line = f"{'all sites':30s} {'':10s} {'':6s}"
    for c in totals.values():
        line += f"   {describe(c)}"
    print(line)
    print("recall: share of WorldCover permanent water detected; false+: share of land flagged as water")


if __name__ == "__main__":
    main()
