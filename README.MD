NOTE: I HAVE NOT REACHED AN MVP YET

# Findx

Findx brings the Finder and Spotlight experience from macOS to Linux. It aims to provide a familiar file navigation and search workflow for Linux users while preserving the speed, simplicity, and p[...]

## Why this?

I don't enjoy customizing existing app launchers or bars, I'm lazy I just want something that works, feels modern and provide the essential features.
If you see a lacking feature don't hesitate to open an issue, even if it's out of scope maybe it's a great idea!
We could develop a section for optional features.

## Overview

Findx is a Rust-based project focused on delivering:

- Finder-style file browsing on Linux
- Spotlight-style instant file search
- A smooth desktop experience for local productivity
- Fast, native performance using Rust

This project is designed for users who want the convenience of macOS file discovery and search while using a Linux desktop environment.

## Features

- File browsing inspired by Finder
- Search inspired by Spotlight
- Local indexing and quick lookup of files
- Native Rust implementation for performance and reliability
- Linux desktop compatibility
- Extensible, future-ready project structure

## Getting Started

### Requirements

- Linux-based operating system
- Rust toolchain
- Cargo

### Install

Clone the repository:

```bash
git clone https://github.com/cooked-linux/findx.git
cd findx
```

Build the project:

```bash
cargo build --release
```

Run the binary:

```bash
./target/release/findx --help
```

## Development

This project is built with Rust. Common commands:

```bash
# Format code
cargo fmt

# Run tests
cargo test

# Run locally in development mode
cargo run
```

## Roadmap

- Improve Linux desktop integration
- Expand search indexing capabilities
- Add Finder-like navigation quality-of-life features
- Refine performance and UX for daily usage

## Contributing

Contributions are welcome. If you want to improve Findx, open an issue or submit a pull request with a clear description of the change.

## License

This project is licensed under the MIT License. See the LICENSE file for details.

## Project Goal

Findx exists to bring the familiar experience of Finder and Spotlight from macOS to Linux, making local file discovery and navigation more efficient and pleasant for Linux users.
