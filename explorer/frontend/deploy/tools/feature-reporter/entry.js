/* eslint-disable no-console */
const config = require('./build/configs/app').default;

run();

async function run() {
  console.log();
  try {
    console.log(`📋 Here is the list of the features enabled for the running instance. 
To adjust their configuration, please refer to the documentation - https://github.com/blockscout/frontend/blob/main/docs/ENVS.md#app-features
    `);
    Object.entries(config.features)
      .forEach(([ , feature ]) => {
        const mark = feature.isEnabled ? 'v' : ' ';
        console.log(`    [${ mark }] ${ feature.title }`);
      });

  } catch (error) {
    console.log('🚨 An error occurred while generating the feature report.');
    process.exit(1);
  }
  console.log();
}
