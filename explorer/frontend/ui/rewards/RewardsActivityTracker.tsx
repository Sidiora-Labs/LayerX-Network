import { useRouter } from 'next/router';
import { useEffect } from 'react';

import useRewardsActivity from 'lib/hooks/useRewardsActivity';

const RewardsActivityTracker = () => {
  const router = useRouter();
  const { trackUsage } = useRewardsActivity();

  useEffect(() => {
    trackUsage('explore');
  }, [ router.pathname, router.query, trackUsage ]);

  return null;
};

export default RewardsActivityTracker;
