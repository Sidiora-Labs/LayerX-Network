import React from 'react';

import PageTitle from 'ui/shared/Page/PageTitle';

import UptimeCharts from './UptimeCharts';
import UptimeStats from './UptimeStats';
import UptimeStatus from './UptimeStatus';
import useUptimeSocketData from './useUptimeSocketData';

const Uptime = () => {

  const { realtimeData, historyData, status, onReconnect } = useUptimeSocketData();

  return (
    <>
      <PageTitle
        title="Uptime"
        contentAfter={ <UptimeStatus status={ status } onReconnect={ onReconnect }/> }
      />
      <UptimeStats realtimeData={ realtimeData }/>
      <UptimeCharts historyData={ historyData }/>
    </>
  );
};

export default React.memo(Uptime);
