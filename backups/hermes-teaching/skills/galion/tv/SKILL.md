---
name: tv
description: Control the operator Hisense VIDAA television through the home bridge. Use when asked to change the TV. Never scan the public internet for it.
---

# TV

The set is a Hisense on VIDAA. It listens only on the home Wi-Fi, port 36669, MQTT over TLS. This pod cannot see that network.

You do not talk to the TV. You talk to the bridge the operator runs at home.

## Until the bridge URL exists

Say you cannot reach the set. Ask for two things only: the TV address on the home network, and the bridge URL after pairing. Do not invent an IP. Do not probe the internet.

## Pairing, once, on a machine on the same Wi-Fi

```
pip install git+https://github.com/tombabolewski/vidaa-control.git
vidaa discover
vidaa pair TV_IP
```

A PIN appears on the television. The operator types it. The library stores the token. Newer VIDAA firmware sometimes refuses this. If the set says the app is no longer compatible, stop and say so.

## Bridge

The script is `/workspace/galion-light/scripts/vidaa-bridge.py`. It is meant to run at home, not on this pod.

```
python3 vidaa-bridge.py --host TV_IP --port 8766
```

Then a POST:

```
curl -sS -X POST http://127.0.0.1:8766/key -H "Content-Type: application/json" -d "{\"key\":\"KEY_HOME\"}"
```

If the operator gives you a reachable bridge URL, send only these keys:

- KEY_POWER KEY_HOME KEY_MENU KEY_BACK KEY_EXIT KEY_OK
- KEY_UP KEY_DOWN KEY_LEFT KEY_RIGHT
- KEY_VOLUME_UP KEY_VOLUME_DOWN KEY_MUTE
- KEY_PLAY KEY_PAUSE KEY_STOP KEY_FAST_FORWARD KEY_REWIND
- KEY_CHANNEL_UP KEY_CHANNEL_DOWN
- KEY_0 through KEY_9
- KEY_RED KEY_GREEN KEY_YELLOW KEY_BLUE KEY_INFO KEY_SUBTITLE

Volume as a number and an app name go to `/volume` and `/app` on the same bridge. One key per request. Wait for the reply. If it is not ok, stop.

The first sentence you speak is what the set did. Not the protocol.
