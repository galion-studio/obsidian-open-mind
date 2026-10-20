# Runs

Compared 23 Sep against 24 Sep from the gateway logs. No secrets in this note.

23 Sep: fourteen calls were rejected for a bad API key, all from this machine. Web extract failed on two public pages. The browser supervisor could not connect. Two streams ended without a finish reason. Those runs did not finish cleanly.

24 Sep: the bad-key rejections stopped. One stream dropped at 03:40. After that the agent log is quiet. The gateway process has stayed up since yesterday.

Services at the 14:24 beat, all healthy: console, hermes, whisper, vision, voice, origin, tunnel. Console answers 401 because the phone gate is on. That is healthy, not down.

Guardian now records that beat every minute from keep.sh. It does not restart the gateway. keep.sh still does the restarts.
