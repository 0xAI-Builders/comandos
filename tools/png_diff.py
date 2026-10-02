#!/usr/bin/env python3
"""Compara dos capturas: % de píxeles distintos y una imagen con las diferencias en rojo.
Uso: python3 tools/png_diff.py mockup.png tablero.png diff.png"""
import sys

from PIL import Image, ImageChops

a, b = (Image.open(p).convert("RGB") for p in sys.argv[1:3])
if a.size != b.size:
    sys.exit(f"tamaños distintos: {a.size} vs {b.size}")
diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 24 else 0)
bad = sum(1 for v in diff.getdata() if v)
out = a.copy()
out.paste((255, 0, 0), mask=diff)
out.save(sys.argv[3])
pct = 100 * bad / (a.size[0] * a.size[1])
print(f"{pct:.3f}% de píxeles distintos")
sys.exit(0 if pct <= 0.1 else 1)
