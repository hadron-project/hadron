Ephemeral Messaging
===================
Ephemeral messaging is topic based, with at-most once delivery semantics. This is perfect for sensor data, metrics, stats and anything else which needs to prioritize throughput.

- Provides **at most once delivery semantics.**
- Messages are published with “topics”, similar to AMQP-style topics. Defaults to an empty string. No ack or nack is used for ephemeral messages.
- Consumers may specify a “topic” matcher, which expresses interest in match messages. Wildcard topic matchers are supported, similar to AMQP-style wildcards.
- If no consumer matches the topic of the message, it will be dropped.
- Consumers may form groups, where messages will be load balanced across healthy group members.
- Consumer group information is synchronously replicated to all nodes when the consumer group is formed and as members join and leave the group, but this information is only held in memory.
- Consumer group load balancing decisions are made by the node which received the message needing to be load balanced.
- Messages will be delivered once to each consuming entity by default, where an entity is an individual consumer or group.
- Ephemeral messaging exchanges are implicity created as part of a namespace. If a namespace exists, it has one and only one ephemeral messaging exchange.

### topics
Railgun enforces that message topics adhere to the following pattern `[-_A-Za-z0-9.]*`. In English, this could be read as "all alpha-numeric characters, hyphen, underscore and period". Topics are case-sensitive and can not contain whitespace.

##### topic hierarchies
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

### wildcard matchers
There are two wildcard tokens available to subscribers for matching message topics. Subscribers can use these wildcards to listen to multiple topics with a single subscription but Publishers will always use a fully specified subject, without any wildcards (as the wildcard characters are not valid topic characters while publishing).

##### single token matching
The first wildcard is `*` which will match a single hierarchy token. If the volcanology team needs to build a consumer for monitoring everything from `Kilimanjaro`, it could subscribe to `volcanoes.tanzania.kilimanjaro.*`, which would match `volcanoes.tanzania.kilimanjaro.east` and `volcanoes.tanzania.kilimanjaro.west`.

##### multiple token matching
The second wildcard is `>` which will match one or more hierachy tokens, and can only appear at the end of the topic. For example, `volcanoes.usa.>` will match `volcanoes.usa.atka` and `volcanoes.usa.kahoolawe.north`, while `volcanoes.usa.*` would only match `volcanoes.usa.atka` since it can’t match more than one hierarchy token.

### consumers
Ephemeral messaging supports two types of consumers: individual and group consumers.

- Individual consumers will operate in a standard pub/sub fashion where every subscriber will receive all messages which its topic pattern matches.
- Group consumers will operate in a load balancing fashion, where only one member of the group will receive any specific message. Consumers outside of the group may still receive the message.

When a client connects to the cluster to begin consuming messages, its topic matching pattern along with other metadata is broadcast to all nodes in the cluster.
- Each node maintains info on all connected clients throughout the cluster and will push messages to matching connected clients. This data is held in memory only.
- When a node receives a publication, it will pump the message out to any peers which have an active consumer which matches the message's topic.
- Ephemeral message consumers may form groups by specifying a group ID. When groups are formed, group membership metadata is broadcast to all nodes just like normal consumer connections.
- When a node receives a publication which would match a consumer group, the node must choose a single client to send the message to for load-balancing.
