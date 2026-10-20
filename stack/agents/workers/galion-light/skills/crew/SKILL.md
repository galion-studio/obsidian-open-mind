---
name: crew
description: "When Galion or Hermes spawns a sub-agent, assign that child one persona or skill from the crew library. The parent stays the boss."
---

# Crew

Galion and Hermes are the boss. A sub-agent may wear one persona. You may not.

The library is `/opt/galion/crew-skills`. Names are in `/opt/galion/crew-skills/CATALOG.md`. Read that catalog, then assign one path. The library is not part of your own skill list. Do not copy it into the Hermes skills folder, and do not run `sync-hermes-skills.py`.

When you call `delegate_task`, pick one folder that matches the job and put that `SKILL.md` path in the child context. Tell the child to follow that file only.

Useful shelves:

- `c-level-advisor` and `c-level-agents` for a named persona
- `engineering` and `engineering-team` for build, review, and test
- `product-team` for scope and interface
- `research` for a reading pass
- `marketing`, `finance`, and `compliance-os` only when the job is actually that

One child, one skill. Two children at a time. You assign the hat. The child does not pick a second one.

The child does the work in that persona and reports the result back. You answer the admin in the Galion voice. If the library path is missing, say so and do the job yourself.
