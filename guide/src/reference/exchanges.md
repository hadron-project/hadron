Exchanges
=========
These docs are currently under construction.

<!--

A topic-based, at most once delivery messaging system, perfect for ephemeral data.

- Provides **at most once delivery semantics.**
- Messages are published with "topics", similar to AMQP-style topics. Defaults to an empty string. No ack or nack is used for ephemeral messages.
- Consumers may specify a "topic" matcher, which expresses interest in matching messages. Wildcard topic matchers are supported, similar to AMQP-style wildcards.
- If no consumer matches the topic of the message, it will be dropped.
- Consumers may form groups, where messages will be load balanced across healthy group members.
- Consumer group information is synchronously replicated to all nodes when the consumer group is formed and as members join and leave the group, but this information is only held in memory.
- Consumer group load balancing decisions are made by the node which received the message needing to be load balanced.
- Messages will be delivered once to each consumer group matching the message's topic.
- Ephemeral messaging exchanges are implicity created as part of a namespace. Namespaces have one and only one ephemeral messaging exchange.

### Topics
Hadron enforces that message topics adhere to the following pattern `[-_A-Za-z0-9.]*`. In English, this could be read as "all alpha-numeric characters, hyphen, underscore and period". Topics are case-sensitive and can not contain whitespace.

##### Topic Hierarchies
The `.` character is used to create a subject hierarchy. A volcanology team might define the following hierarchy for collecting sensor readings on volcanoes they are monitoring, where volcanoes are grouped by the region they are in, followed by the name of the volcano, then followed by the cardinal point where the sensor is stationed with respect to the center of the volcano.

```
volcanoes.usa
volcanoes.usa.atka
volcanoes.usa.kahoolawe.north
volcanoes.tanzania
volcanoes.tanzania.meru
volcanoes.tanzania.kilimanjaro.east
volcanoes.tanzania.kilimanjaro.west
```

### Wildcard Matchers
There are two wildcard tokens available to subscribers for matching message topics. Subscribers can use these wildcards to listen to multiple topics with a single subscription but Publishers will always use a fully specified subject, without any wildcards (as the wildcard characters are not valid topic characters while publishing).

##### Single-Token Matching
The first wildcard is `*` which will match a single hierarchy token. If the volcanology team needs to build a consumer for monitoring everything from `Kilimanjaro`, it could subscribe to `volcanoes.tanzania.kilimanjaro.*`, which would match `volcanoes.tanzania.kilimanjaro.east` and `volcanoes.tanzania.kilimanjaro.west`.

##### Multi-Token matching
The second wildcard is `>` which will match one or more hierachy tokens, and can only appear at the end of the topic. For example, `volcanoes.usa.>` will match `volcanoes.usa.atka` and `volcanoes.usa.kahoolawe.north`, while `volcanoes.usa.*` would only match `volcanoes.usa.atka` since it can’t match more than one hierarchy token.

### Consumers
Ephemeral message consumers specify a topic matcher and may optionally specify a queue group to begin consuming messages. Every consumer which is part of the same queue group will have messages load balanced across the group. -->
