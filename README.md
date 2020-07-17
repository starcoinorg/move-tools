# move-language-server
Implementation of Language Server Protocol for [Move language](https://developers.libra.org/docs/crates/move-language).

```shell script
RUST_LOG=info cargo run --bin move-language-server
```

Features:
* check source code files with the official compiler on-the-fly

For the corresponding VSCode extension, see https://marketplace.visualstudio.com/items?itemName=damirka.move-ide

## Configuration

`dialect` - dialect of the Move language. Either `move` (for original Libra version) or `starcoin`. Default is `move`.

`sender_address` - address of the user, used for module imports. Default is `0x0`.

`stdlib_folder` - stdlib folder path. Default is `null`, no stdlib is loaded.

`modules_folders` - array of folder paths for module lookup. Default is empty array.
