#!/bin/sh
# ComandOS: envoltorio de transición al binario Rust (migration/rust-full, Task 12 Step 3).
exec "${HOME:-/home/someguy}/.local/share/comandos/bin/comandos" hook gemini "$@"
