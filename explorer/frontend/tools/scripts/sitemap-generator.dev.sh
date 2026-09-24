#!/bin/bash

config_file="./configs/envs/.env.eth"

if [ ! -f "$config_file" ]; then
    echo "Error: File '$config_file' not found."
    exit 1
fi

dotenv \
  -e $config_file \
  -- bash -c 'cd ./deploy/tools/sitemap-generator && yarn && yarn next-sitemap'