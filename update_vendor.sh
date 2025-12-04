#!/bin/bash
#Create the .cargo directory if it doesn't exist
mkdir -p .cargo
# Download all dependencies to a local 'vendor' folder and save the necessary config to .cargo/config.toml
cargo vendor > .cargo/config.toml
