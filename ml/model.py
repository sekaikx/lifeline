"""A small U-Net: four scales, about half a million parameters, fully
convolutional so it runs on any tile whose sides are multiples of 8."""

import torch
from torch import nn


def block(cin: int, cout: int) -> nn.Sequential:
    return nn.Sequential(
        nn.Conv2d(cin, cout, 3, padding=1, bias=False),
        nn.BatchNorm2d(cout),
        nn.ReLU(inplace=True),
        nn.Conv2d(cout, cout, 3, padding=1, bias=False),
        nn.BatchNorm2d(cout),
        nn.ReLU(inplace=True),
    )


class UNet(nn.Module):
    def __init__(self, cin: int = 8, width: int = 16, classes: int = 2) -> None:
        super().__init__()
        w = [width, width * 2, width * 4, width * 8]
        self.enc = nn.ModuleList([block(cin, w[0]), block(w[0], w[1]), block(w[1], w[2]), block(w[2], w[3])])
        self.pool = nn.MaxPool2d(2)
        self.up = nn.ModuleList([nn.ConvTranspose2d(w[i + 1], w[i], 2, stride=2) for i in range(3)])
        self.dec = nn.ModuleList([block(w[i] * 2, w[i]) for i in range(3)])
        self.head = nn.Conv2d(w[0], classes, 1)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        skips = []
        for i, enc in enumerate(self.enc):
            x = enc(x)
            if i < 3:
                skips.append(x)
                x = self.pool(x)
        for i in reversed(range(3)):
            x = self.up[i](x)
            x = self.dec[i](torch.cat([x, skips[i]], dim=1))
        return self.head(x)
