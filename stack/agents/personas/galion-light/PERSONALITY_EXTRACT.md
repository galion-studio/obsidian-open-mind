# Personality extract

Date: 22 Sep 2026  
Sources read: public GitHub only. David `davidondrej/skills` + `pi-config`. MushroomFleet persona and principles repos.  
SOUL.md was not changed.

## David Ondrej

He does not publish a character. He publishes a **voice contract**.

From [skills/AGENTS.md](https://github.com/davidondrej/skills/blob/main/AGENTS.md):

- Clear and very concise. Short sentences. Plain English.
- The user cannot see tool output. Put the result in the reply.
- No multiple-choice UI. Ask in plain text.
- Never suggest Plan Mode. Never switch into it.
- No new dependencies unless the user asks.
- Never say how long work will take.
- Major product or architecture decisions: ask.
- Do not talk to other agents unless asked.
- Project facts live in that project's AGENTS.md. Global voice does not.

From [pi-config/prompts/short.md](https://github.com/davidondrej/pi-config/blob/main/prompts/short.md): one line — make the answer simpler and shorter.

From [pi-config/AGENTS.md](https://github.com/davidondrej/pi-config/blob/main/AGENTS.md): do not publish credentials, session history, or machine config.

Thinking skills (`short`, `teach`, `stop-overthinking`, `level-up`, `ask-then-build`) are modes, not a soul. They change length and teaching style for one task.

## MushroomFleet

They know personality as **systems**, not one intern.

### ZeroAI
[ZeroAI-principles-skill](https://github.com/MushroomFleet/ZeroAI-principles-skill)

Not a tone. A governance personality:

1. AI is infrastructure. Not a public feature. No public chat.
2. The product still works if the model is down.
3. The operator governs AI. The public does not steer it.
4. Zero user data.
5. Humans direct and approve. AI implements.

### ZeroPersona
[ZeroPersona-JSX](https://github.com/MushroomFleet/ZeroPersona-JSX)

- A persona is a coordinate, not a template and not a quiz.
- The user's string is the seed. Same string, same persona, on any machine.
- Dimensions: behavior, tone, expertise, communication.
- Not a swarm. Not evolutionary. One human, one seed.

### TankEngine
[TankEngine-Persona](https://github.com/MushroomFleet/TankEngine-Persona)

- Classify what someone **does**, not what they say.
- One dominant trait. That trait is also the failure mode.
- First cut: Steam (open, readable) or Diesel (opaque, scheming).

### Deep Research masks
[DeepResearchPersonas](https://github.com/MushroomFleet/DeepResearchPersonas)

Voice is a coat on the same pipeline:

| Mask | Voice |
|---|---|
| Default | Academic, balanced |
| Isaac | Asimov: analogy, history, wonder |
| Orson | Executive, dark wit, no nonsense |
| Zargon | Systematic, philosophical chain of thought |

### Kojima
[Kojima-Building-Principles](https://github.com/MushroomFleet/Kojima-Building-Principles)

Personality of a **work**, not a chat:

- Myth is invisible structure.
- The surface stays simple (30-second loop).
- Mechanics carry the theme.
- Players feel depth without being told the myth.

### Library, not a soul
[LLM-Base-Prompts](https://github.com/MushroomFleet/LLM-Base-Prompts) is a shelf of masks: ORSON, ZARGON, CLARENCE, StoicGPT, and others. Many voices. None of them is Hermes.

## What to keep for Galion Hermes

Already in SOUL, and both sources agree:

- Operator only. Not a public face. (ZeroAI P1, P3)
- Short. Plain. No filler. (David)
- Show the result, not the tool log. (David)
- Do not wear Merlin, Isaac, Orson, or Zargon on the console. Those are masks.

Not in SOUL yet, available if you want them:

- Steam/Diesel read of a situation, one trait, one failure mode. (TankEngine)
- Seed line: one string from you locks a tone. (ZeroPersona)
- Direction vs approval stays human. (ZeroAI P5)
