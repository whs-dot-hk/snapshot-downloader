# Snapshot Downloader

A Rust tool for downloading, extracting, and setting up Cronos node snapshots.

## Features

- Downloads snapshot and binary tarballs from configured URLs
- Streams downloads to disk with progress indication
- Handles extraction of `.tar.lz4` and `.tar.gz` archives
- Automates Cosmos node initialization and configuration
- Configures node settings via `app.toml` and `config.toml`

## Prerequisites

- Rust and Cargo (1.56.0 or newer)

## Installation

Clone this repository and build the application:

```bash
git clone https://github.com/your-username/snapshot-downloader.git
cd snapshot-downloader
cargo build --release
```

The binary will be available at `target/release/snapshot-downloader`.

## Usage

```bash
# Basic usage with default config.yaml in current directory
./snapshot-downloader

# Specify a custom config file
./snapshot-downloader -c /path/to/my-config.yaml

# Specify output directory
./snapshot-downloader -o /path/to/node

# Enable verbose logging
./snapshot-downloader -v
```

## Configuration File

The configuration is specified in a YAML file. Example `config.yaml`:

```yaml
snapshot_url: https://snapshot.cronos.org/cronos/testnet-snapshot/versiondb/memiavl/cronostestnet_338-3-versiondb-memiavl-20250305.tar.lz4
binary_url: https://github.com/crypto-org-chain/cronos/releases/download/v1.4.4/cronos_1.4.4-testnet_Linux_x86_64.tar.gz
cosmos:
  bin: bin/cronosd
  init_command: init test --chain-id cronostestnet_338-3
  start_command: start
  app:
    minimum-gas-prices: "5000000000000basetcro"
  config:
    moniker: "my-testnet-node"
    fastsync:
      version: "v0"
    rpc:
      laddr: "tcp://0.0.0.0:26657"
    p2p:
      laddr: "tcp://0.0.0.0:26656"
      persistent_peers: "peers-here"
```

### Configuration Options

- `snapshot_url`: URL to download the snapshot tarball (.tar.lz4)
- `binary_url`: URL to download the binary tarball (.tar.gz)
- `cosmos`: Configuration for the Cosmos node
  - `bin`: Relative path to the binary after extraction
  - `init_command`: Command for initializing the node
  - `start_command`: Command for starting the node
  - `app`: Key-value pairs for app.toml configuration
  - `config`: Key-value pairs for config.toml configuration

## Directory Structure

After running the tool, the following directory structure will be created:

```
output_dir/
├── snapshots/
│   ├── [snapshot-archive-file]
│   └── [extracted-snapshot-data]
├── bin_extract/
│   ├── bin/
│   └── ...
└── data/
    ├── config/
    │   ├── app.toml
    │   ├── config.toml
    │   └── ...
    ├── data/
    └── ...
```

## Starting Your Node

After the tool completes successfully, you can start your node with:

```bash
cd output_dir/data
../bin_extract/bin/cronosd start
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.