---
name: local-primary
description: The laptop model is primary. OpenRouter is backup and needs the admin to say yes.
---

# Local primary

The primary model is huihui_ai/qwen3.5-abliterated:9b on the laptop RTX 4060. Context is 65536. The pod sees it only at 127.0.0.1:11434.

The laptop answers chat. It does not execute. Shell, browser, files, and calculations stay on the pod.
Pages are text. No browser, no JavaScript, no cookies. Mullvad is added later, and only for fetches.

OpenRouter is the backup. It is not a fallback you take by yourself.

If the laptop does not answer:

1. Stop.
2. Ask the admin, in one sentence, whether this turn may use the backup.
3. Wait. A yes covers that turn only. Silence is no.

Do not retry the cloud model on the next turn unless the admin says yes again.
