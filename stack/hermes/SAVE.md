# Save

2026-09-23. Both machines are on the volume.

- Secrets that used to live only on the container disk are in `/workspace/galion-light/secrets/persist/`. keep.sh copies them back to `/root/galion-secrets` if that disk is empty.
- The sealer skips symlinks, so it cannot overwrite a live key with the word SEALED.
- The Grok computer, without the big source checkout, is `/workspace/galion-light/docs/build-session/grok-computer-2026-09-23.tgz`.
- Console source, Hermes, and the build notes were already on this volume.

R2 is not turned on, so this copy is the one that survives a stop.
