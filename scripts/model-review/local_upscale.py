"""A texture enlarged 4x on this PC's GPU by Real-ESRGAN x4plus (xinntao's RRDBNet weights, RealESRGAN_x4plus.pth from the
project's v0.1.0 release; sha256 4fa0d389...82f1), with no image API. The network is defined here, as basicsr's rrdbnet_arch,
and the weights are read with weights_only so the file can hold nothing but tensors.

A tiling texture is padded with its own opposite edges before it is enlarged, so that its edges still meet once cropped."""
import numpy as np
import torch
import torch.nn.functional as F
from torch import nn


class ResidualDenseBlock(nn.Module):
    def __init__(self, num_feat=64, num_grow_ch=32):
        super().__init__()
        self.conv1 = nn.Conv2d(num_feat, num_grow_ch, 3, 1, 1)
        self.conv2 = nn.Conv2d(num_feat + num_grow_ch, num_grow_ch, 3, 1, 1)
        self.conv3 = nn.Conv2d(num_feat + 2 * num_grow_ch, num_grow_ch, 3, 1, 1)
        self.conv4 = nn.Conv2d(num_feat + 3 * num_grow_ch, num_grow_ch, 3, 1, 1)
        self.conv5 = nn.Conv2d(num_feat + 4 * num_grow_ch, num_feat, 3, 1, 1)
        self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)

    def forward(self, x):
        x1 = self.lrelu(self.conv1(x))
        x2 = self.lrelu(self.conv2(torch.cat((x, x1), 1)))
        x3 = self.lrelu(self.conv3(torch.cat((x, x1, x2), 1)))
        x4 = self.lrelu(self.conv4(torch.cat((x, x1, x2, x3), 1)))
        return self.conv5(torch.cat((x, x1, x2, x3, x4), 1)) * 0.2 + x


class RRDB(nn.Module):
    def __init__(self, num_feat, num_grow_ch=32):
        super().__init__()
        self.rdb1 = ResidualDenseBlock(num_feat, num_grow_ch)
        self.rdb2 = ResidualDenseBlock(num_feat, num_grow_ch)
        self.rdb3 = ResidualDenseBlock(num_feat, num_grow_ch)

    def forward(self, x):
        return self.rdb3(self.rdb2(self.rdb1(x))) * 0.2 + x


class RRDBNet(nn.Module):
    def __init__(self, num_feat=64, num_block=23, num_grow_ch=32):
        super().__init__()
        self.conv_first = nn.Conv2d(3, num_feat, 3, 1, 1)
        self.body = nn.Sequential(*[RRDB(num_feat, num_grow_ch) for _ in range(num_block)])
        self.conv_body = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_up1 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_up2 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_hr = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
        self.conv_last = nn.Conv2d(num_feat, 3, 3, 1, 1)
        self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)

    def forward(self, x):
        feat = self.conv_first(x)
        feat = feat + self.conv_body(self.body(feat))
        feat = self.lrelu(self.conv_up1(F.interpolate(feat, scale_factor=2, mode='nearest')))
        feat = self.lrelu(self.conv_up2(F.interpolate(feat, scale_factor=2, mode='nearest')))
        return self.conv_last(self.lrelu(self.conv_hr(feat)))


class SpandrelUpscaler:
    """Another 4x model (Kim2091's PBRify game-texture upscalers: RealPLKSR, DAT2), its architecture found by spandrel from
    the weights, read as tensors only (safetensors, or torch's weights_only)."""

    def __init__(self, weights, bf16=True):
        import spandrel
        from safetensors.torch import load_file
        weights = str(weights)
        sd = load_file(weights) if weights.endswith('.safetensors') else torch.load(weights, map_location='cpu', weights_only=True)
        self.model = spandrel.ModelLoader().load_from_state_dict(sd)
        if self.model.scale != 4:
            raise ValueError(f'{weights}: scale {self.model.scale}, not 4')
        # Half precision where the architecture takes it, else bfloat16 (DAT2 takes only bfloat16: on the GPU's tensor cores it
        # ran several times faster than float32), else float32.
        self.dtype = (torch.float16 if self.model.supports_half else torch.bfloat16 if self.model.supports_bfloat16 and bf16
                      else torch.float32)
        self.net = self.model.model.eval().cuda().to(self.dtype)

    @torch.inference_mode()
    def __call__(self, rgb, tiling, tile=256):
        h, w = rgb.shape[:2]
        pad = max(8, min(h, w, tile) // 8)
        dtype = self.dtype
        x = torch.from_numpy(np.ascontiguousarray(rgb)).cuda().permute(2, 0, 1)[None].to(dtype) / 255
        x = F.pad(x, (pad, pad, pad, pad), mode='circular' if tiling else 'replicate')
        out = torch.empty((3, 4 * h, 4 * w), dtype=dtype, device='cuda')
        for y0 in range(0, h, tile):
            for x0 in range(0, w, tile):
                y1, x1 = min(y0 + tile, h), min(x0 + tile, w)
                part = self.net(x[:, :, y0:y1 + 2 * pad, x0:x1 + 2 * pad]).clamp(0, 1)[0]
                out[:, 4 * y0:4 * y1, 4 * x0:4 * x1] = part[:, 4 * pad:4 * (pad + y1 - y0), 4 * pad:4 * (pad + x1 - x0)]
        return (out.permute(1, 2, 0).float().cpu().numpy() * 255 + 0.5).astype(np.uint8)


class Upscaler:
    def __init__(self, weights):
        self.net = RRDBNet()
        self.net.load_state_dict(torch.load(weights, map_location='cpu', weights_only=True)['params_ema'], strict=True)
        self.net = self.net.eval().half().cuda()

    @torch.inference_mode()
    def __call__(self, rgb, tiling, tile=256):
        """rgb: HxWx3 uint8 -> 4H x 4W x 3 uint8. A picture larger than `tile` is enlarged a tile at a time, each with a
        margin of its neighbours that is cropped away (a 1024 picture whole needs several GB of feature maps)."""
        h, w = rgb.shape[:2]
        pad = max(4, min(h, w, tile) // 8)
        x = torch.from_numpy(np.ascontiguousarray(rgb)).cuda().permute(2, 0, 1)[None].half() / 255
        x = F.pad(x, (pad, pad, pad, pad), mode='circular' if tiling else 'replicate')
        out = torch.empty((3, 4 * h, 4 * w), dtype=torch.float16, device='cuda')
        for y0 in range(0, h, tile):
            for x0 in range(0, w, tile):
                y1, x1 = min(y0 + tile, h), min(x0 + tile, w)
                part = self.net(x[:, :, y0:y1 + 2 * pad, x0:x1 + 2 * pad]).clamp(0, 1)[0]
                out[:, 4 * y0:4 * y1, 4 * x0:4 * x1] = part[:, 4 * pad:4 * (pad + y1 - y0), 4 * pad:4 * (pad + x1 - x0)]
        return (out.permute(1, 2, 0).float().cpu().numpy() * 255 + 0.5).astype(np.uint8)
