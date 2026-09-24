import dotenv from 'dotenv';

export async function setup() {
  dotenv.config({ path: './configs/envs/.env.vitest' });
}
