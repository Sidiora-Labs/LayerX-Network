import * as yup from 'yup';
import { urlTest } from '../../utils';

export const megaEthSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_MEGA_ETH_SOCKET_URL_METRICS: yup.string().test(urlTest),
    NEXT_PUBLIC_MEGA_ETH_SOCKET_URL_RPC: yup.string().test(urlTest),
  });