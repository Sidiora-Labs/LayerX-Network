#!/bin/bash

master_url="${FAVICON_MASTER_URL:-$NEXT_PUBLIC_NETWORK_ICON}"
export MASTER_URL="$master_url"

cd ./deploy/tools/favicon-generator
yarn install --frozen-lockfile
node "$(pwd)/index.js"
if [ $? -ne 0 ]; then
    cd ../../../
    exit 1
else
    cd ../../../
    favicon_folder="./public/assets/favicon/"

    echo "⏳ Replacing default favicons with freshly generated pack..."
    if [ -d "$favicon_folder" ]; then
        rm -r "$favicon_folder"
    fi
    mkdir -p "$favicon_folder"
    cp -r ./deploy/tools/favicon-generator/output/* "$favicon_folder"
fi