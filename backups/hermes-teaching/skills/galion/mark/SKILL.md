---
name: mark
description: Turn a black shard image into one filled SVG for Galion One. Use when the operator gives a logo.
---

# Mark

The picture is a solid black shape on white. Look at it with vision, then trace the corners. Do not redraw it by eye.

## Look

```
curl -sS -F file=@/absolute/path.jpg http://127.0.0.1:8645/see
```

Read `caption`. If it is not one dark mark on a light field, stop.

## Trace

```
python3 /workspace/galion-light/one/trace.py /absolute/path.jpg /workspace/galion-light/one/mark.svg
```

The script calls vision, then writes one filled polygon. No curves. No second shape.

## Done

One sentence: the caption. Then the svg path and the corner count.
