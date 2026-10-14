"""Export the trained network to ONNX with its preprocessing built in.

    python ml/export_onnx.py [--out ml/runs/water-unet.onnx]

The exported graph takes Sentinel-2 surface reflectance (0..1) for bands
B02 B03 B04 B08 B11 B12 as a (1, 6, H, W) float tensor, H and W multiples
of 8, and returns the water probability as (1, 1, H, W). Lifeline loads it
with `LIFELINE_WATER_MODEL=/path/to/water-unet.onnx`.
"""

import argparse
import pathlib

import numpy as np
import torch
from torch import nn

from common import BANDS, CHANNELS
from model import UNet

RUNS = pathlib.Path(__file__).parent / "runs"


class Deployed(nn.Module):
    def __init__(self, net: UNet, stats: dict) -> None:
        super().__init__()
        self.net = net
        self.register_buffer("mean", torch.tensor(stats["mean"]).view(1, -1, 1, 1))
        self.register_buffer("std", torch.tensor(stats["std"]).view(1, -1, 1, 1))

    def forward(self, refl: torch.Tensor) -> torch.Tensor:
        g, n, s = refl[:, 1:2], refl[:, 3:4], refl[:, 4:5]
        mndwi = (g - s) / torch.clamp(g + s, min=1e-4)
        ndwi = (g - n) / torch.clamp(g + n, min=1e-4)
        x = (torch.cat([refl, mndwi, ndwi], dim=1) - self.mean) / self.std
        return self.net(x).softmax(1)[:, 1:2]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ckpt", default=RUNS / "best.pt", type=pathlib.Path)
    ap.add_argument("--out", default=RUNS / "water-unet.onnx", type=pathlib.Path)
    args = ap.parse_args()

    ckpt = torch.load(args.ckpt, weights_only=False)
    assert ckpt["stats"]["channels"] == CHANNELS
    net = UNet()
    net.load_state_dict(ckpt["state"])
    model = Deployed(net, ckpt["stats"]).eval()

    example = torch.rand(1, len(BANDS), 256, 256) * 0.3
    torch.onnx.export(
        model,
        example,
        args.out,
        input_names=["reflectance"],
        output_names=["water"],
        dynamic_axes={"reflectance": {2: "height", 3: "width"}, "water": {2: "height", 3: "width"}},
        opset_version=17,
        dynamo=False,
    )

    import onnxruntime as ort

    sess = ort.InferenceSession(str(args.out))
    probe = (torch.rand(1, len(BANDS), 128, 192) * 0.3).numpy()
    ours = model(torch.from_numpy(probe)).detach().numpy()
    theirs = sess.run(None, {"reflectance": probe})[0]
    err = float(np.abs(ours - theirs).max())
    print(f"wrote {args.out} ({args.out.stat().st_size / 1e6:.1f} MB), max |torch - onnx| = {err:.2e}")
    assert err < 1e-4


if __name__ == "__main__":
    main()
