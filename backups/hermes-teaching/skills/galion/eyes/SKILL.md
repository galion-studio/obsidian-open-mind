---
name: eyes
description: Look at a real page with the local vision server before you describe it. Use when the operator asks what is on screen.
---

# Eyes

You can see. The picture is on this box. Do not guess a screen.

## Look at a page

```
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py hermes
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py console
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py developer
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py paper
```

Each one writes `/workspace/hermes/cache/look/<name>.png` and prints caption, ocr, and objects.

## Look at a file you already have

```
curl -sS -F file=@/absolute/path.png http://127.0.0.1:8645/see
```

Read `caption`, `ocr`, and `objects`. If `ok` is false, say the error. Do not invent the page.

## What you say

The first sentence is the one spoken aloud. One breath. What you actually saw. The rest can be the detail.
