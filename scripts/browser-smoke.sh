#!/usr/bin/env bash
set -euo pipefail

cargo test --test browser_smoke -- --nocapture
