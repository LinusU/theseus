#!/usr/bin/env python3
"""4x upscaler for RGBA game textures with alpha-testing (magenta/hard-cutout style
transparency), designed for 1997-era pixel-art-ish textures that tile in-game.

Pipeline per image:
  1. Load RGBA PNG.
  2. If there are any fully-transparent pixels, "bleed" colour from the nearest
     opaque pixels into the transparent region (iterative dilation) so the model
     never sees the magenta/black "matte" colour and doesn't smear it into edges.
  3. Wrap-pad the bled RGB (textures tile in-game) so the model doesn't create a
     seam artefact at the border, then crop the corresponding region back out
     after inference.
  4. Upscale RGB with the selected super-resolution model.
  5. Upscale alpha separately with bicubic interpolation, then clean it up so
     fully-opaque/fully-transparent areas are pure 0/255 (a thin AA band is kept
     at the edge).
  6. Recombine into an RGBA PNG, exactly 4x the input size, same filename.

Setup (see "Texture packs" in README.md for the models and how they compare):
    uv venv --python 3.12 scratch/moto/upscale/.venv && source scratch/moto/upscale/.venv/bin/activate
    uv pip install torch spandrel pillow numpy   # plus mflux for SeedVR2
    # models in scratch/moto/upscale/models/ (or $MOTO_UPSCALE_MODELS):
    #   4xTextureDAT2_otf.safetensors, RealESRGAN_x4plus.pth

Usage, from a texture dump to a texture pack:
    python out/moto/upscale.py --model texturedat2 --in scratch/moto/textures/dump --out scratch/moto/textures/pack
    (other models: realesrgan_x4plus, seedvr2-3b, seedvr2-7b; baselines: bicubic, nearest)
"""
import argparse
import glob
import os
import sys
import time

import numpy as np
from PIL import Image

SCALE = 4
MIN_PAD = 16
PAD_MULTIPLE = 16

MODELS_DIR = os.environ.get("MOTO_UPSCALE_MODELS") or os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "..", "scratch", "moto", "upscale", "models"
)

MODEL_FILES = {
    "realesrgan_x4plus": os.path.join(MODELS_DIR, "RealESRGAN_x4plus.pth"),
    "texturedat2": os.path.join(MODELS_DIR, "4xTextureDAT2_otf.safetensors"),
}


# --------------------------------------------------------------------------
# Alpha bleed (colour dilation into fully-transparent regions)
# --------------------------------------------------------------------------
def bleed_fill(rgb: np.ndarray, alpha: np.ndarray) -> np.ndarray:
    """Fill fully-transparent pixels with colour bled from the nearest opaque
    neighbours via iterative 4-neighbour dilation (edge-clamped, not wrapped:
    this is a local "push" of real colour, independent of the tiling pad step
    that comes later)."""
    h, w = alpha.shape
    known = alpha > 127
    if known.all():
        return rgb.copy()
    out = rgb.astype(np.float32).copy()
    if not known.any():
        out[:] = 128.0
        return out.astype(np.uint8)

    remaining = ~known
    max_iters = h + w
    for _ in range(max_iters):
        if not remaining.any():
            break
        sum_c = np.zeros((h, w, 3), dtype=np.float32)
        cnt = np.zeros((h, w), dtype=np.float32)
        pad_known = np.pad(known, 1, mode="edge")
        pad_color = np.pad(out, ((1, 1), (1, 1), (0, 0)), mode="edge")
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            shifted_known = pad_known[1 + dy : 1 + dy + h, 1 + dx : 1 + dx + w]
            shifted_color = pad_color[1 + dy : 1 + dy + h, 1 + dx : 1 + dx + w, :]
            sum_c[shifted_known] += shifted_color[shifted_known]
            cnt[shifted_known] += 1
        idx = cnt > 0
        avg = np.zeros((h, w, 3), dtype=np.float32)
        avg[idx] = sum_c[idx] / cnt[idx, None]
        newly = remaining & idx
        out[newly] = avg[newly]
        known = known | newly
        remaining = ~known
    return np.clip(out, 0, 255).astype(np.uint8)


# --------------------------------------------------------------------------
# Wrap padding (textures tile in-game) with asymmetric pad to hit a multiple
# of 16 in each padded dimension (needed by SeedVR2; harmless for the others)
# --------------------------------------------------------------------------
def compute_padding(dim, min_pad=MIN_PAD, multiple=PAD_MULTIPLE):
    total_min = dim + 2 * min_pad
    target = ((total_min + multiple - 1) // multiple) * multiple
    extra = target - dim - 2 * min_pad
    pad_before = min_pad
    pad_after = min_pad + extra
    return pad_before, pad_after


def wrap_pad(img: np.ndarray, ph, pw):
    if img.ndim == 2:
        return np.pad(img, (ph, pw), mode="wrap")
    return np.pad(img, (ph, pw, (0, 0)), mode="wrap")


def crop_padding(arr: np.ndarray, ph, pw, scale):
    h, w = arr.shape[0], arr.shape[1]
    top, bottom = ph[0] * scale, h - ph[1] * scale
    left, right = pw[0] * scale, w - pw[1] * scale
    return arr[top:bottom, left:right, ...]


# --------------------------------------------------------------------------
# Alpha upscale + cleanup
# --------------------------------------------------------------------------
def upscale_alpha(alpha: np.ndarray, out_w, out_h) -> np.ndarray:
    if (alpha == 255).all():
        return np.full((out_h, out_w), 255, dtype=np.uint8)
    im = Image.fromarray(alpha, mode="L").resize((out_w, out_h), Image.BICUBIC)
    a = np.asarray(im).astype(np.float32)
    a = np.clip(a, 0, 255)
    a[a < 10] = 0
    a[a > 245] = 255
    return a.astype(np.uint8)


# --------------------------------------------------------------------------
# Tiling helper (only kicks in for images bigger than --tile-size; our test
# textures are well under this, so the untiled path is what actually runs,
# but this keeps the script correct for bigger real-world dumps too).
# --------------------------------------------------------------------------
def run_tiled(rgb: np.ndarray, infer_fn, scale, tile_size, overlap=16):
    h, w = rgb.shape[:2]
    if max(h, w) <= tile_size:
        return infer_fn(rgb)
    out = np.zeros((h * scale, w * scale, 3), dtype=np.uint8)
    step = tile_size - overlap
    for y0 in range(0, h, step):
        for x0 in range(0, w, step):
            y1, x1 = min(y0 + tile_size, h), min(x0 + tile_size, w)
            y0c, x0c = max(0, y1 - tile_size), max(0, x1 - tile_size)
            tile = rgb[y0c:y1, x0c:x1]
            up = infer_fn(tile)
            # region of `up` corresponding to the *new* (non-overlap) area
            oy0, ox0 = (y0 - y0c) * scale, (x0 - x0c) * scale
            oy1, ox1 = (y1 - y0c) * scale, (x1 - x0c) * scale
            out[y0 * scale : y1 * scale, x0 * scale : x1 * scale] = up[oy0:oy1, ox0:ox1]
    return out


# --------------------------------------------------------------------------
# Model runners
# --------------------------------------------------------------------------
class SpandrelRunner:
    def __init__(self, model_key):
        import torch
        from spandrel import ModelLoader

        self.torch = torch
        self.device = "mps" if torch.backends.mps.is_available() else "cpu"
        path = MODEL_FILES[model_key]
        wrapped = ModelLoader().load_from_file(path)
        self.scale = wrapped.scale
        self.model = wrapped.to(self.device).eval()
        print(f"[spandrel] loaded {model_key} ({wrapped.architecture.name}, scale={self.scale}) on {self.device}")

    def infer(self, rgb: np.ndarray) -> np.ndarray:
        torch = self.torch
        x = torch.from_numpy(rgb).permute(2, 0, 1).float().div(255.0).unsqueeze(0).to(self.device)
        with torch.no_grad():
            y = self.model(x)
        y = y.clamp(0, 1).squeeze(0).permute(1, 2, 0).mul(255.0).round().clamp(0, 255)
        return y.to("cpu").numpy().astype(np.uint8)


class SimpleResizeRunner:
    """nearest / bicubic baselines (also used for the comparison sheet)."""

    def __init__(self, method):
        self.scale = SCALE
        self.method = Image.NEAREST if method == "nearest" else Image.BICUBIC

    def infer(self, rgb: np.ndarray) -> np.ndarray:
        h, w = rgb.shape[:2]
        im = Image.fromarray(rgb, mode="RGB").resize((w * SCALE, h * SCALE), self.method)
        return np.asarray(im)


class SeedVR2Runner:
    """Runs ByteDance-Seed's SeedVR2 via the mflux MLX port. Patches a known
    mlx<->mflux incompatibility (mx.repeat called with an array of per-window
    repeat counts, which this mlx build's mx.repeat doesn't support) -- see
    NOTES.md for the exact traceback and upstream issue reference."""

    def __init__(self, model_name, softness=0.5, tmp_dir=None):
        import mlx.core as mx

        _orig_repeat = mx.repeat

        def patched_repeat(arr, repeats, axis=None, **kwargs):
            is_array_like = isinstance(repeats, mx.array) or isinstance(repeats, (list, tuple))
            if not is_array_like:
                return _orig_repeat(arr, repeats, axis=axis, **kwargs)
            rep_list = repeats.tolist() if hasattr(repeats, "tolist") else list(repeats)
            if len(set(rep_list)) <= 1:
                r = rep_list[0] if rep_list else 0
                return _orig_repeat(arr, r, axis=axis, **kwargs)
            work_axis = 0 if axis is None else axis
            if axis is None:
                arr = arr.reshape(-1)
            parts = []
            for i, r in enumerate(rep_list):
                if r == 0:
                    continue
                idx = [slice(None)] * arr.ndim
                idx[work_axis] = slice(i, i + 1)
                parts.append(_orig_repeat(arr[tuple(idx)], r, axis=work_axis))
            if not parts:
                shape = list(arr.shape)
                shape[work_axis] = 0
                return mx.zeros(shape, dtype=arr.dtype)
            return mx.concatenate(parts, axis=work_axis)

        mx.repeat = patched_repeat

        from mflux.models.common.config.model_config import ModelConfig
        from mflux.models.seedvr2.variants.upscale.seedvr2 import SeedVR2
        from mflux.utils.scale_factor import ScaleFactor

        cfg = ModelConfig.seedvr2_7b() if "7b" in model_name else ModelConfig.seedvr2_3b()
        self.model = SeedVR2(model_config=cfg)
        self.ScaleFactor = ScaleFactor
        self.softness = softness
        self.scale = SCALE
        self.tmp_dir = tmp_dir or "/tmp"
        print(f"[seedvr2] loaded {model_name} via mflux (MLX)")

    def infer(self, rgb: np.ndarray) -> np.ndarray:
        tmp_path = os.path.join(self.tmp_dir, "_seedvr2_input.png")
        Image.fromarray(rgb, mode="RGB").save(tmp_path)
        result = self.model.generate_image(
            seed=42,
            image_path=tmp_path,
            resolution=self.ScaleFactor(self.scale),
            softness=self.softness,
        )
        return np.asarray(result.image.convert("RGB"))


def build_runner(model_name, tmp_dir=None):
    if model_name in ("realesrgan_x4plus", "texturedat2"):
        return SpandrelRunner(model_name)
    if model_name in ("nearest", "bicubic"):
        return SimpleResizeRunner(model_name)
    if model_name in ("seedvr2-3b", "seedvr2-7b", "seedvr2"):
        return SeedVR2Runner(model_name, tmp_dir=tmp_dir)
    raise ValueError(f"unknown model {model_name!r}")


# --------------------------------------------------------------------------
# Per-image pipeline
# --------------------------------------------------------------------------
def process_image(path, out_path, runner, tile_size=400):
    im = Image.open(path).convert("RGBA")
    arr = np.asarray(im)
    rgb, alpha = arr[:, :, :3], arr[:, :, 3]
    h, w = alpha.shape

    bled = bleed_fill(rgb, alpha)

    ph = compute_padding(h)
    pw = compute_padding(w)
    padded = wrap_pad(bled, ph, pw)

    def infer_fn(tile_rgb):
        return runner.infer(tile_rgb)

    up = run_tiled(padded, infer_fn, runner.scale, tile_size)
    up = crop_padding(up, ph, pw, runner.scale)

    out_w, out_h = w * SCALE, h * SCALE
    if up.shape[0] != out_h or up.shape[1] != out_w:
        # SeedVR2 (and in principle any model) may round to its own internal
        # grid; correct with a final high-quality resize to the exact 4x size.
        up = np.asarray(Image.fromarray(up, mode="RGB").resize((out_w, out_h), Image.LANCZOS))

    alpha_up = upscale_alpha(alpha, out_w, out_h)
    rgba_out = np.dstack([up, alpha_up])
    assert rgba_out.shape == (out_h, out_w, 4), rgba_out.shape
    Image.fromarray(rgba_out, mode="RGBA").save(out_path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--in", dest="in_dir", required=True)
    ap.add_argument("--out", dest="out_dir", required=True)
    ap.add_argument("--tile-size", type=int, default=400)
    ap.add_argument("--tmp-dir", default=None)
    ap.add_argument(
        "--no-skip-existing",
        dest="skip_existing",
        action="store_false",
        help="By default, images whose output file already exists are skipped "
        "(so the script can be re-run incrementally as new textures are added). "
        "Pass this to force re-processing everything.",
    )
    args = ap.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)
    files = sorted(glob.glob(os.path.join(args.in_dir, "*.png")))
    if not files:
        print(f"no PNGs found in {args.in_dir}", file=sys.stderr)
        sys.exit(1)

    if args.skip_existing:
        pending = [p for p in files if not os.path.exists(os.path.join(args.out_dir, os.path.basename(p)))]
        skipped = len(files) - len(pending)
        if skipped:
            print(f"skipping {skipped} already-processed image(s)")
        files = pending
    if not files:
        print("nothing to do (all outputs already exist)")
        return

    runner = build_runner(args.model, tmp_dir=args.tmp_dir)

    total_t0 = time.time()
    for path in files:
        name = os.path.basename(path)
        out_path = os.path.join(args.out_dir, name)
        t0 = time.time()
        process_image(path, out_path, runner, tile_size=args.tile_size)
        dt = time.time() - t0
        with Image.open(path) as im0:
            size0 = im0.size
        print(f"{name}  {size0[0]}x{size0[1]} -> {size0[0]*SCALE}x{size0[1]*SCALE}  {dt:.2f}s")
    total_dt = time.time() - total_t0
    print(f"=== {args.model}: {len(files)} images in {total_dt:.2f}s ({total_dt/len(files):.2f}s/img avg) ===")


if __name__ == "__main__":
    main()
