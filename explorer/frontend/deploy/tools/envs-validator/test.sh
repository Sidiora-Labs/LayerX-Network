#!/bin/bash

test_folder="./test"
common_file="${test_folder}/.env.common"

# Generate ENV registry file
export NEXT_PUBLIC_GIT_COMMIT_SHA=$(git rev-parse --short HEAD)
export NEXT_PUBLIC_GIT_TAG=$(git describe --tags --abbrev=0)
../../scripts/collect_envs.sh ../../../docs/ENVS.md

# Copy test assets
mkdir -p "./public/assets/configs"
cp -r ${test_folder}/assets ./public/

# Build validator script
yarn build

validate_file() {
    local test_file="$1"
    local with_common="$2"

    echo
    echo "🧿 Validating file '$test_file'..."

    if [ "$with_common" = "true" ]; then
        dotenv -e "$test_file" -e "$common_file" yarn run validate -- --silent
    else
        dotenv -e "$test_file" yarn run validate -- --silent
    fi

    if [ $? -eq 0 ]; then
        echo "👍 All good!"
        return 0
    else
        echo "🛑 The file is invalid. Please fix errors and run script again."
        echo
        return 1
    fi
}

test_files=($(find "$test_folder" -maxdepth 1 -type f | grep -vE '\/\.env\.common$'))

for file in "${test_files[@]}"; do
    if [[ "$file" == *".env.multichain" ]]; then
        validate_file "$file" false
    else
        validate_file "$file" true
    fi
    
    if [ $? -eq 1 ]; then
        exit 1
    fi
done
