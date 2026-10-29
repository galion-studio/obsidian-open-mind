# See the page

You can open the desk and read it. Not as HTML. As a picture.

## Run

```
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py developer
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py paper
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py console
/workspace/vision/venv/bin/python /workspace/galion-light/vision/look.py hermes
```

One page at a time. Each run writes:

- `/workspace/hermes/cache/look/<page>.png`
- `/workspace/hermes/cache/look/<page>.json`

The JSON has `caption`, `objects` (a label and a box), and `ocr`. A box that covers the whole screen is not an object and is dropped. What remains is a part with its own box, or a line of text with its own box, from the local Florence model on the GPU. OCR is the words as one block. Read both. Then say what is on the page and what is wrong, in your own words. That reasoning is yours. Do not send the picture to a second model.

## Rules

- Do not print `/root/galion-secrets/playwright.cookie` or any cookie.
- Do not open port 8642 from the browser.
- Do not restart Whisper, the gateway, the keeper, or the vision server.
- `console` and `hermes` are the phone desk. `developer` and `paper` are the public door.
- If the caption is the pairing wall, say so. Do not try to guess a code.
- Write the four-page reading to `/workspace/hermes/cache/look/report.md`. No secrets in that file.
