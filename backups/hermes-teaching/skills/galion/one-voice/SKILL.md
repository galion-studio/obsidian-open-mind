---
name: one-voice
description: Fix Galion One when the phone shows the heard words and then the word Again.
---

# One voice

The ear can work while the mouth is dead. Words on the page come from /hear. The answer comes from /reply.

## What Again means

/reply runs `reply_to` in `/workspace/galion-light/one/talk.py`. Any exception there is thrown away and the page is told `Again`. The traceback is the real fault. Read it. Do not guess.

## Reproduce

Do this from a normal shell, not from inside a voice turn.

```
python3 - << 'PY'
import sys, traceback
sys.path.insert(0, "/workspace/galion-light/one")
import importlib
import talk
importlib.reload(talk)
try:
    say, wav = talk.reply_to("Answer with the word ready and nothing else.")
    print("SAY", say)
    print("WAV", len(wav))
except Exception:
    traceback.print_exc()
PY
```

## Fix

Change only `talk.py`. A missing name, a bad path, or a log write must not erase a sentence that was already spoken. Define the log file. If writing the log fails, still return the sentence and the wav.

Do not restart the gateway. Do not edit the page. Do not edit secrets.

## Done

SAY is a short sentence. WAV is longer than 1000 bytes. Then stop.
