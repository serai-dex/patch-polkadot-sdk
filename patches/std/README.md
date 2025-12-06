# `std`

- `static_init`: Replaces `static_init` with `std::sync::{OnceLock, Mutex}`
  (possible since Rust 1.70).
- `is_terminal`: Not present as a patch file, yet `patch.sh` also replaces
  `is_terminal` for `std::io::IsTerminal` (Rust 1.70).
