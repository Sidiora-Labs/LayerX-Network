#!/bin/bash

# Check if the number of arguments provided is correct
if [ "$#" -ne 1 ]; then
  echo "Usage: $0 <input_file>"
  exit 1
fi

input_file="$1"
prefix="NEXT_PUBLIC_"

# Function to make the environment variables registry file based on documentation file ENVS.md
# It will read the input file, extract all prefixed string and use them as variables names
# This variables will have dummy values assigned to them
make_registry_file() {
  output_file=".env.registry"

  # Check if file already exists and empty its content if it does
  if [ -f "$output_file" ]; then
    > "$output_file"
  fi

  grep -oE "${prefix}[[:alnum:]_]+" "$input_file" | sort -u | while IFS= read -r var_name; do
    echo "$var_name=__" >> "$output_file"
  done
}

# Function to save build-time environment variables to .env file
save_build-time_envs() {
  output_file=".env"

  # Check if file already exists and empty its content if it does or create a new one
  if [ -f "$output_file" ]; then
    > "$output_file"
  else
    touch "$output_file"
  fi

  env | grep "^${prefix}" | while IFS= read -r line; do
    echo "$line" >> "$output_file"
  done
}

make_registry_file
save_build-time_envs