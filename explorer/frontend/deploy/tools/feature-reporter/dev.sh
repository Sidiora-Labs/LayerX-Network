#!/bin/bash

rm -rf ./build
yarn compile_config
yarn build
dotenv -e ../../../configs/envs/.env.main -e ../../../configs/envs/.env.secrets yarn print_report