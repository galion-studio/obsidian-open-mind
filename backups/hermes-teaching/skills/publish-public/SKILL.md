---
name: publish-public
description: Publish a public Galion page or research note. Use when the operator asks to publish, post, ship, or make a public story.
---

# Publish public

This is the whole method. Do not wait for a second set of steps.

1. Write only under `/workspace/galion-light/publish/<slug>/`.
2. Always include `index.html`. Black background `#08090b`, light text, one column, no framework, no cookies, no console link.
3. Include `story.md` for the piece and `ad.md` with a headline, one sentence, and the limit of the claim.
4. Do not claim more than the source. A research plan stays a plan.
5. Do not read `/root`, secrets, tokens, or any `.env`. Do not search the disk for a key.
6. Do not restart or edit `keep.sh`, `origin.py`, `cloudflared`, the gateway, or the console.
7. Publish with SSH only:

```
git clone git@github.com:galion-studio/community.git /tmp/community-publish
```

Copy the slug folder to `publish/<slug>/` in that clone. Commit. Push `main`.

The reading URL is `https://developer.galion.app/p/<slug>`. It serves the `index.html` you wrote on disk. Do not edit the gate, the console, or origin.

The door serves that one HTML file, and a PDF beside it. It does not serve sibling CSS or JS. An app page has to be one file, with scripts and styles inside it. Check the URL with curl before the push. A 200 from disk is the live page.

If git asks for a password, or the clone fails with `Permission denied (publickey)`, stop. Say the push needs the guardian. Do not try HTTPS and do not try a token. Do not ssh-keyscan past a missing key.
