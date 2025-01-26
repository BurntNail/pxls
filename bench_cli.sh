#!/usr/bin/env bash

set -eu
set -o pipefail

export RUSTFLAGS="-C target-cpu=native"
cargo b --release

flamegraph --image-width 5000 -- ./target/release/pxls ./album.jpg 100 15 euclidean output.jpg 32 4 2
xdg-open ./flamegraph.svg


hyperfine "./target/release/pxls ./album.jpg 100 15 euclidean output.jpg 32 4 2" #about 5.1s