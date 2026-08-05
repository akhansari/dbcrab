---
title: Installation
description: Install DBCrab with Cargo, a GitLab release archive, or a local source checkout.
---

Installing from crates.io with Cargo is the recommended path across platforms
supported by Rust and DBCrab's dependencies. Prebuilt GitLab release archives
are also available for supported Linux and Windows systems.

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

## Install a prebuilt release

On a supported Linux system or in a POSIX-compatible Windows shell such as Git
Bash, install the latest release with:

```sh
curl -fsSL https://gitlab.com/akhansari/dbcrab/-/raw/main/install.sh | sh
```

:::tip[Help wanted: prebuilt macOS releases]
Prebuilt macOS releases are not available yet, so use Cargo on macOS.
If you can help build and test them, [open an issue](https://gitlab.com/akhansari/dbcrab/-/issues)
or submit a merge request on GitLab.
:::

The destination is selected in this order:

| Platform | Install-directory precedence |
| --- | --- |
| All | `$INSTALL_DIR` when explicitly set |
| Unix and WSL | `$XDG_BIN_HOME`, then `$HOME/.local/bin` |
| Native Windows shell | `$XDG_BIN_HOME`, `shell:UserProgramFiles`, then `%LOCALAPPDATA%\Programs` |

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
