import type { NextApiRequest, NextApiResponse } from 'next';

export default async function configHandler(req: NextApiRequest, res: NextApiResponse) {
  const publicEnvs = Object.entries(process.env)
    .filter(([ key ]) => key.startsWith('NEXT_PUBLIC_'))
    .sort(([ keyA ], [ keyB ]) => keyA.localeCompare(keyB))
    .reduce((result, [ key, value ]) => {
      result[key] = value || '';
      return result;
    }, {} as Record<string, string>);

  res.status(200).json({
    envs: publicEnvs,
  });
}
