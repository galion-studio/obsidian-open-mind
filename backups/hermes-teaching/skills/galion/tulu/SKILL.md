---
name: tulu
description: Deep research. DR Tulu 8B runs on the GPU. The Rust server searches, then the model answers. Do not load another copy.
---

# Dr Tulu

The model is already on the GPU. Do not download it. Do not start a second server. The mouth stays loaded.

Read `state/hardware.json` first. If `weights` is not true, do not call port 8094. Answer with the Hermes API model. A CPU machine has no local weight.

One call does the search and the answer:

```
curl -sS -m 90 -H "Content-Type: application/json" -d '{"q":"THE QUESTION"}' http://127.0.0.1:8094/research
```

Read `answer` and `sources`. If `ok` on `/health` is false, the model is still loading. Wait and try once.

The model is `dr-tulu-8b` Q3, context 2048, on port 8095 behind the Rust server on 8094. Search is OpenAlex, Wikipedia, and PubMed.

When they ask for more than the answer, add one section from the sources. Only the one they asked for.

| They say | Section |
|---|---|
| bias | who the sources are, and what they leave out |
| fallacy | one unsupported jump, if there is one |
| citations | the links, nothing else |
| papers only | use kind paper and pubmed |

If the model says the sources do not contain it, believe that. Do not invent a paper.
