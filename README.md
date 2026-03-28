# swpui

<div align="center">
  <a href="https://github.com/beeb/swpui"><img
      alt="github"
      src="https://img.shields.io/badge/github-beeb%2Fswpui-228b22?style=flat&logo=github"
      height="20"
  /></a>
  <a href="https://crates.io/crates/swpui"><img
      alt="crates.io"
      src="https://img.shields.io/crates/v/swpui.svg?style=flat&color=e37602&logo=rust"
      height="20"
  /></a>
</div>

`swpui` (pronounced "swap UI") is a TUI utility to search and replace text, with a focus on ergonomics, speed and
case-awareness in source code.

Dual-licensed under MIT or Apache 2.0.

## Installation

#### Via `cargo`

```bash
cargo install swpui
```

#### Via [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall)

```bash
cargo binstall swpui
```

## Usage

### Launch the TUI

```bash
$ swp
```

### Keybinds

TODO

## Features

- [x] TODO: add done features
- [x] Multi-line matches and replacement
- [ ] Capture groups in replace
- [ ] Toggle for hidden files
- [ ] Focus pane with mouse
- [ ] Glob to include/exclude files

## Credits

This tool was inspired by [serpl](https://github.com/yassinebridi/serpl), thanks to them for inspiring this project! It
would also not have been possible without the amazing work put into [ratatui](https://ratatui.rs/) and
[rat-widget](https://github.com/thscharler/rat-salsa), thank you!
