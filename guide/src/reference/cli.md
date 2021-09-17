CLI
===
Hadron ships with a native CLI (Command-Line Interface), called `hadron`.

```
The Hadron CLI

USAGE:
    hadron [FLAGS] [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
    -v               Enable debug logging

OPTIONS:
        --token <token>    Set the auth token to use for interacting with the cluster
        --url <url>        Set the URL of the cluster to interact with

SUBCOMMANDS:
    help        Prints this message or the help of the given subcommand(s)
    pipeline    Hadron pipeline interaction
    stream      Hadron stream interaction
```

### hadron stream
```
Hadron stream interaction

USAGE:
    hadron stream <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    help    Prints this message or the help of the given subcommand(s)
    pub     Publish data to a stream
    sub     Subscribe to data on a stream
```

#### hadron stream pub
```
Publish data to a stream

USAGE:
    hadron stream pub [FLAGS] [OPTIONS] <data> --subject <subject> --type <type>

FLAGS:
    -b, --binary     If true, treat the data payload as a base64 encoded binary blob
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -o <optattrs>...           Optional attributes to associate with the given payload
    -s, --subject <subject>    The subject of the new event
    -t, --type <type>          The type of the new event

ARGS:
    <data>    The data payload to be published
```

#### hadron stream sub
```
Subscribe to data on a stream

USAGE:
    hadron stream sub [FLAGS] [OPTIONS] --group <group>

FLAGS:
    -d, --durable            Make the new subscription durable
    -h, --help               Prints help information
        --start-beginning    Start from the first offset of the stream, defaults to latest
        --start-latest       Start from the latest offset of the stream, default
    -V, --version            Prints version information

OPTIONS:
    -b, --batch-size <batch-size>        The batch size to use for this subscription [default: 1]
    -g, --group <group>                  The subscription group to use
        --start-offset <start-offset>    Start from the given offset, defaults to latest
```

### hadron pipeline
```
Hadron pipeline interaction

USAGE:
    hadron pipeline <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    help    Prints this message or the help of the given subcommand(s)
    sub     Subscribe to data on a stream
```

#### hadron pipeline sub
```
Subscribe to data on a stream

USAGE:
    hadron pipeline sub <pipeline> <stage>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <pipeline>    The pipeline to which the subscription should be made
    <stage>       The pipeline stage to process
```
