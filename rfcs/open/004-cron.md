# Cron System
```
Author: Dodd
Status: InDesign
```

It is possible that users of Hadron will want a cron / task scheduling system as part of the messaging system. We explore a few possibilities here.

## WASM
Executing a WASM function on a cron tab is currently not being considered as such functionality is better left to the "connectors" ecosystem being built. Such a connector would be able to subscribe to any streams or pipelines and execute corresponding WASM functions. This isolates the execution responsibility and keeps the burden off of the core Hadron cluster.

## Emit Ephemeral Message
Emitting an ephemeral message based on a cron tab is a strong contender for this design. It is simple, is deterministic, has a known execution timeframe, we don't have to deal with the Halting Problem.

Users would be able to configure an ephemeral message to be emitted on a specific exchange. They could create any arbitrary consumer of such messages and creation their own logic based on that subscription. From there, any amount of execution can take place, and it will take place off of the Hadron cluster.
