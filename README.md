# libtaurus

A proof-of-concept implementation of Avalanche consensus, using an ordering
constraint graph instead of a transaction graph to mitigate the multi-coloring
problem. This is a research implementation of the associated paper:
https://github.com/phreaknik/papers/blob/master/avalanche-constraint-graph/paper.pdf

> Disclaimer: This project is research grade and NOT recommended for production use.

## Binaries

### taurusd
Network client daemon for participating in the consensus network.

```
taurusd [OPTIONS]

OPTIONS:
    --bind <ADDR>           Address to bind for P2P connections [default: 0.0.0.0]
    --bootpeer <MULTIADDR>  Specify a boot peer to connect to
    -d, --datadir <PATH>    Specify data directory
    --dhtmode <MODE>        Set Kademlia DHT mode [client|server]
    --loglevel <LEVEL>      Set log level [default: info] [info|debug|trace]
    --port <PORT>           Port number to accept P2P connections [default: 9047]
    --portsearch            Increment P2P port number until an available port is found
    --rpcbind <ADDR>        Address to bind for JSON RPC [default: 127.0.0.1]
    --rpcport <PORT>        Port number to accept JSON RPC http/ws connections [default: 9048]
    --rpcportsearch         Increment RPC port number until an available port is found
    --tmpdir                Use a temporary data directory
```

### taurseq
Sequencer service for producing vertices in the consensus network.

```
taurseq [OPTIONS] -u <URL>

OPTIONS:
    -u, --rpchost <URL>     Host address to reach consensus RPC
    --delay <LEVEL>         Delay (in milliseconds) before producing each new vertex [default: 1000]
    --loglevel <LEVEL>      Set log level [default: info] [info|debug|trace]
```

## Building

```bash
cargo build --release
```
