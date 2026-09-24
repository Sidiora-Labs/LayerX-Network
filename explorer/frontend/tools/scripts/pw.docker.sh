#!/bin/bash

export NODE_PATH=$(pwd)/node_modules_linux

yarn test:pw "$@"
