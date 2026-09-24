#!/bin/bash

# Check if the .env file exists
if [ -f .env ]; then
    # Load the environment variables from .env
    source .env
fi

# Check run-time ENVs values integrity
node "$(dirname "$0")/envs-validator/index.js" "$input"
if [ $? != 0 ]; then                   
   exit 1
fi