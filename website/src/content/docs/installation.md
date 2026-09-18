---
title: Installation
description: Install DBCrab with Cargo, a GitLab release archive, or a local source checkout.
---

## Install a prebuilt release

On a supported Linux or macOS system, or in a POSIX-compatible Windows shell such
as Git Bash, install the latest release with:

```sh
curl -fsSL https://gitlab.com/akhansari/dbcrab/-/raw/main/install.sh | sh
```

The installer automatically selects the archive for your architecture
and verifies its SHA-256 checksum before installation.

The destination is selected in this order:

| Platform | Install-directory precedence |
| --- | --- |
| All | `$INSTALL_DIR` when explicitly set |
| Unix and WSL | `$XDG_BIN_HOME`, then `$HOME/.local/bin` |
| Native Windows shell | `$XDG_BIN_HOME`, `shell:UserProgramFiles`, then `%LOCALAPPDATA%\Programs` |

## Install from crates.io

Install the current release with:

```sh
cargo install --locked dbcrab
```

Confirm that Cargo's binary directory is on your `PATH`:

```sh
dbcrab --version
```

To upgrade an existing installation:

```sh
cargo install --locked dbcrab --force
```

To remove it:

```sh
cargo uninstall dbcrab
```

## Install from a checkout

Use the current source tree when testing an unreleased change:

```sh
git clone https://gitlab.com/akhansari/dbcrab.git
cd dbcrab
cargo install --locked --path .
```

This installs the checkout's `dbcrab` binary into Cargo's binary directory.

## Next step

Continue to the [Quickstart](../quickstart/) to connect and run your first query,
or read [Connecting](../connecting/) for connection-string and startup details.
