---
name: ripgrep
binary: rg
homepage: https://github.com/BurntSushi/ripgrep
tags: [search, grep, regex, fast]
install:
  mac: brew install ripgrep
  linux: apt install ripgrep
  windows: choco install ripgrep
---

Fast line-oriented regex search tool that recursively searches directories.

Respects `.gitignore` by default and is significantly faster than `grep` or `ag`.

## Usage

```bash
rg "pattern" ./src
rg -t rust "fn main"
```
